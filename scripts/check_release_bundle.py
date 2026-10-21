"""Compare a local candidate with its trusted clean checkout and build input.

This is a CI integrity check, not authentication of downloaded releases.
No archive member is extracted or executed.
"""

import argparse
import io
from pathlib import Path
import stat
import tarfile

from release_bundle import MAX_BINARY, TARGET, canonical, payload, sha256, file_mode
from release_inventory import SBOM, PROVENANCE, provenance


def regular(path, limit):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
        raise ValueError("Expected a bounded regular candidate file.")
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ValueError("Candidate file exceeds its size limit.")
    return data


def check(repository, binary, output, runtime=None):
    manifest, files = payload(repository, binary, runtime)
    name = f"devoption-{manifest['platform_version']}-{TARGET}.tar.gz"
    if output.is_symlink() or {p.name for p in output.iterdir()} != {name, "manifest.json", "SHA256SUMS", SBOM, PROVENANCE}:
        raise ValueError("Candidate output must contain exactly its archive, manifest, inventory, unsigned statement and checksums.")
    expected = files | {"manifest.json": canonical(manifest)}
    if regular(output / "manifest.json", 256 * 1024) != expected["manifest.json"]:
        raise ValueError("Candidate manifest differs from the trusted source/build inputs.")
    raw = regular(output / name, MAX_BINARY + 8 * 1024 * 1024)
    if regular(output / "SHA256SUMS", 1024) != f"{sha256(raw)}  {name}\n".encode():
        raise ValueError("Candidate archive checksum differs.")
    if regular(output / SBOM, 2 * 1024 * 1024) != files[SBOM]:
        raise ValueError("Candidate SBOM differs from the trusted source/build inputs.")
    seen = set()
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
        for entry in archive:
            if entry.name not in expected or entry.name in seen or not entry.isfile():
                raise ValueError("Candidate has an unexpected, repeated or nonregular member.")
            seen.add(entry.name)
            content = expected[entry.name]
            mode = file_mode(entry.name)
            if (entry.size != len(content) or entry.mode != mode or
                    (entry.uid, entry.gid, entry.mtime, entry.uname, entry.gname) != (0, 0, 0, "", "")):
                raise ValueError("Candidate member size or normalized metadata differs.")
            with archive.extractfile(entry) as stream:
                if stream.read(len(content) + 1) != content:
                    raise ValueError("Candidate member differs from the trusted source/build inputs.")
    if seen != set(expected):
        raise ValueError("Candidate is missing required members.")
    statement = provenance(manifest, name, sha256(raw), canonical(manifest), files[SBOM],
                           sha256(files["bin/devoption"]), sha256(files["rust-toolchain.toml"]))
    if regular(output / PROVENANCE, 64 * 1024) != canonical(statement):
        raise ValueError("Candidate unsigned provenance subject or materials differ.")
    return sha256(raw)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--repository", type=Path, default=Path.cwd())
    args = parser.parse_args()
    try:
        print("Verified local candidate against trusted source/build inputs: " +
              check(args.repository, args.binary, args.output, args.runtime))
    except (ValueError, KeyError, OSError, tarfile.TarError) as error:
        parser.exit(1, f"Bundle check failed: {error}\n")
