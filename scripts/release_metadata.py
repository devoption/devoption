"""Validate version-bound release statements and render their required notes block."""

import hashlib
from pathlib import Path
import re
import sys
import tomllib

FILENAME = "release-metadata.toml"
START = "<!-- devoption-release-metadata:start -->"
END = "<!-- devoption-release-metadata:end -->"
FIELDS = {
    "compatibility": {"installation", "upgrade_from", "data", "configuration"},
    "migration": {"required", "instructions"},
    "recovery": {"binary_rollback", "instructions"},
    "support": {"status", "warnings"},
}


def text(value, field):
    if (not isinstance(value, str) or not value.strip() or value != value.strip() or
            len(value) > 2000 or any(ord(c) < 32 or ord(c) == 127 for c in value) or
            "devoption-release-metadata:" in value):
        raise ValueError(f"Release metadata {field} needs nonempty, single-line text (at most 2000 characters).")


def validate_metadata(data, version):
    if not isinstance(data, dict) or set(data) != {"schema_version", "version", "channel", "platforms", *FIELDS}:
        raise ValueError("Release metadata has missing or unknown fields.")
    if type(data["schema_version"]) is not int or data["schema_version"] != 1:
        raise ValueError("Unsupported release metadata schema_version; expected integer 1.")
    if not isinstance(version, str) or not re.fullmatch(
            r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)-preview\.[1-9][0-9]*", version):
        raise ValueError("Release metadata currently supports numbered previews only; stable policy is not enabled.")
    if data["version"] != version:
        raise ValueError(f"Release metadata version must match proposed version {version}; review and update {FILENAME}.")
    if data["channel"] != "preview" or data["platforms"] != ["ubuntu-24.04-amd64"]:
        raise ValueError("Release metadata must retain the supported preview/Ubuntu 24.04 amd64 profile.")
    for section, keys in FIELDS.items():
        if not isinstance(data[section], dict) or set(data[section]) != keys:
            raise ValueError(f"Release metadata {section} has missing or unknown fields.")
    compatibility = data["compatibility"]
    if compatibility["installation"] != "fresh-only" or compatibility["upgrade_from"] != []:
        raise ValueError("Supported platform upgrades are not implemented; metadata must specify fresh-only and no upgrade_from versions.")
    if data["migration"]["required"] is not False:
        raise ValueError("No platform migration is implemented; migration.required must be false.")
    if data["recovery"]["binary_rollback"] is not False:
        raise ValueError("Binary rollback is not supported; recovery.binary_rollback must be false.")
    if data["support"]["status"] != "development-preview":
        raise ValueError("Release metadata support.status must be development-preview.")
    for section, key in [("compatibility", "data"), ("compatibility", "configuration"),
                         ("migration", "instructions"), ("recovery", "instructions")]:
        text(data[section][key], f"{section}.{key}")
    warnings = data["support"]["warnings"]
    if not isinstance(warnings, list) or not 1 <= len(warnings) <= 10:
        raise ValueError("Release metadata support.warnings needs 1 to 10 warnings.")
    for warning in warnings:
        text(warning, "support.warnings")
    if len(set(warnings)) != len(warnings):
        raise ValueError("Release metadata support.warnings must not contain duplicates.")


def load_metadata(root, version):
    path = root / FILENAME
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"A checked-in regular {FILENAME} file is required for a release proposal.")
    with path.open("rb") as stream:
        raw = stream.read(32769)
    if len(raw) > 32768:
        raise ValueError("Release metadata exceeds 32 KiB.")
    try:
        data = tomllib.loads(raw.decode("utf-8"))
    except (UnicodeError, tomllib.TOMLDecodeError) as error:
        raise ValueError("Release metadata must be valid UTF-8 TOML.") from error
    validate_metadata(data, version)
    return data, hashlib.sha256(raw).hexdigest()


def render_metadata(data, digest):
    return "\n".join([
        START, "### Release compatibility and support", "",
        f'- Preview version: `{data["version"]}`; supported platform: Ubuntu 24.04 amd64.',
        "- Installation: fresh only. Upgrades from earlier platform versions are unsupported.",
        f'- Data compatibility: {data["compatibility"]["data"]}',
        f'- Configuration compatibility: {data["compatibility"]["configuration"]}',
        f'- Migration: none implemented. {data["migration"]["instructions"]}',
        f'- Binary rollback: unsupported. {data["recovery"]["instructions"]}',
        "- Support status: development preview.",
        *[f"- Warning: {warning}" for warning in data["support"]["warnings"]],
        f"- Metadata SHA-256: `{digest}` (metadata identity, not artifact provenance).", END,
    ])


def validate_notes(root, version, notes):
    data, digest = load_metadata(root, version)
    expected = render_metadata(data, digest)
    if notes.count(START) != 1 or notes.count(END) != 1 or notes.count(expected) != 1:
        raise ValueError("Release notes must contain exactly the current validated release metadata block; regenerate the proposal.")
    return digest


if __name__ == "__main__":
    try:
        if len(sys.argv) != 2:
            raise ValueError("Usage: python3 scripts/release_metadata.py PROPOSED_PREVIEW_VERSION")
        metadata, checksum = load_metadata(Path(__file__).resolve().parents[1], sys.argv[1])
        print(render_metadata(metadata, checksum))
    except (ValueError, OSError) as error:
        sys.exit(str(error))
