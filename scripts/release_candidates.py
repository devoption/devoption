"""Reserve reviewed preview identities in Git; never tag, publish, sign or install."""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile

from bundle_candidate import check, identity, write_files
from release_bundle import MAX_BINARY, canonical
from release_plan import git
from verify_bundle_provenance import snapshot, source_sha, unique_object

CATALOG = "release-candidates"


def validate(record):
    if not isinstance(record, dict) or set(record) != {"schema_version", "candidate", "artifact"}:
        raise ValueError("Expected a complete release-candidate reservation.")
    candidate, artifact = record["candidate"], record["artifact"]
    expected = identity(candidate["source_commit"], candidate["platform_version"], candidate["archive_sha256"],
                        candidate["proof_sha256"], candidate["run_id"], candidate["run_attempt"])
    if (type(record["schema_version"]) is not int or record["schema_version"] != 1 or
            canonical(candidate) != canonical(expected) or not isinstance(artifact, dict) or
            set(artifact) != {"id", "transport_sha256"} or
            not re.fullmatch(r"[1-9][0-9]{0,19}", artifact["id"]) or
            not re.fullmatch(r"[0-9a-f]{64}", artifact["transport_sha256"])):
        raise ValueError("Reservation schema, candidate identity or artifact locator differs.")
    return record


def load(raw):
    record = validate(json.loads(raw, object_pairs_hook=unique_object))
    if raw != canonical(record):
        raise ValueError("Reservation must use canonical JSON bytes.")
    return record


def verify_transport(transport, record, gh):
    validate(record)
    raw = snapshot(transport, MAX_BINARY + 16 * 1024 * 1024)
    if hashlib.sha256(raw).hexdigest() != record["artifact"]["transport_sha256"]:
        raise ValueError("Transport ZIP differs from reviewed digest.")
    candidate = record["candidate"]
    limits = {candidate["archive"]: MAX_BINARY + 8 * 1024 * 1024,
              "attestation.json": 8 * 1024 * 1024, "candidate.json": 8192}
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        entries = archive.infolist()
        if (len(entries) != 3 or {entry.filename for entry in entries} != set(limits) or
                any(entry.is_dir() or stat.S_IFMT(entry.external_attr >> 16) not in (0, stat.S_IFREG) or
                    not 0 < entry.file_size <= limits[entry.filename] for entry in entries)):
            raise ValueError("Transport ZIP must contain exactly the bounded candidate packet.")
        files = {entry.filename: archive.read(entry) for entry in entries}
    with tempfile.TemporaryDirectory(prefix="devoption-reservation-verify-") as temporary:
        packet = Path(temporary)
        write_files(packet, files)  # Fixed filenames only; never extract downloaded paths or platform contents.
        check(packet, candidate, gh)


def existing_bytes(destination):
    try:
        return snapshot(destination, 8192)
    except ValueError:
        # The winning writer removes its temporary hard link after publication.
        # That single ctime change can overlap the first read without changing bytes.
        # Retry once with the same strict snapshot; continued instability still fails.
        return snapshot(destination, 8192)


def reserve(catalog, record, transport, gh):
    validate(record)
    verify_transport(transport, record, gh)  # Also required on identical retries; unavailable bytes fail closed.
    if catalog.is_symlink():
        raise ValueError("Catalog must not be a symlink.")
    catalog.mkdir(exist_ok=True)
    destination = catalog / (record["candidate"]["platform_version"] + ".json")
    raw = canonical(record)
    with tempfile.TemporaryDirectory(prefix=".candidate-reservation-", dir=catalog.parent) as temporary:
        staged = Path(temporary) / "record.json"
        staged.write_bytes(raw)
        staged.chmod(0o644)
        try:
            os.link(staged, destination)  # Atomic create, never replace; concurrent identical retries reconcile below.
        except FileExistsError:
            if existing_bytes(destination) != raw:
                raise ValueError("Version already reserved for a different candidate; never replace it.")
            return "already_reserved"
    return "reserved"


def tree(repository, revision):
    source_sha(revision)
    entries = git(repository, "ls-tree", "-r", "-z", revision, "--", CATALOG).split("\0")
    result = {}
    for entry in filter(None, entries):
        metadata, path = entry.split("\t", 1)
        mode, kind, blob = metadata.split()
        if mode != "100644" or kind != "blob" or not re.fullmatch(r"release-candidates/[^/]+\.json", path):
            raise ValueError("Catalog entries must be regular nonexecutable version records.")
        size = int(git(repository, "cat-file", "-s", blob).strip())
        if not 0 < size <= 8192:
            raise ValueError("Reservation exceeds its size limit.")
        result[path] = git(repository, "cat-file", "blob", blob).encode()
    return result


def audit(repository, base=None):
    head = git(repository, "rev-parse", "HEAD").strip()
    current = tree(repository, head)
    if base is not None:
        for path, raw in tree(repository, base).items():
            if current.get(path) != raw:
                raise ValueError("Existing version reservations cannot be edited, removed or renamed.")
    for path, raw in current.items():
        candidate = load(raw)["candidate"]
        if path != f"{CATALOG}/{candidate['platform_version']}.json":
            raise ValueError("Reservation filename differs from its version.")
        source = candidate["source_commit"]
        git(repository, "merge-base", "--is-ancestor", source, head)
        manifest = tomllib.loads(git(repository, "show", f"{source}:Cargo.toml"))
        if manifest["workspace"]["package"]["version"] != candidate["platform_version"]:
            raise ValueError("Reserved version differs from the source commit's embedded version.")
    return len(current)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    reservation = sub.add_parser("reserve")
    reservation.add_argument("--record", type=Path, required=True, help="Separately reviewed identity, never from the download")
    reservation.add_argument("--transport", type=Path, required=True)
    reservation.add_argument("--catalog", type=Path, default=Path(CATALOG))
    reservation.add_argument("--gh", type=Path, required=True)
    review = sub.add_parser("audit")
    review.add_argument("--repository", type=Path, default=Path.cwd())
    review.add_argument("--base", help="Full trusted base commit for append-only comparison")
    args = parser.parse_args()
    try:
        if args.mode == "reserve":
            record = load(snapshot(args.record, 8192))
            print(reserve(args.catalog, record, args.transport, args.gh))
        else:
            print(f"Audited {audit(args.repository, args.base)} committed candidate reservations; no publication performed.")
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError, tarfile.TarError,
            zipfile.BadZipFile, RuntimeError, NotImplementedError) as error:
        parser.exit(1, f"Candidate reservation stopped: {error}\n")


if __name__ == "__main__":
    main()
