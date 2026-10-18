"""Run pinned release-plz update in a disposable clone; emit a proposal, never publish."""

import argparse
from datetime import date
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

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
        if any(key in package for key in required if key != "changelog_update"):
            raise ValueError("Package overrides cannot weaken preparation policy.")
    writers = [p for p in packages if p.get("changelog_update")]
    if len(writers) != 1 or writers[0]["name"] != "devoption-cli" or writers[0].get("changelog_path") != "CHANGELOG.md":
        raise ValueError("Only devoption-cli generates the root changelog.")
    if set(writers[0].get("changelog_include", [])) != set(members) - {"devoption-cli"}:
        raise ValueError("The platform changelog must include every other crate.")


def prepare(repository, executable, release_date):
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
        tags = git(scratch, "tag", "--list")
        run([executable, "update", "--release-date", release_date.isoformat(),
             "--repo-url", REPOSITORY_URL], scratch, env)
        after, updated_members = workspace(scratch)
        if updated_members != members:
            raise ValueError("Preparation changed workspace membership.")
        if git(scratch, "tag", "--list") != tags or git(scratch, "rev-parse", "HEAD").strip() != source:
            raise ValueError("Preparation unexpectedly changed Git history or tags.")
        files = git(scratch, "diff", "--name-only").splitlines()
        allowed = {"Cargo.toml", "Cargo.lock", "CHANGELOG.md", *members.values()}
        if set(files) - allowed or git(scratch, "ls-files", "--others", "--exclude-standard").strip():
            raise ValueError("Preparation changed files outside version/changelog scope.")
        patch = git(scratch, "diff", "--no-ext-diff", "--binary")
        report = {"source_commit": source, "release_date": release_date.isoformat(),
                  "tool": TOOL_VERSION, "current_version": before, "proposed_version": after,
                  "changed_files": files, "publication_authorized": False,
                  "limitations": ["Crate-based analysis does not cover all shipped files outside crates."]}
        if "-" in after:
            report["limitations"].append(
                "Prerelease versions require explicit selection; this proposal does not advance or promote them.")
        return report, patch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path.cwd())
    parser.add_argument("--release-plz", type=Path, required=True)
    parser.add_argument("--date", type=date.fromisoformat, default=date.today())
    parser.add_argument("--output", type=Path, required=True, help="New directory outside the source checkout")
    args = parser.parse_args()
    repository = args.repository.resolve()
    output = args.output.resolve()
    if output.is_relative_to(repository) or output.exists():
        parser.error("Output must be a new directory outside the source checkout.")
    report, patch = prepare(repository, args.release_plz, args.date)
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
