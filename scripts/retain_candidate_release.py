"""Opted-in exact-main writer: retain a reserved ZIP in an unpublished validation draft."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import Request
import zipfile

from reconcile_candidate_release import PREFIX, REPOSITORY, Reader, expected, reconcile
from release_bundle import MAX_BINARY, canonical
from release_candidates import audit, load, verify_transport
from release_plan import git
from release_pr import GitHub
from verify_bundle_provenance import snapshot, source_sha, unique_object

LIMIT = MAX_BINARY + 16 * 1024 * 1024
WORKFLOW = REPOSITORY + "/.github/workflows/retain-candidate.yml@refs/heads/main"


def require_workflow(repository, source):
    source_sha(source)
    required = {"GITHUB_ACTIONS": "true", "GITHUB_REPOSITORY": REPOSITORY,
                "GITHUB_SERVER_URL": "https://github.com", "GITHUB_REF": "refs/heads/main",
                "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_SHA": source,
                "GITHUB_WORKFLOW_REF": WORKFLOW, "RETAIN_APPROVED": "true", "RETAIN_CHECKS": "success"}
    if any(os.environ.get(key) != value for key, value in required.items()):
        raise ValueError("Retention requires an approved exact-main dispatch and successful source CI.")
    if git(repository, "rev-parse", "HEAD").strip() != source or git(repository, "status", "--porcelain").strip():
        raise ValueError("Retention requires the clean reviewed workflow commit.")


def identifier(value):
    if type(value) is not int or value <= 0:
        raise ValueError("Expected a positive GitHub object ID.")
    return str(value)


class Writer(Reader):
    def __init__(self, gh):
        super().__init__(gh)
        self.rest = GitHub()

    def post(self, path, payload):
        if path not in ("git/refs", "releases"):
            raise ValueError("Retention only creates the reserved tag or validation draft.")
        return self.rest.request("POST", path, payload)

    def download(self, path, destination):
        if not re.fullmatch(r"(?:actions/artifacts/[1-9][0-9]*/zip|releases/assets/[1-9][0-9]*)", path):
            raise ValueError("Download must use one fixed-repository artifact/asset ID.")
        with destination.open("xb") as stream:
            subprocess.run([self.gh, "api", "--hostname", "github.com", "--method", "GET",
                            PREFIX + "/" + path, "-H", "Accept: application/octet-stream"],
                           stdout=stream, stderr=subprocess.PIPE, check=True, timeout=120)
        snapshot(destination, LIMIT)

    def upload(self, release_id, target, transport):
        release_id = identifier(release_id)
        raw = snapshot(transport, LIMIT)
        if len(raw) != target["asset"]["size"] or "sha256:" + hashlib.sha256(raw).hexdigest() != target["asset"]["digest"]:
            raise ValueError("Verified upload bytes changed.")
        url = (f"https://uploads.github.com/repos/{REPOSITORY}/releases/{release_id}/assets?name=" +
               quote(target["asset"]["name"], safe=""))
        request = Request(url, data=raw, headers=self.rest.headers | {"Content-Type": "application/zip"}, method="POST")
        try:
            with self.rest.opener.open(request, timeout=120) as response:
                result = response.read(2097153)
            if len(result) > 2097152:
                raise ValueError("Upload response exceeds its bound.")
            return json.loads(result, object_pairs_hook=unique_object)
        except HTTPError as error:
            raise ValueError(f"Candidate upload failed ({error.code}); inspect/retry without deleting existing assets.") from error


def get_transport(api, record, directory, gh):
    # An existing retained asset is the preferred source after Actions retention expires.
    probe = expected(record, 1)
    state = api.state(probe)
    releases = state["releases"]
    assets = releases[0]["assets"] if len(releases) == 1 else []
    if len(assets) == 1:
        target = expected(record, assets[0]["size"])
        plan = reconcile(target, state)
        source = "releases/assets/" + identifier(plan["observed_asset_id"])
    else:
        reconcile(probe, state)  # Reject conflicts before any artifact fallback.
        artifact_id = record["artifact"]["id"]
        metadata = api.get(PREFIX + "/actions/artifacts/" + artifact_id)
        candidate = record["candidate"]
        if (str(metadata["id"]) != artifact_id or metadata.get("expired") is not False or
                metadata.get("name") != candidate["artifact_name"] or
                metadata.get("digest") != "sha256:" + record["artifact"]["transport_sha256"] or
                str(metadata["workflow_run"]["id"]) != candidate["run_id"] or
                metadata["workflow_run"]["head_sha"] != candidate["source_commit"] or
                type(metadata.get("size_in_bytes")) is not int or not 0 < metadata["size_in_bytes"] <= LIMIT):
            raise ValueError("Original Actions artifact identity, availability or digest differs.")
        source = "actions/artifacts/" + artifact_id + "/zip"
    transport = directory / "source.zip"
    api.download(source, transport)
    verify_transport(transport, record, gh)
    return transport, expected(record, transport.stat().st_size)


def retain(record, api, directory, gh):
    transport, target = get_transport(api, record, directory, gh)
    writes = []
    for step in range(4):
        plan = reconcile(target, api.state(target))  # Never execute a saved plan.
        if "create_reserved_tag" in plan["pending"]:
            result = api.post("git/refs", {"ref": "refs/tags/" + target["tag"], "sha": target["source_commit"]})
            if result["ref"] != "refs/tags/" + target["tag"] or result["object"]["sha"] != target["source_commit"]:
                raise ValueError("Created tag response differs from the reserved source.")
            writes.append("tag")
        elif "create_validation_draft" in plan["pending"]:
            result = api.post("releases", {"tag_name": target["tag"], "target_commitish": target["source_commit"],
                                           "name": target["name"], "body": target["body"], "draft": True,
                                           "prerelease": True, "generate_release_notes": False})
            identifier(result["id"])
            observed = reconcile(target, api.state(target))
            if observed["observed_release_id"] != result["id"]:
                raise ValueError("Created draft ID differs from the observed draft.")
            writes.append("draft")
        elif "upload_exact_reserved_zip" in plan["pending"]:
            result = api.upload(plan["observed_release_id"], target, transport)
            identifier(result["id"])
            observed = reconcile(target, api.state(target))
            if observed["observed_asset_id"] != result["id"]:
                raise ValueError("Uploaded asset ID differs from the observed asset.")
            writes.append("asset")
        else:
            downloaded = directory / f"readback-{step}.zip"
            api.download("releases/assets/" + identifier(plan["observed_asset_id"]), downloaded)
            verify_transport(downloaded, record, gh)
            after = reconcile(target, api.state(target))
            if (after["pending"] != ["download_and_verify_existing_zip"] or
                    any(after[key] != plan[key] for key in ("observed_release_id", "observed_asset_id"))):
                raise ValueError("Remote identity changed during final verification.")
            return {"retained": True, "source_commit": target["source_commit"], "tag": target["tag"],
                    "release_id": after["observed_release_id"], "asset_id": after["observed_asset_id"],
                    "transport_sha256": record["artifact"]["transport_sha256"], "writes": writes,
                    "remote_bytes_verified": True, "published": False, "publication_eligible": False}
    raise ValueError("Remote retention changed repeatedly; inspect before retrying.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--workflow-commit", required=True)
    parser.add_argument("--reservation-sha256", required=True)
    parser.add_argument("--gh", type=Path, required=True)
    args = parser.parse_args()
    try:
        root = Path.cwd()
        require_workflow(root, args.workflow_commit)
        if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+-preview\.[1-9][0-9]*", args.version):
            raise ValueError("Select a reserved numbered preview.")
        audit(root)
        raw = git(root, "show", f"HEAD:release-candidates/{args.version}.json").encode()
        if hashlib.sha256(raw).hexdigest() != args.reservation_sha256:
            raise ValueError("Reservation differs from the independently reviewed digest.")
        record = load(raw)
        with tempfile.TemporaryDirectory(prefix="devoption-retain-release-") as temporary:
            print(json.dumps(retain(record, Writer(args.gh), Path(temporary), args.gh), indent=2))
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError, tarfile.TarError,
            zipfile.BadZipFile, RuntimeError, NotImplementedError) as error:
        parser.exit(1, f"Candidate retention stopped: {error}\n")


if __name__ == "__main__":
    main()
