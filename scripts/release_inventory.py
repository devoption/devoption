"""Deterministic source inventory and unsigned assembly provenance for candidates."""

import hashlib
import json
import re
import tomllib
from urllib.parse import quote

REPOSITORY = "https://github.com/devoption/devoption"
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
SBOM = "SBOM.cdx.json"
PROVENANCE = "provenance.unsigned.json"
VENDOR = "crates/devoption-ui/assets/vendor/"
VENDOR_FILES = tuple(VENDOR + name for name in ("README.md", "highlight.min.js", "highlight.LICENSE"))


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()


def inventory(files, version, source, workspace_names):
    lock = tomllib.loads(files["Cargo.lock"].decode())
    if lock.get("version") != 4:
        raise ValueError("SBOM requires Cargo.lock version 4.")
    packages = lock["package"]
    identities = {}
    components = []
    for package in packages:
        name, release = package["name"], package["version"]
        if not re.fullmatch(r"[A-Za-z0-9_-]+", name) or not re.fullmatch(r"[0-9A-Za-z.+-]+", release):
            raise ValueError("Invalid locked package identity.")
        key = (name, release)
        if key in identities:
            raise ValueError("Duplicate locked package identity.")
        ref = f"pkg:cargo/{quote(name, safe='')}@{quote(release, safe='')}"
        identities[key] = ref
        component = {"type": "library", "bom-ref": ref, "name": name, "version": release,
                     "purl": ref}
        if package.get("source") == REGISTRY:
            checksum = package.get("checksum", "")
            if not re.fullmatch(r"[0-9a-f]{64}", checksum):
                raise ValueError("Registry package needs its locked source-archive checksum.")
            component["hashes"] = [{"alg": "SHA-256", "content": checksum}]
            component["properties"] = [{"name": "devoption:hash-scope", "value": "crate-source-archive"}]
        elif "source" not in package and name in workspace_names and release == version:
            component["externalReferences"] = [{"type": "vcs", "url": f"{REPOSITORY}/tree/{source}"}]
        else:
            raise ValueError("Unsupported package source or workspace version in SBOM.")
        components.append(component)
    if {p["name"] for p in packages if "source" not in p} != set(workspace_names):
        raise ValueError("SBOM must include exactly the platform workspace packages.")
    dependencies = []
    for package in packages:
        edges = []
        for raw in package.get("dependencies", []):
            parts = raw.split()
            if len(parts) not in (1, 2):
                raise ValueError("Unsupported locked dependency reference.")
            matches = [ref for (name, release), ref in identities.items()
                       if name == parts[0] and (len(parts) == 1 or release == parts[1])]
            if len(matches) != 1:
                raise ValueError("Locked dependency is missing or ambiguous.")
            edges.append(matches[0])
        dependencies.append({"ref": identities[(package["name"], package["version"])],
                             "dependsOn": sorted(set(edges))})
    readme = files[VENDOR + "README.md"].decode()
    tags = re.findall(r"tag `([0-9]+\.[0-9]+\.[0-9]+)`", readme)
    if len(tags) != 1 or f"Highlight.js v{tags[0]}".encode() not in files[VENDOR + "highlight.min.js"][:200]:
        raise ValueError("Embedded Highlight.js version and reviewed vendor record disagree.")
    highlight = f"pkg:npm/%40highlightjs/cdn-assets@{tags[0]}"
    components.append({"type": "library", "bom-ref": highlight, "name": "@highlightjs/cdn-assets",
                       "version": tags[0], "purl": highlight,
                       "hashes": [{"alg": "SHA-256", "content": digest(files[VENDOR + "highlight.min.js"])}],
                       "licenses": [{"license": {"id": "BSD-3-Clause"}}],
                       "properties": [{"name": "devoption:hash-scope", "value": "vendored-browser-build"}]})
    # Payload files have distinct refs from packages; the executable is the metadata component.
    for path, content in sorted(files.items()):
        if path == "bin/devoption":
            continue
        components.append({"type": "file", "bom-ref": "file:" + path, "name": path,
                           "hashes": [{"alg": "SHA-256", "content": digest(content)}]})
    root_ref = "devoption:platform"
    dependencies.append({"ref": root_ref, "dependsOn": sorted(
        [identities[(name, version)] for name in workspace_names] + [highlight])})
    return {
        "$schema": "http://cyclonedx.org/schema/bom-1.6.schema.json",
        "bomFormat": "CycloneDX", "specVersion": "1.6", "version": 1,
        "metadata": {"component": {"type": "application", "bom-ref": root_ref,
                                    "name": "DevOption", "version": version,
                                    "hashes": [{"alg": "SHA-256", "content": digest(files["bin/devoption"])}]},
                     "properties": [
                         {"name": "devoption:source-commit", "value": source},
                         {"name": "devoption:scope", "value": "Full Cargo.lock source inventory and bundle files; includes development and other-platform dependencies. Not a linked-code or host-OS inventory."},
                         {"name": "devoption:license-coverage", "value": "Third-party Cargo licenses are not available from the lockfile and are not inferred."},
                     ]},
        "components": sorted(components, key=lambda c: c["bom-ref"]),
        "dependencies": sorted(dependencies, key=lambda d: d["ref"]),
        "compositions": [{"aggregate": "incomplete", "assemblies": [root_ref]}],
    }


def provenance(manifest, archive_name, archive_digest, manifest_bytes, sbom_bytes, binary_digest, toolchain_digest):
    """Describe assembly of a caller-supplied binary; do not claim compilation or CI identity."""
    source = manifest["source_commit"]
    return {
        "_type": "https://in-toto.io/Statement/v1",
        "subject": [{"name": name, "digest": {"sha256": value}} for name, value in (
            (archive_name, archive_digest), ("manifest.json", digest(manifest_bytes)), (SBOM, digest(sbom_bytes)))],
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": {
            "buildDefinition": {
                "buildType": REPOSITORY + "/blob/" + source + "/docs/release-bundles.md#unsigned-assembly-statement",
                "externalParameters": {"target": manifest["target"], "platformVersion": manifest["platform_version"],
                                       "binaryOrigin": "caller-supplied", "authentication": "unsigned"},
                "resolvedDependencies": [
                    {"uri": "git+" + REPOSITORY, "digest": {"gitCommit": source}},
                    {"name": "bin/devoption", "digest": {"sha256": binary_digest}},
                    {"name": "Cargo.lock", "digest": {"sha256": manifest["cargo_lock_sha256"]}},
                    {"name": "rust-toolchain.toml", "digest": {"sha256": toolchain_digest}},
                    {"name": "release-metadata.toml", "digest": {"sha256": manifest["release_metadata_sha256"]}},
                ],
            },
            "runDetails": {"builder": {"id": REPOSITORY + "/blob/" + source + "/scripts/release_bundle.py"}},
        },
    }
