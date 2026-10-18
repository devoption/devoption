"""Run pinned release-plz in a disposable clone; emit a proposal, never publish."""

import argparse
from datetime import date
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

from release_preview import preview_changelog, select_preview
from release_metadata import FILENAME, load_metadata, render_metadata, validate_notes
from release_notes import extract_notes

TOOL_VERSION = "release-plz 0.3.172"
REPOSITORY_URL = "https://github.com/devoption/devoption"


def run(command, cwd, env=None):
    return subprocess.run(command, cwd=cwd, env=env, check=True, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=240).stdout


def git(root, *args):
    return run(["git", *args], root)


def workspace(root):
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    package = manifest["workspace"]["package"]
    if package.get("publish") is not False:
        raise ValueError("Platform crates must retain publish = false.")
    members = {}
    for member in manifest["workspace"]["members"]:
        path = root / member / "Cargo.toml"
        crate = tomllib.loads(path.read_text())["package"]
        if crate.get("version") != {"workspace": True} or crate.get("publish") != {"workspace": True}:
            raise ValueError("Every platform crate must inherit version and publish policy.")
        members[crate["name"]] = str(path.relative_to(root))
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    for name in members:
        versions = [p["version"] for p in lock["package"] if p["name"] == name]
        if versions != [package["version"]]:
            raise ValueError(f"Cargo.lock does not match the platform version for {name}.")
    return package["version"], members


def check_config(root, members):
    config = tomllib.loads((root / "release-plz.toml").read_text())
    policy = config["workspace"]
    required = {"git_only": True, "publish": False, "git_tag_enable": False,
                "git_release_enable": False, "dependencies_update": False,
                "semver_check": False, "changelog_update": False}
    if any(policy.get(key) is not value for key, value in required.items()):
        raise ValueError("Release preparation must retain its nonpublishing configuration.")
    packages = config["package"]
    if len(packages) != len(members) or {p["name"] for p in packages} != set(members):
        raise ValueError("Configure every workspace package in the platform version group.")
    for package in packages:
        if package.get("version_group") != "platform":
            raise ValueError("All platform packages must share a version group.")
        if package.get("changelog_path") != "CHANGELOG.md":
            raise ValueError("All platform packages must reference the shared root changelog.")
        if any(key in package for key in required if key != "changelog_update"):
            raise ValueError("Package overrides cannot weaken preparation policy.")
    writers = [p for p in packages if p.get("changelog_update")]
    if len(writers) != 1 or writers[0]["name"] != "devoption-cli" or writers[0].get("changelog_path") != "CHANGELOG.md":
        raise ValueError("Only devoption-cli generates the root changelog.")
    if set(writers[0].get("changelog_include", [])) != set(members) - {"devoption-cli"}:
        raise ValueError("The platform changelog must include every other crate.")


def nonversion_inputs(root, members):
    """Compare parsed manifests/lockfile while allowing only the shared version to change."""
    manifests = {}
    for path in ["Cargo.toml", *members.values()]:
        manifests[path] = tomllib.loads((root / path).read_text())
    del manifests["Cargo.toml"]["workspace"]["package"]["version"]
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    for package in lock["package"]:
        if package["name"] in members:
            del package["version"]
    return manifests, lock


def prepare(repository, executable, release_date, channel="preview"):
    if channel not in {"preview", "analysis"}:
        raise ValueError("Choose preview or analysis; stable promotion is not enabled.")
    repository = repository.resolve()
    if git(repository, "status", "--porcelain").strip():
        raise ValueError("Commit changes before preparing an exact-revision proposal.")
    source = git(repository, "rev-parse", "HEAD").strip()
    if not re.fullmatch(r"[0-9a-f]{40}", source):
        raise ValueError("Expected a full source commit SHA.")
    # No forge token is needed. Keep this local command independent of caller credentials.
    env = {key: value for key, value in os.environ.items()
           if key not in {"GH_TOKEN", "GITHUB_TOKEN", "GIT_TOKEN", "CARGO_REGISTRY_TOKEN"}}
    env["CARGO_NET_OFFLINE"] = "true"
    executable = str(executable.resolve())
    if run([executable, "--version"], repository, env).strip() != TOOL_VERSION:
        raise ValueError(f"Use exactly {TOOL_VERSION}.")
    with tempfile.TemporaryDirectory(prefix="devoption-release-plan-") as directory:
        scratch = Path(directory) / "source"
        run(["git", "clone", "--quiet", "--no-hardlinks", "--no-checkout",
             str(repository), str(scratch)], repository, env)
        git(scratch, "checkout", "--quiet", "--detach", source)
        before, members = workspace(scratch)
        check_config(scratch, members)
        inputs = nonversion_inputs(scratch, members)
        tags = git(scratch, "for-each-ref", "--format=%(refname) %(objectname)", "refs/tags")
        selection = None
        metadata_report = None
        if channel == "analysis":
            run([executable, "update", "--release-date", release_date.isoformat(),
                 "--repo-url", REPOSITORY_URL], scratch, env)
        else:
            selection = select_preview(scratch, before, source, git)
            if selection["shipped_changes"]:
                metadata, digest = load_metadata(scratch, selection["next_version"])
                changelog = scratch / "CHANGELOG.md"
                notes = preview_changelog(changelog.read_text(), selection["next_version"], release_date,
                                          render_metadata(metadata, digest))
                metadata_report = {"path": FILENAME, "version": metadata["version"], "sha256": digest}
                # set-version needs an existing release heading. Give it a temporary
                # standalone section, then install the reviewed human notes unchanged.
                changelog.write_text(f"# Changelog\n\n## [{before}] - {release_date.isoformat()}\n\n- Preparation.\n")
                run([executable, "set-version", selection["next_version"]], scratch, env)
                changelog.write_text(notes)
        after, updated_members = workspace(scratch)
        if metadata_report is not None:
            notes = extract_notes(after, after, (scratch / "CHANGELOG.md").read_text())
            if validate_notes(scratch, after, notes) != metadata_report["sha256"]:
                raise ValueError("Preparation changed release metadata after validation.")
        if updated_members != members:
            raise ValueError("Preparation changed workspace membership.")
        if nonversion_inputs(scratch, members) != inputs:
            raise ValueError("Preparation changed dependency or manifest inputs beyond the platform version.")
        if git(scratch, "for-each-ref", "--format=%(refname) %(objectname)", "refs/tags") != tags or git(scratch, "rev-parse", "HEAD").strip() != source:
            raise ValueError("Preparation unexpectedly changed Git history or tags.")
        files = git(scratch, "diff", "--name-only").splitlines()
        allowed = {"Cargo.toml", "Cargo.lock", "CHANGELOG.md", *members.values()}
        if set(files) - allowed or git(scratch, "ls-files", "--others", "--exclude-standard").strip():
            raise ValueError("Preparation changed files outside version/changelog scope.")
        patch = git(scratch, "diff", "--no-ext-diff", "--binary")
        report = {"source_commit": source, "release_date": release_date.isoformat(),
                  "tool": TOOL_VERSION, "current_version": before, "proposed_version": after,
                  "channel": channel, "changed_files": files, "publication_authorized": False,
                  "release_metadata": metadata_report,
                  "limitations": ["Publication eligibility, provenance and approval gates remain separate and are not satisfied by metadata validation."]}
        if selection is not None:
            report["preview_selection"] = selection
            if after != selection["next_version"]:
                raise ValueError("Tool output does not match the selected preview version.")
        else:
            report["limitations"].append("Analysis diagnostics are not metadata-validated release proposals.")
            report["limitations"].append("Crate-based analysis does not cover all shipped files outside crates.")
        if channel == "analysis" and "-" in after:
            report["limitations"].append(
                "Prerelease versions require explicit selection; this proposal does not advance or promote them.")
        return report, patch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path.cwd())
    parser.add_argument("--release-plz", type=Path, required=True)
    parser.add_argument("--date", type=date.fromisoformat, default=date.today())
    parser.add_argument("--channel", choices=("preview", "analysis"), default="preview")
    parser.add_argument("--output", type=Path, required=True, help="New directory outside the source checkout")
    args = parser.parse_args()
    repository = args.repository.resolve()
    output = args.output.resolve()
    if output.is_relative_to(repository) or output.exists():
        parser.error("Output must be a new directory outside the source checkout.")
    report, patch = prepare(repository, args.release_plz, args.date, args.channel)
    output.mkdir(parents=True)
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (output / "proposal.patch").write_text(patch)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        raise SystemExit(f"Release proposal failed: {error}\n{error.stderr or ''}") from error
    except (ValueError, KeyError, OSError, subprocess.TimeoutExpired) as error:
        raise SystemExit(f"Release proposal failed: {error}") from error
