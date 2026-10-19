"""End-to-end retention transitions against a disposable API model; never make remote writes."""

from copy import deepcopy
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from reconcile_candidate_release import PREFIX, Reader, expected
from retain_candidate_release import WORKFLOW, Writer, require_workflow, retain
import test_release_candidates as fixtures
from test_bundle_provenance import GH_VERSION


class Remote:
    def __init__(self, record, transport):
        self.record = record
        self.raw = transport.read_bytes()
        self.target = expected(record, len(self.raw))
        self.commit = None
        self.release = None
        self.posts = []
        self.downloads = []
        self.expired = False
        self.lose_after = None
        self.corrupt = False

    def state(self, target):
        return {"tag_commit": self.commit, "releases": [deepcopy(self.release)] if self.release else []}

    def get(self, path):
        candidate = self.record["candidate"]
        if path != PREFIX + "/actions/artifacts/" + self.record["artifact"]["id"]:
            raise AssertionError(path)
        return {"id": int(self.record["artifact"]["id"]), "expired": self.expired,
                "name": candidate["artifact_name"], "digest": "sha256:" + self.record["artifact"]["transport_sha256"],
                "workflow_run": {"id": int(candidate["run_id"]), "head_sha": candidate["source_commit"]},
                "size_in_bytes": len(self.raw)}

    def completed(self, kind):
        self.posts.append(kind)
        if self.lose_after == len(self.posts):
            raise ValueError("Lost response after successful remote mutation")

    def post(self, path, payload):
        if path == "git/refs":
            if self.commit is not None:
                raise AssertionError("duplicate tag creation")
            self.commit = payload["sha"]
            self.completed("tag")
            return {"ref": payload["ref"], "object": {"sha": self.commit}}
        if path == "releases":
            if self.release is not None:
                raise AssertionError("duplicate release creation")
            self.release = payload | {"id": 12, "immutable": False, "published_at": None, "assets": []}
            self.completed("draft")
            return deepcopy(self.release)
        raise AssertionError("unexpected write endpoint: " + path)

    def upload(self, release_id, target, transport):
        if release_id != 12 or self.release["assets"] or transport.read_bytes() != self.raw:
            raise AssertionError("wrong or duplicate upload")
        asset = target["asset"] | {"id": 34, "state": "uploaded"}
        self.release["assets"] = [asset]
        self.completed("asset")
        return asset

    def download(self, path, destination):
        self.downloads.append(path)
        if path.startswith("actions/") and self.expired:
            raise AssertionError("expired original used instead of retained bytes")
        destination.write_bytes(b"corrupt" if self.corrupt and path.startswith("releases/") else self.raw)


class WriterTests(unittest.TestCase):
    repack = fixtures.ReservationTests.repack
    response = fixtures.ReservationTests.response
    commit = fixtures.ReservationTests.commit
    rezip = fixtures.ReservationTests.rezip

    def setUp(self):
        fixtures.ReservationTests.setUp(self)
        self.remote = Remote(self.record, self.transport)

    def run_retention(self):
        def command(arguments, environment):
            return GH_VERSION.encode() if arguments[-1] == "--version" else self.response()
        with tempfile.TemporaryDirectory() as directory, patch("verify_bundle_provenance.command", side_effect=command):
            return retain(self.record, self.remote, Path(directory), self.gh)

    def test_full_download_tag_draft_upload_readback_and_identical_retry(self):
        result = self.run_retention()
        self.assertTrue(result["retained"])
        self.assertTrue(result["remote_bytes_verified"])
        self.assertFalse(result["published"])
        self.assertFalse(result["publication_eligible"])
        self.assertEqual(result["writes"], ["tag", "draft", "asset"])
        self.assertEqual(self.remote.posts, ["tag", "draft", "asset"])
        self.assertTrue(self.remote.release["draft"])
        self.assertEqual(self.run_retention()["writes"], [])
        self.assertEqual(self.remote.posts, ["tag", "draft", "asset"])

    def test_lost_tag_draft_and_upload_responses_resume_without_duplicates(self):
        for boundary in (1, 2, 3):
            with self.subTest(boundary=boundary):
                self.remote = Remote(self.record, self.transport)
                self.remote.lose_after = boundary
                with self.assertRaisesRegex(ValueError, "Lost response"):
                    self.run_retention()
                self.remote.lose_after = None
                result = self.run_retention()
                self.assertTrue(result["remote_bytes_verified"])
                self.assertEqual(self.remote.posts, ["tag", "draft", "asset"])

    def test_existing_retained_asset_survives_original_actions_expiry(self):
        self.run_retention()
        self.remote.expired = True
        self.remote.downloads.clear()
        self.assertEqual(self.run_retention()["writes"], [])
        self.assertEqual(self.remote.downloads, ["releases/assets/34", "releases/assets/34"])

    def test_missing_original_and_corrupt_source_cannot_create_remote_objects(self):
        self.remote.expired = True
        with self.assertRaisesRegex(ValueError, "availability"):
            self.run_retention()
        self.assertEqual(self.remote.posts, [])
        self.remote.expired = False
        self.remote.raw = b"corrupt source bytes"
        with self.assertRaises(ValueError):
            self.run_retention()
        self.assertEqual(self.remote.posts, [])

    def test_corrupt_remote_readback_fails_without_publishing_or_replacing(self):
        self.remote.corrupt = True
        with self.assertRaisesRegex(ValueError, "Transport ZIP differs"):
            self.run_retention()
        self.assertTrue(self.remote.release["draft"])
        self.assertEqual(self.remote.posts, ["tag", "draft", "asset"])
        self.remote.corrupt = False
        self.assertEqual(self.run_retention()["writes"], [])

    def test_conflicting_tag_published_draft_and_incomplete_asset_stop_without_mutation(self):
        self.remote.commit = "f" * 40
        with self.assertRaisesRegex(ValueError, "never move"):
            self.run_retention()
        self.assertEqual(self.remote.posts, [])
        self.remote.commit = None
        self.run_retention()
        before = self.remote.posts.copy()
        self.remote.release["draft"] = False
        with self.assertRaisesRegex(ValueError, "manual review"):
            self.run_retention()
        self.remote.release["draft"] = True
        self.remote.release["assets"][0]["state"] = "starter"
        with self.assertRaisesRegex(ValueError, "incomplete"):
            self.run_retention()
        self.assertEqual(self.remote.posts, before)

    def test_context_requires_explicit_opt_in_main_exact_commit_and_passing_ci(self):
        context = {"GITHUB_ACTIONS": "true", "GITHUB_REPOSITORY": "devoption/devoption",
                   "GITHUB_SERVER_URL": "https://github.com", "GITHUB_REF": "refs/heads/main",
                   "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_SHA": self.source,
                   "GITHUB_WORKFLOW_REF": WORKFLOW, "RETAIN_APPROVED": "true", "RETAIN_CHECKS": "success"}
        with patch.dict(os.environ, context, clear=True):
            require_workflow(self.repository, self.source)
        for field, value in (("RETAIN_APPROVED", "false"), ("RETAIN_CHECKS", "failure"),
                             ("GITHUB_REF", "refs/pull/1/merge"), ("GITHUB_SHA", "f" * 40)):
            with self.subTest(field=field), patch.dict(os.environ, context | {field: value}, clear=True):
                with self.assertRaisesRegex(ValueError, "approved exact-main"):
                    require_workflow(self.repository, self.source)

    def test_adapter_limits_write_routes_and_uploads_exact_bytes_to_fixed_host(self):
        with patch.dict(os.environ, {"GH_TOKEN": "fixture-not-a-real-token"}), patch("retain_candidate_release.require_workflow") as guard:
            writer = Writer(self.gh, self.repository, self.source)
        guard.assert_called_once_with(self.repository, self.source)
        for path in ("merges", "releases/12", "git/refs/tags/vother"):
            with self.assertRaisesRegex(ValueError, "only creates"):
                writer.post(path, {})
        for path in ("https://other.invalid/asset", "releases/assets/../34", "actions/artifacts/34"):
            with self.assertRaisesRegex(ValueError, "fixed-repository"):
                writer.download(path, self.root / "unused")
        response = io.BytesIO(json.dumps({"id": 34}).encode())
        with patch.object(writer.rest.opener, "open", return_value=response) as upload:
            self.assertEqual(writer.upload(12, self.remote.target, self.transport), {"id": 34})
        request = upload.call_args.args[0]
        self.assertTrue(request.full_url.startswith("https://uploads.github.com/repos/devoption/devoption/releases/12/assets?name="))
        self.assertEqual(request.get_method(), "POST")
        self.assertEqual(request.data, self.transport.read_bytes())
        self.assertEqual(request.get_header("Content-type"), "application/zip")

    def test_workflow_token_inventory_without_user_permissions_and_local_reader_stays_strict(self):
        with patch.dict(os.environ, {"GH_TOKEN": "fixture-not-a-real-token"}), patch("retain_candidate_release.require_workflow"):
            writer = Writer(self.gh, self.repository, self.source)
        paths = []
        def get(path):
            paths.append(path)
            if path == PREFIX:
                return {"full_name": "devoption/devoption"}
            if "/matching-refs/" in path:
                return []
            if path == PREFIX + "/releases?per_page=100&page=1":
                return [self.remote.target | {"id": 12, "tag_name": self.remote.target["tag"]}]
            if path == PREFIX + "/releases/12/assets?per_page=100&page=1":
                return [self.remote.target["asset"] | {"id": 34}]
            self.fail(path)
        with patch.object(writer, "get", side_effect=get):
            state = writer.state(self.remote.target)
        self.assertEqual(state["releases"][0]["id"], 12)
        self.assertEqual(state["releases"][0]["assets"][0]["id"], 34)
        self.assertIn(PREFIX + "/releases/12/assets?per_page=100&page=1", paths)
        with patch.object(writer, "get", return_value={"full_name": "other/repository"}):
            with self.assertRaisesRegex(ValueError, "repository identity"):
                writer.state(self.remote.target)
        reader = Reader(self.gh)
        with patch.object(reader, "get", return_value={"full_name": "devoption/devoption"}):
            with self.assertRaisesRegex(ValueError, "draft inventory"):
                reader.state(self.remote.target)

    def test_writer_cannot_use_workflow_token_exception_outside_approved_context(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(ValueError, "approved exact-main"), patch("retain_candidate_release.GitHub") as client:
                Writer(self.gh, self.repository, self.source)
            client.assert_not_called()

    def test_download_adapter_selects_actions_redirect_and_release_binary_media_types(self):
        with patch.dict(os.environ, {"GH_TOKEN": "fixture-not-a-real-token"}), patch("retain_candidate_release.require_workflow"):
            writer = Writer(self.gh, self.repository, self.source)
        for path, media in (("actions/artifacts/56/zip", "application/vnd.github+json"),
                            ("releases/assets/34", "application/octet-stream")):
            with self.subTest(path=path):
                destination = self.root / path.replace("/", "-")
                def run(arguments, **kwargs):
                    self.assertEqual(arguments, [writer.gh, "api", "--hostname", "github.com", "--method", "GET",
                                                 PREFIX + "/" + path, "-H", "Accept: " + media])
                    self.assertTrue(kwargs["check"])
                    self.assertEqual(kwargs["timeout"], 120)
                    kwargs["stdout"].write(self.transport.read_bytes())
                with patch("retain_candidate_release.subprocess.run", side_effect=run):
                    writer.download(path, destination)
                self.assertEqual(destination.read_bytes(), self.transport.read_bytes())
                with self.assertRaises(FileExistsError):
                    writer.download(path, destination)

    def test_download_error_reports_only_http_status_without_private_stderr(self):
        with patch.dict(os.environ, {"GH_TOKEN": "fixture-not-a-real-token"}), patch("retain_candidate_release.require_workflow"):
            writer = Writer(self.gh, self.repository, self.source)
        failure = subprocess.CalledProcessError(1, "gh", stderr=b"private signed URL and token (HTTP 415)")
        with patch("retain_candidate_release.subprocess.run", side_effect=failure):
            with self.assertRaisesRegex(ValueError, "HTTP 415") as caught:
                writer.download("actions/artifacts/56/zip", self.root / "failed.zip")
        self.assertNotIn("private", str(caught.exception))


if __name__ == "__main__":
    unittest.main()
