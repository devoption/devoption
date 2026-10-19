"""Stage or recheck a short-lived signed candidate; never install or publish it."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile

from release_bundle import MAX_BINARY, TARGET, canonical
from verify_bundle_provenance import snapshot, source_sha, unique_object, verify


def identity(source, version, digest, proof_digest, run_id, attempt):
    source_sha(source)
    if not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)-preview\.[1-9][0-9]*", version):
        raise ValueError("Candidate requires a numbered preview version.")
    if any(not re.fullmatch(r"[0-9a-f]{64}", value) for value in (digest, proof_digest)):
        raise ValueError("Candidate requires archive and proof SHA-256 digests.")
    if any(not re.fullmatch(r"[1-9][0-9]{0,19}", value) for value in (run_id, attempt)):
        raise ValueError("Candidate requires positive run and attempt identifiers.")
    return {"schema_version": 1, "source_commit": source, "platform_version": version,
            "archive": f"devoption-{version}-{TARGET}.tar.gz", "archive_sha256": digest,
            "proof_sha256": proof_digest, "run_id": run_id, "run_attempt": attempt,
            "artifact_name": f"bundle-{source}-{run_id}-{attempt}", "publication_eligible": False}


def packet_bytes(directory, expected):
    if directory.is_symlink() or not directory.is_dir():
        raise ValueError("Candidate must be a regular directory.")
    names = {expected["archive"], "attestation.json", "candidate.json"}
    if {path.name for path in directory.iterdir()} != names:
        raise ValueError("Candidate packet has missing or unexpected files.")
    receipt = snapshot(directory / "candidate.json", 8192)
    if receipt != canonical(expected):
        raise ValueError("Candidate receipt differs from independently supplied identity.")
    archive = snapshot(directory / expected["archive"], MAX_BINARY + 8 * 1024 * 1024)
    proof = snapshot(directory / "attestation.json", 8 * 1024 * 1024)
    if (hashlib.sha256(archive).hexdigest() != expected["archive_sha256"] or
            hashlib.sha256(proof).hexdigest() != expected["proof_sha256"]):
        raise ValueError("Candidate archive or proof digest differs.")
    return {expected["archive"]: archive, "attestation.json": proof, "candidate.json": receipt}


def write_files(directory, files):
    for name, content in files.items():
        with (directory / name).open("xb") as stream:
            stream.write(content)


def check(directory, expected, gh):
    files = packet_bytes(directory, expected)
    # Verify private snapshots, never mutable transport paths or downloaded code.
    with tempfile.TemporaryDirectory(prefix="devoption-candidate-check-") as temporary:
        root = Path(temporary)
        write_files(root, files)
        verify(root / expected["archive"], root / "attestation.json",
               expected["source_commit"], expected["archive_sha256"], expected["platform_version"], gh)
    return expected


def stage(candidate, proof, output, source, run_id, attempt, gh):
    manifest = json.loads(snapshot(candidate / "manifest.json", 256 * 1024), object_pairs_hook=unique_object)
    # Validate before constructing any path from metadata.
    version = manifest["platform_version"]
    preliminary = identity(source, version, "0" * 64, "0" * 64, run_id, attempt)
    if manifest["source_commit"] != source:
        raise ValueError("Candidate source differs from reviewed source.")
    archive = snapshot(candidate / preliminary["archive"], MAX_BINARY + 8 * 1024 * 1024)
    proof_raw = snapshot(proof, 8 * 1024 * 1024)
    expected = identity(source, version, hashlib.sha256(archive).hexdigest(),
                        hashlib.sha256(proof_raw).hexdigest(), run_id, attempt)
    sums = f"{expected['archive_sha256']}  {expected['archive']}\n".encode()
    if snapshot(candidate / "SHA256SUMS", 1024) != sums:
        raise ValueError("Candidate checksum sidecar differs from archive.")
    files = {expected["archive"]: archive, "attestation.json": proof_raw, "candidate.json": canonical(expected)}
    output = output.absolute()
    if output.exists() or output.is_symlink():
        raise ValueError("Candidate output already exists; never replace retained bytes.")
    with tempfile.TemporaryDirectory(prefix="devoption-candidate-stage-") as temporary:
        staging = Path(temporary)
        write_files(staging, files)
        check(staging, expected, gh)
    output.mkdir(mode=0o700)  # Exclusive reservation; collisions and partial retries fail closed.
    write_files(output, files)
    return expected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("stage", "check"))
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--source", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--attempt", required=True)
    parser.add_argument("--gh", type=Path, required=True)
    parser.add_argument("--proof", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--version")
    parser.add_argument("--sha256")
    parser.add_argument("--proof-sha256")
    args = parser.parse_args()
    try:
        if args.mode == "stage":
            if args.proof is None or args.output is None:
                parser.error("stage requires --proof and --output")
            result = stage(args.directory, args.proof, args.output, args.source, args.run_id, args.attempt, args.gh)
            # All fields have fixed formats validated by identity(), with no newlines.
            if os.environ.get("GITHUB_OUTPUT"):
                with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
                    for key in ("platform_version", "archive_sha256", "proof_sha256", "artifact_name", "run_attempt"):
                        stream.write(f"{key}={result[key]}\n")
        else:
            if any(value is None for value in (args.version, args.sha256, args.proof_sha256)):
                parser.error("check requires --version, --sha256 and --proof-sha256")
            expected = identity(args.source, args.version, args.sha256, args.proof_sha256, args.run_id, args.attempt)
            result = check(args.directory, expected, args.gh)
        print(json.dumps(result, indent=2))
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError, tarfile.TarError) as error:
        parser.exit(1, f"Candidate retention stopped: {error}\n")


if __name__ == "__main__":
    main()
