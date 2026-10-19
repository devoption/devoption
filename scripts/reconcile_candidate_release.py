"""Read GitHub state and plan draft candidate retention; perform no remote writes."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import zipfile

from release_bundle import MAX_BINARY, canonical
from release_candidates import audit, load, validate, verify_transport
from release_plan import git
from verify_bundle_provenance import snapshot, source_sha, unique_object

REPOSITORY = "devoption/devoption"
PREFIX = "repos/" + REPOSITORY


def expected(record, size):
    validate(record)
    if type(size) is not int or not 0 < size <= MAX_BINARY + 16 * 1024 * 1024:
        raise ValueError("Expected a bounded verified candidate ZIP.")
    candidate = record["candidate"]
    version = candidate["platform_version"]
    reservation = hashlib.sha256(canonical(record)).hexdigest()
    body = ("Validation candidate only; publication_eligible: false.\n\n"
            f"Reservation SHA-256: {reservation}\nSource: {candidate['source_commit']}\n"
            f"Archive SHA-256: {candidate['archive_sha256']}\n"
            f"Transport SHA-256: {record['artifact']['transport_sha256']}\n")
    return {"tag": "v" + version, "source_commit": candidate["source_commit"],
            "name": f"DevOption {version} validation candidate", "body": body,
            "reservation_sha256": reservation,
            "asset": {"name": candidate["archive"].removesuffix(".tar.gz") + ".candidate.zip",
                      "size": size, "digest": "sha256:" + record["artifact"]["transport_sha256"],
                      "content_type": "application/zip"}}


class Reader:
    def __init__(self, gh):
        self.gh = str(gh.resolve(strict=True))

    def get(self, endpoint):
        result = subprocess.run([self.gh, "api", "--hostname", "github.com", "--method", "GET", endpoint],
                                check=True, capture_output=True, timeout=60)
        if len(result.stdout) > 8 * 1024 * 1024:
            raise ValueError("GitHub response exceeds its bound.")
        return json.loads(result.stdout, object_pairs_hook=unique_object)

    def pages(self, endpoint):
        records = []
        seen = set()
        for page in range(1, 11):
            result = self.get(f"{endpoint}?per_page=100&page={page}")
            if not isinstance(result, list) or len(result) > 100:
                raise ValueError("Expected a bounded GitHub inventory page.")
            for item in result:
                identifier = item["id"]
                if type(identifier) is not int or identifier <= 0 or identifier in seen:
                    raise ValueError("GitHub inventory has invalid or repeated IDs.")
                seen.add(identifier)
                records.append(item)
            if len(result) < 100:
                return records
        raise ValueError("GitHub inventory exceeded its complete-scan bound.")

    def state(self, target):
        repository = self.get(PREFIX)
        if repository.get("full_name") != REPOSITORY or repository.get("permissions", {}).get("push") is not True:
            raise ValueError("Use existing authenticated push access so draft inventory is visible; no writes are made.")
        refs = self.get(PREFIX + "/git/matching-refs/tags/" + target["tag"])
        if not isinstance(refs, list) or len(refs) > 1000:
            raise ValueError("Expected a bounded tag inventory.")
        matches = [ref for ref in refs if ref["ref"] == "refs/tags/" + target["tag"]]
        if len(matches) > 1:
            raise ValueError("Tag inventory is ambiguous.")
        commit = None
        if matches:
            obj = matches[0]["object"]
            seen = set()
            for _ in range(8):
                sha = source_sha(obj["sha"])
                if sha in seen:
                    raise ValueError("Annotated tag chain repeats an object.")
                seen.add(sha)
                if obj["type"] == "commit":
                    commit = sha
                    break
                if obj["type"] != "tag":
                    raise ValueError("Tag must resolve to a commit.")
                tag = self.get(PREFIX + "/git/tags/" + sha)
                if tag["sha"] != sha:
                    raise ValueError("Annotated tag response changed identity.")
                obj = tag["object"]
            else:
                raise ValueError("Annotated tag chain exceeds its bound.")
        releases = [item for item in self.pages(PREFIX + "/releases") if item["tag_name"] == target["tag"]]
        if len(releases) > 1:
            raise ValueError("Multiple releases claim the reserved tag.")
        for release in releases:
            release["assets"] = self.pages(PREFIX + f"/releases/{release['id']}/assets")
        return {"tag_commit": commit, "releases": releases}


def reconcile(target, state):
    pending = []
    commit = state["tag_commit"]
    if commit is None:
        pending.append("create_reserved_tag")
    elif commit != target["source_commit"]:
        raise ValueError("Existing tag points to different source; never move or replace it.")
    releases = state["releases"]
    if not isinstance(releases, list) or len(releases) > 1:
        raise ValueError("Release inventory is ambiguous.")
    release_id = asset_id = None
    if not releases:
        pending += ["create_validation_draft", "upload_exact_reserved_zip"]
    else:
        release = releases[0]
        release_id = release["id"]
        if (type(release_id) is not int or release_id <= 0 or release["tag_name"] != target["tag"] or
                release.get("draft") is not True or release.get("prerelease") is not True or
                release.get("immutable") is not False or "published_at" not in release or release["published_at"] is not None or
                release.get("target_commitish") != target["source_commit"] or
                release.get("name") != target["name"] or release.get("body") != target["body"]):
            raise ValueError("Existing release differs from the reserved validation draft; manual review required.")
        assets = release["assets"]
        if not isinstance(assets, list) or len(assets) > 1:
            raise ValueError("Release has unexpected or duplicate assets; never delete or clobber them.")
        if not assets:
            pending.append("upload_exact_reserved_zip")
        else:
            asset = assets[0]
            asset_id = asset["id"]
            if (type(asset_id) is not int or asset_id <= 0 or asset.get("state") != "uploaded" or
                    type(asset.get("size")) is not int or
                    any(asset.get(key) != value for key, value in target["asset"].items())):
                raise ValueError("Existing asset differs or is incomplete; never overwrite, delete or rebuild it.")
            pending.append("download_and_verify_existing_zip")
    return {"schema_version": 1, "repository": REPOSITORY, "expected": target,
            "observed_release_id": release_id, "observed_asset_id": asset_id,
            "pending": pending, "remote_bytes_verified": False, "writes_performed": False,
            "requires_separate_approval_for_writes": True, "publication_eligible": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--repository", type=Path, default=Path.cwd())
    parser.add_argument("--transport", type=Path, required=True)
    parser.add_argument("--gh", type=Path, required=True)
    args = parser.parse_args()
    try:
        if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+-preview\.[1-9][0-9]*", args.version):
            raise ValueError("Select a reserved numbered preview version.")
        audit(args.repository)
        raw = git(args.repository, "show", f"HEAD:release-candidates/{args.version}.json").encode()
        record = load(raw)
        # Snapshot once so verification and planned upload size refer to the same local bytes.
        transport = snapshot(args.transport, MAX_BINARY + 16 * 1024 * 1024)
        with tempfile.TemporaryDirectory(prefix="devoption-retention-plan-") as temporary:
            archive = Path(temporary) / "candidate.zip"
            archive.write_bytes(transport)
            verify_transport(archive, record, args.gh)
        target = expected(record, len(transport))
        print(json.dumps(reconcile(target, Reader(args.gh).state(target)), indent=2))
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError, tarfile.TarError,
            zipfile.BadZipFile, RuntimeError, NotImplementedError) as error:
        parser.exit(1, f"Candidate reconciliation stopped: {error}\n")


if __name__ == "__main__":
    main()
