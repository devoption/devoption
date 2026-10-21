"""Verify a reviewed archive with pinned gh/Sigstore policy; never extract or install."""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import subprocess
import tarfile
import tempfile
import tomllib

from release_bundle import MAX_BINARY, TARGET, SOURCE_FILES, RUNTIME, file_mode, runtime_versions
from release_inventory import SBOM
from release_metadata import validate_metadata
from release_plan import git

REPOSITORY = "devoption/devoption"
WORKFLOW = REPOSITORY + "/.github/workflows/bundle-provenance.yml"
IDENTITY = "https://github.com/" + WORKFLOW + "@refs/heads/main"
GH_VERSION = "gh version 2.102.0 (2026-09-30)"
PREDICATE = "https://slsa.dev/provenance/v1"


def source_sha(value):
    if not re.fullmatch(r"[0-9a-f]{40}", value):
        raise ValueError("Supply the full independently reviewed source commit.")
    return value


def require_workflow(source, repository):
    source_sha(source)
    expected = {"GITHUB_ACTIONS": "true", "GITHUB_EVENT_NAME": "workflow_dispatch",
                "GITHUB_REPOSITORY": REPOSITORY, "GITHUB_REF": "refs/heads/main",
                "GITHUB_SERVER_URL": "https://github.com", "GITHUB_SHA": source,
                "GITHUB_WORKFLOW_REF": WORKFLOW + "@refs/heads/main",
                "SOURCE_CHECKS": "success", "SIGN_APPROVED": "true"}
    if any(os.environ.get(key) != value for key, value in expected.items()):
        raise ValueError("Signing requires opted-in exact-main context and successful source CI.")
    if git(repository, "rev-parse", "HEAD").strip() != source or git(repository, "status", "--porcelain").strip():
        raise ValueError("Signing requires a clean checkout of the reviewed source.")


def snapshot(path, limit):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as stream:
        before = os.fstat(stream.fileno())
        if not stat.S_ISREG(before.st_mode) or not 0 < before.st_size <= limit:
            raise ValueError("Verification input must be a bounded nonempty regular file.")
        content = stream.read(limit + 1)
        after = os.fstat(stream.fileno())
    fields = ("st_dev", "st_ino", "st_size", "st_mode", "st_mtime_ns", "st_ctime_ns")
    if len(content) != before.st_size or any(getattr(before, k) != getattr(after, k) for k in fields):
        raise ValueError("Verification input changed while reading.")
    return content


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("Repeated JSON key in verification input.")
        result[key] = value
    return result


def command(arguments, environment):
    result = subprocess.run(arguments, env=environment, check=True, capture_output=True, timeout=120)
    if len(result.stdout) > 2 * 1024 * 1024:
        raise ValueError("Verifier output exceeds its limit.")
    return result.stdout


def manifest_from_archive(raw, source, version):
    manifest = None
    seen = set()
    records = {}
    metadata_raw = None
    lock_raw = None
    total = 0
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
        for member in archive:
            path = PurePosixPath(member.name)
            if (not member.isfile() or member.name in seen or path.is_absolute() or
                    ".." in path.parts or str(path) != member.name or str(path) == "."):
                raise ValueError("Archive contains unsafe or repeated members.")
            mode = file_mode(member.name)
            if (member.mode != mode or (member.uid, member.gid, member.mtime, member.uname, member.gname) !=
                    (0, 0, 0, "", "")):
                raise ValueError("Archive member metadata is not normalized.")
            seen.add(member.name)
            total += member.size
            if len(seen) > 128 or total > MAX_BINARY + 8 * 1024 * 1024:
                raise ValueError("Archive inventory exceeds its limits.")
            digest = hashlib.sha256()
            with archive.extractfile(member) as stream:
                while chunk := stream.read(1024 * 1024):
                    digest.update(chunk)
            records[member.name] = {"path": member.name, "size": member.size,
                                    "mode": f"{member.mode:04o}", "sha256": digest.hexdigest()}
            if member.name == "manifest.json":
                if member.size > 256 * 1024:
                    raise ValueError("Archive manifest exceeds its limit.")
                with archive.extractfile(member) as stream:
                    manifest = json.loads(stream.read(), object_pairs_hook=unique_object)
            if member.name == "Cargo.lock":
                if member.size > 2 * 1024 * 1024:
                    raise ValueError("Archive lockfile exceeds its limit.")
                with archive.extractfile(member) as stream:
                    lock_raw = stream.read()
            if member.name == "release-metadata.toml":
                if member.size > 32768:
                    raise ValueError("Archive release metadata exceeds its limit.")
                with archive.extractfile(member) as stream:
                    metadata_raw = stream.read()
    if (not isinstance(manifest, dict) or type(manifest.get("schema_version")) is not int or
            manifest["schema_version"] not in (2, 3) or manifest.get("source_commit") != source or
            manifest.get("platform_version") != version or manifest.get("target") != TARGET or
            manifest.get("publication_eligible") is not False):
        raise ValueError("Authenticated manifest does not match the reviewed preview candidate.")
    validate_metadata(manifest["release_metadata"], version)
    required = {*SOURCE_FILES, "bin/devoption", "RELEASE-NOTES.md", SBOM}
    if manifest["schema_version"] == 3:
        required.add(RUNTIME)
        if (lock_raw is None or hashlib.sha256(lock_raw).hexdigest() != manifest.get("cargo_lock_sha256") or
                manifest.get("embedded_components") != ["server", "CLI", "UI"] or
                manifest.get("app_runtime") != {"path": RUNTIME, "version": version,
                    "embedded_runtime_versions": runtime_versions(tomllib.loads(lock_raw.decode()))}):
            raise ValueError("Authenticated app runtime descriptor or locked identity differs.")
    elif "app_runtime" in manifest:
        raise ValueError("Legacy bundles cannot declare an app runtime payload.")
    declared = manifest.get("files")
    if (not isinstance(declared, list) or len(declared) != len(required) or
            seen != required | {"manifest.json"}):
        raise ValueError("Authenticated archive inventory is incomplete or unexpected.")
    expected = {}
    for record in declared:
        if (not isinstance(record, dict) or set(record) != {"path", "size", "mode", "sha256"} or
                type(record["size"]) is not int or record["path"] in expected):
            raise ValueError("Authenticated manifest has invalid or duplicate file records.")
        expected[record["path"]] = record
    actual = {name: record for name, record in records.items() if name != "manifest.json"}
    if actual != expected:
        raise ValueError("Authenticated archive contents do not match its manifest digests/modes.")
    if (metadata_raw is None or hashlib.sha256(metadata_raw).hexdigest() != manifest["release_metadata_sha256"] or
            tomllib.loads(metadata_raw.decode()) != manifest["release_metadata"]):
        raise ValueError("Authenticated release metadata identity differs.")
    return manifest


def verify(archive, bundle, source, expected_digest, version, gh):
    source_sha(source)
    if not re.fullmatch(r"[0-9a-f]{64}", expected_digest):
        raise ValueError("Supply the independently reviewed archive SHA-256.")
    if not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)-preview\.[1-9][0-9]*", version):
        raise ValueError("Verification supports reviewed numbered previews only.")
    name = f"devoption-{version}-{TARGET}.tar.gz"
    raw = snapshot(archive, MAX_BINARY + 8 * 1024 * 1024)
    if hashlib.sha256(raw).hexdigest() != expected_digest:
        raise ValueError("Archive does not match the independently reviewed digest.")
    proof = snapshot(bundle, 8 * 1024 * 1024)
    gh = str(gh.resolve(strict=True))
    with tempfile.TemporaryDirectory(prefix="devoption-attestation-verify-") as directory:
        root = Path(directory)
        artifact = root / name
        artifact.write_bytes(raw)
        proof_path = root / "attestation.json"
        proof_path.write_bytes(proof)
        # The public-repository bundle verifier needs no caller credentials/config.
        environment = {"PATH": "/usr/bin:/bin", "HOME": directory, "GH_CONFIG_DIR": str(root / "gh"),
                       "GH_PROMPT_DISABLED": "1", "GH_NO_UPDATE_NOTIFIER": "1", "LANG": "C.UTF-8"}
        reported = command([gh, "--version"], environment).decode().splitlines()
        if not reported or reported[0] != GH_VERSION:
            raise ValueError("Use the pinned GitHub CLI 2.102.0 verifier.")
        output = command([
            gh, "attestation", "verify", str(artifact), "--bundle", str(proof_path),
            "--hostname", "github.com", "--repo", REPOSITORY,
            "--cert-identity", IDENTITY,
            "--cert-oidc-issuer", "https://token.actions.githubusercontent.com",
            "--source-ref", "refs/heads/main", "--source-digest", source,
            "--signer-digest", source, "--deny-self-hosted-runners",
            "--predicate-type", PREDICATE, "--format", "json",
        ], environment)
        results = json.loads(output, object_pairs_hook=unique_object)
        if not isinstance(results, list) or not 1 <= len(results) <= 16:
            raise ValueError("Cryptographic verifier returned no bounded verification results.")
        matched = False
        for result in results:
            statement = result["verificationResult"]["statement"]
            if statement.get("predicateType") == PREDICATE and any(
                    subject.get("name") == name and subject.get("digest", {}).get("sha256") == expected_digest
                    for subject in statement["subject"]):
                matched = True
        if not matched:
            raise ValueError("Verified statement has no matching archive subject.")
    manifest_from_archive(raw, source, version)
    return {"authenticated": True, "source_commit": source, "platform_version": version,
            "archive_sha256": expected_digest, "publication_eligible": False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check-workflow", metavar="REVIEWED_SOURCE")
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--source")
    parser.add_argument("--sha256")
    parser.add_argument("--version")
    parser.add_argument("--gh", type=Path)
    args = parser.parse_args()
    try:
        if args.check_workflow:
            require_workflow(args.check_workflow, Path.cwd())
            print("Opted-in exact-main signing context checked; no credential was requested.")
        else:
            if any(value is None for value in (args.archive, args.bundle, args.source, args.sha256, args.version, args.gh)):
                parser.error("verification requires --archive, --bundle, --source, --sha256, --version and --gh")
            print(json.dumps(verify(args.archive, args.bundle, args.source, args.sha256, args.version, args.gh), indent=2))
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError, tarfile.TarError) as error:
        parser.exit(1, f"Provenance verification stopped: {error}\n")
