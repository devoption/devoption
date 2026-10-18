"""Assemble a local Ubuntu preview candidate; never install or publish it."""

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import stat
import subprocess
import tarfile
import tempfile
import tomllib

from release_metadata import load_metadata, validate_notes
from release_notes import extract_notes
from release_plan import git, workspace

TARGET = "ubuntu-24.04-amd64"
MAX_BINARY = 256 * 1024 * 1024
HELPERS = (
    "build_broker.py", "build_worker.py", "deployment_broker.py",
    "prepare_build_config.py", "prepare_platform_service.py",
    "configure_app_hostname.py", "recovery_snapshot.py",
)
DOCUMENTS = (
    "public-linux-install.md", "private-linux-install.md", "web-app-builds.md",
    "web-app-deployments.md", "app-hostnames.md", "same-node-recovery.md",
    "release-bundles.md",
)
SOURCE_FILES = (
    "LICENSE", "Cargo.lock", "rust-toolchain.toml", "release-metadata.toml",
    "CHANGELOG.md", "examples/caddy/Caddyfile",
    "examples/systemd/devoption.service", "examples/systemd/devoption-public.service",
    "examples/systemd/caddy-devoption.service",
    *("scripts/" + name for name in HELPERS),
    *("docs/" + name for name in DOCUMENTS),
)
CADDY_VERSION = "2.11.7"
CADDY_SHA512 = ("a7a433a1b133efc3c8d10eb0b99d52a24b5ef5c322dc77f5282182b1c0402139ab83f3"
                "a99f0c52409df77d20123fb0b523edad8a66d8f5e49136197bf61ef0e7")


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()


def require_host():
    if (platform.system() != "Linux" or platform.machine() != "x86_64" or
            platform.freedesktop_os_release().get("ID") != "ubuntu" or
            platform.freedesktop_os_release().get("VERSION_ID") != "24.04"):
        raise ValueError("Build and test candidates on Ubuntu 24.04 amd64.")


def binary_bytes(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as stream:
        before = os.fstat(stream.fileno())
        if (not stat.S_ISREG(before.st_mode) or
                not 64 <= before.st_size <= MAX_BINARY):
            raise ValueError("Expected a bounded regular platform executable.")
        data = stream.read(MAX_BINARY + 1)
        after = os.fstat(stream.fileno())
    fields = ("st_dev", "st_ino", "st_size", "st_mode", "st_mtime_ns", "st_ctime_ns")
    if len(data) != before.st_size or any(getattr(before, k) != getattr(after, k) for k in fields):
        raise ValueError("Platform executable changed while reading.")
    # ELF64, little endian, current ELF version, x86-64 executable or PIE.
    if (data[:7] != b"\x7fELF\x02\x01\x01" or data[16:18] not in (b"\x02\x00", b"\x03\x00") or
            data[18:20] != b"\x3e\x00"):
        raise ValueError("Expected an x86-64 Linux ELF executable.")
    return data


def source_bytes(repository, source, path):
    entry = git(repository, "ls-tree", source, "--", path).strip()
    if not entry.startswith(("100644 blob ", "100755 blob ")):
        raise ValueError("Bundle input must be a committed regular file: " + path)
    return subprocess.run(["git", "show", f"{source}:{path}"], cwd=repository,
                          check=True, capture_output=True, timeout=30).stdout


def payload(repository, executable):
    if git(repository, "status", "--porcelain").strip():
        raise ValueError("Commit changes before assembling an exact-source candidate.")
    source = git(repository, "rev-parse", "HEAD").strip()
    version, members = workspace(repository)
    metadata, metadata_digest = load_metadata(repository, version)
    files = {name: source_bytes(repository, source, name) for name in SOURCE_FILES}
    notes = extract_notes(version, version, files["CHANGELOG.md"].decode())
    if validate_notes(repository, version, notes) != metadata_digest:
        raise ValueError("Bundle notes and metadata disagree.")
    instructions = files["docs/public-linux-install.md"].decode()
    if CADDY_VERSION not in instructions or CADDY_SHA512 not in instructions:
        raise ValueError("Proxy acquisition pin and installation instructions disagree.")
    files["bin/devoption"] = binary_bytes(executable)
    files["RELEASE-NOTES.md"] = notes.encode()
    toolchain = tomllib.loads(files["rust-toolchain.toml"].decode())["toolchain"]["channel"]
    lock = tomllib.loads(files["Cargo.lock"].decode())
    runtimes = {}
    for name in ("wasmtime", "wasmtime-wasi", "wasmtime-wasi-http"):
        versions = [p["version"] for p in lock["package"] if p["name"] == name]
        if len(versions) != 1:
            raise ValueError("Expected one locked embedded runtime version: " + name)
        runtimes[name] = versions[0]
    manifest = {
        "schema_version": 1, "platform_version": version, "source_commit": source,
        "target": TARGET, "channel": "preview", "publication_eligible": False,
        "provenance": "not-attested", "sbom": "not-generated",
        "components": {name: version for name in sorted(members)},
        "embedded_components": ["server", "CLI", "UI", "Wasmtime"],
        "embedded_runtime_versions": runtimes,
        "cargo_lock_sha256": sha256(files["Cargo.lock"]),
        "release_metadata_sha256": metadata_digest,
        "release_metadata": metadata,
        "external_requirements": {
            "host": "Dedicated Ubuntu 24.04 amd64 with systemd and cgroup v2",
            "os_packages": ["python3 (3.11+)", "git", "sudo"],
            "os_acquisition": "Ubuntu signed package archives; record installed package versions in host acceptance evidence",
            "app_build_toolchain": {"rust": toolchain, "target": "wasm32-wasip2",
                                    "instructions": "docs/web-app-builds.md"},
            "proxy": {"name": "Caddy", "version": CADDY_VERSION,
                      "url": f"https://github.com/caddyserver/caddy/releases/download/v{CADDY_VERSION}/caddy_{CADDY_VERSION}_linux_amd64.tar.gz",
                      "sha512": CADDY_SHA512, "instructions": "docs/public-linux-install.md"},
        },
        "files": [{"path": path, "size": len(data), "sha256": sha256(data),
                   "mode": "0755" if path == "bin/devoption" else "0644"}
                  for path, data in sorted(files.items())],
    }
    if git(repository, "rev-parse", "HEAD").strip() != source or git(repository, "status", "--porcelain").strip():
        raise ValueError("Source checkout changed during assembly.")
    return manifest, files


def probe(binary, version):
    environment = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"}
    result = subprocess.run([str(binary), "--version"], env=environment, check=True,
                            capture_output=True, timeout=10)
    if result.stdout != f"devoption {version}\n".encode() or result.stderr:
        raise ValueError("Packaged executable does not report the selected version.")
    result = subprocess.run([str(binary), "--help"], env=environment, check=True,
                            capture_output=True, timeout=10)
    if b"Usage: devoption" not in result.stdout or result.stderr:
        raise ValueError("Packaged executable help check failed.")


def write_archive(path, files, manifest):
    entries = files | {"manifest.json": canonical(manifest)}
    with path.open("xb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive:
            for name, content in sorted(entries.items()):
                entry = tarfile.TarInfo(name)
                entry.size = len(content)
                entry.mode = 0o755 if name == "bin/devoption" else 0o644
                archive.addfile(entry, io.BytesIO(content))


def assemble(repository, executable, output):
    require_host()
    repository = repository.resolve(strict=True)
    output = output.absolute()
    if output.is_relative_to(repository) or output.exists() or output.is_symlink():
        raise ValueError("Use a new output directory outside the source checkout.")
    # Resolve the parent before creating anything; aliases cannot select the checkout.
    output = output.parent.resolve(strict=True) / output.name
    if output.is_relative_to(repository):
        raise ValueError("Output must be outside the source checkout.")
    manifest, files = payload(repository, executable)
    name = f"devoption-{manifest['platform_version']}-{TARGET}.tar.gz"
    with tempfile.TemporaryDirectory(prefix="devoption-bundle-", dir=output.parent) as temporary:
        staging = Path(temporary)
        binary = staging / "devoption"
        binary.write_bytes(files["bin/devoption"])
        binary.chmod(0o700)
        # Execute only the snapshot whose bytes are subsequently archived.
        probe(binary, manifest["platform_version"])
        archive = staging / name
        write_archive(archive, files, manifest)
        archive_data = archive.read_bytes()
        digest = sha256(archive_data)
        outputs = {name: archive_data, "manifest.json": canonical(manifest),
                   "SHA256SUMS": f"{digest}  {name}\n".encode()}
        output.mkdir(mode=0o700)  # Exclusive reservation; never replace an existing candidate.
        for filename, content in outputs.items():
            with (output / filename).open("xb") as stream:
                stream.write(content)
    return {"archive": name, "sha256": digest, "source_commit": manifest["source_commit"],
            "platform_version": manifest["platform_version"], "publication_eligible": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path.cwd())
    parser.add_argument("--binary", type=Path, required=True,
                        help="Trusted locally built release executable; its version/help will run")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(assemble(args.repository, args.binary, args.output), indent=2))
    except (ValueError, KeyError, OSError, subprocess.SubprocessError, tarfile.TarError) as error:
        parser.exit(1, f"Bundle not assembled: {error}\n")


if __name__ == "__main__":
    main()
