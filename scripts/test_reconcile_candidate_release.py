"""Exercise read-only observations and conservative retry plans; no release writes."""

from copy import deepcopy
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

from reconcile_candidate_release import PREFIX, Reader, expected, reconcile
from release_candidates import load

ROOT = Path(__file__).resolve().parents[1]


class ReconciliationTests(unittest.TestCase):
    def setUp(self):
        self.record = load((ROOT / "release-candidates/0.1.0-preview.1.json").read_bytes())
        self.target = expected(self.record, 1234)
        self.source = self.target["source_commit"]

    def release(self):
        return {"id": 12, "tag_name": self.target["tag"], "name": self.target["name"],
                "body": self.target["body"], "target_commitish": self.source, "draft": True,
                "prerelease": True, "immutable": False, "published_at": None, "assets": []}

    def asset(self):
        return {"id": 34, "state": "uploaded", **self.target["asset"]}

    def test_empty_and_interrupted_states_plan_only_missing_work(self):
        empty = reconcile(self.target, {"tag_commit": None, "releases": []})
        self.assertEqual(empty["pending"], ["create_reserved_tag", "create_validation_draft", "upload_exact_reserved_zip"])
        tagged = reconcile(self.target, {"tag_commit": self.source, "releases": []})
        self.assertEqual(tagged["pending"], ["create_validation_draft", "upload_exact_reserved_zip"])
        draft = reconcile(self.target, {"tag_commit": self.source, "releases": [self.release()]})
        self.assertEqual(draft["pending"], ["upload_exact_reserved_zip"])
        missing_tag = reconcile(self.target, {"tag_commit": None, "releases": [self.release()]})
        self.assertEqual(missing_tag["pending"], ["create_reserved_tag", "upload_exact_reserved_zip"])
        self.assertFalse(empty["writes_performed"])
        self.assertFalse(empty["publication_eligible"])
        self.assertTrue(empty["requires_separate_approval_for_writes"])

    def test_matching_metadata_still_requires_downloaded_byte_verification(self):
        release = self.release() | {"assets": [self.asset()]}
        plan = reconcile(self.target, {"tag_commit": self.source, "releases": [release]})
        self.assertEqual(plan["pending"], ["download_and_verify_existing_zip"])
        self.assertFalse(plan["remote_bytes_verified"])
        self.assertEqual(plan["observed_asset_id"], 34)
        self.assertEqual(plan["observed_release_id"], 12)
        self.assertNotIn("publish", plan["pending"])

    def test_wrong_tag_source_never_produces_a_repair_plan(self):
        with self.assertRaisesRegex(ValueError, "never move"):
            reconcile(self.target, {"tag_commit": "f" * 40, "releases": []})

    def test_published_immutable_or_edited_releases_require_manual_review(self):
        for key, value in (("draft", False), ("prerelease", False), ("immutable", True),
                           ("published_at", "2026-10-19T00:00:00Z"), ("target_commitish", "main"),
                           ("name", "other"), ("body", "unrelated notes"), ("tag_name", "v9.0.0")):
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "manual review"):
                reconcile(self.target, {"tag_commit": self.source, "releases": [self.release() | {key: value}]})

    def test_conflicting_unknown_duplicate_and_incomplete_assets_are_never_clobbered(self):
        cases = [[self.asset() | {"digest": None}], [self.asset() | {"digest": "sha256:" + "0" * 64}],
                 [self.asset() | {"state": "starter"}], [self.asset() | {"size": 0}],
                 [self.asset() | {"name": "other.zip"}], [self.asset(), self.asset()]]
        for assets in cases:
            with self.subTest(assets=assets), self.assertRaises(ValueError):
                reconcile(self.target, {"tag_commit": self.source, "releases": [self.release() | {"assets": assets}]})

    def test_reader_always_uses_explicit_get_and_github_hostname(self):
        reader = Reader(Path("/usr/bin/true"))
        response = subprocess.CompletedProcess([], 0, stdout=b'{"ok":true}')
        with patch("reconcile_candidate_release.subprocess.run", return_value=response) as run:
            self.assertEqual(reader.get(PREFIX), {"ok": True})
        self.assertEqual(run.call_args.args[0][1:], ["api", "--hostname", "github.com", "--method", "GET", PREFIX])
        self.assertNotIn("input", run.call_args.kwargs)
        with patch("reconcile_candidate_release.subprocess.run", side_effect=subprocess.CalledProcessError(403, "gh")):
            with self.assertRaises(subprocess.CalledProcessError):
                reader.get(PREFIX)

    def test_annotated_tag_is_resolved_and_draft_assets_are_separately_paginated(self):
        reader = Reader(Path("/usr/bin/true"))
        annotation = "b" * 40
        paths = []
        release = self.release()
        def get(path):
            paths.append(path)
            if path == PREFIX:
                return {"full_name": "devoption/devoption", "permissions": {"push": True}}
            if "/matching-refs/" in path:
                return [{"ref": "refs/tags/" + self.target["tag"], "object": {"type": "tag", "sha": annotation}},
                        {"ref": "refs/tags/" + self.target["tag"] + "0", "object": {"type": "commit", "sha": "f" * 40}}]
            if path.endswith(annotation):
                return {"sha": annotation, "object": {"type": "commit", "sha": self.source}}
            if path == PREFIX + "/releases?per_page=100&page=1":
                return [deepcopy(release)]
            if path == PREFIX + "/releases/12/assets?per_page=100&page=1":
                return [self.asset()]
            self.fail(path)
        with patch.object(reader, "get", side_effect=get):
            state = reader.state(self.target)
        self.assertEqual(state["tag_commit"], self.source)
        self.assertEqual(state["releases"][0]["assets"], [self.asset()])
        self.assertIn(PREFIX + "/releases/12/assets?per_page=100&page=1", paths)

    def test_incomplete_visibility_annotation_cycles_and_duplicate_releases_fail(self):
        reader = Reader(Path("/usr/bin/true"))
        with patch.object(reader, "get", return_value={"full_name": "devoption/devoption", "permissions": {"push": False}}):
            with self.assertRaisesRegex(ValueError, "draft inventory"):
                reader.state(self.target)
        repository = {"full_name": "devoption/devoption", "permissions": {"push": True}}
        refs = [{"ref": "refs/tags/" + self.target["tag"], "object": {"sha": "b" * 40, "type": "tag"}}]
        tag = {"sha": "b" * 40, "object": {"sha": "b" * 40, "type": "tag"}}
        with patch.object(reader, "get", side_effect=[repository, refs, tag]):
            with self.assertRaisesRegex(ValueError, "repeats"):
                reader.state(self.target)
        with patch.object(reader, "get", side_effect=[repository, [], [self.release(), self.release() | {"id": 13}]]):
            with self.assertRaisesRegex(ValueError, "Multiple releases"):
                reader.state(self.target)

    def test_pagination_cannot_silently_truncate_or_repeat_items(self):
        reader = Reader(Path("/usr/bin/true"))
        page = [{"id": i + 1} for i in range(100)]
        with patch.object(reader, "get", side_effect=[page, [{"id": 101}]]):
            self.assertEqual(len(reader.pages(PREFIX + "/releases")), 101)
        with patch.object(reader, "get", side_effect=[page, [page[0]]]):
            with self.assertRaisesRegex(ValueError, "repeated"):
                reader.pages(PREFIX + "/releases")
        with patch.object(reader, "get", side_effect=[[{"id": 100 * n + i + 1} for i in range(100)] for n in range(10)]):
            with self.assertRaisesRegex(ValueError, "complete-scan bound"):
                reader.pages(PREFIX + "/releases")


if __name__ == "__main__":
    unittest.main()
