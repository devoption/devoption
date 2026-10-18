"""Select conservative preview proposals from repository changes and immutable tag baselines."""

from pathlib import PurePosixPath
import re
import subprocess
import tomllib

from check_pr import check_title
from release_notes import extract_notes

VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-(dev|preview\.([1-9][0-9]*)))?")
DOCUMENTS = {"AGENTS.md", "CONTRIBUTING.md", "README.md", "ROADMAP.md", "SECURITY.md", "CHANGELOG.md"}


def version_parts(version):
    match = VERSION.fullmatch(version)
    if not match:
        raise ValueError("Expected a stable version, VERSION-dev or VERSION-preview.N (N >= 1).")
    return tuple(int(match[i]) for i in (1, 2, 3)), match[4], int(match[5]) if match[5] else None


def version_key(version):
    base, suffix, number = version_parts(version)
    return (*base, -1 if suffix == "dev" else 0 if suffix else 1, number or 0)


def shipped_path(path):
    """Unknown paths count: exclusions describe documentation and standalone tests only."""
    parts = PurePosixPath(path).parts
    return not (path in DOCUMENTS or path.startswith("docs/") or
                path.startswith("scripts/test_") or
                (len(parts) > 2 and parts[0] == "crates" and parts[2] == "tests") or
                (path.startswith(".github/") and path.endswith(".md")))


def select_preview(root, current, source, git):
    if git(root, "rev-parse", "--is-shallow-repository").strip() != "false":
        raise ValueError("Preview selection requires complete history and platform tags; fetch unshallow first.")
    base, suffix, number = version_parts(current)
    tags = git(root, "tag", "--list").splitlines()
    platform_tags = [tag for tag in tags if tag.startswith("v") and VERSION.fullmatch(tag[1:])]
    if any(version_key(tag[1:]) > version_key(current) for tag in platform_tags):
        raise ValueError("Workspace version is behind a known platform tag; reconcile the baseline first.")
    baseline_tag = None if suffix == "dev" else f"v{current}"
    baseline_commit = None
    if baseline_tag is None:
        if platform_tags:
            raise ValueError("Development bootstrap requires no existing platform tags.")
    else:
        if baseline_tag not in tags:
            raise ValueError("Current version has no baseline tag; finish or reconcile its publication before preparing another.")
        baseline_commit = git(root, "rev-parse", f"refs/tags/{baseline_tag}^{{commit}}").strip()
        # A tag on another history or a mismatched manifest cannot be an upgrade baseline.
        try:
            git(root, "merge-base", "--is-ancestor", baseline_commit, source)
        except subprocess.CalledProcessError as error:
            raise ValueError("Baseline tag is not an ancestor of the source; reconcile the history first.") from error
        manifest = tomllib.loads(git(root, "show", f"{baseline_commit}:Cargo.toml"))
        if manifest["workspace"]["package"]["version"] != current:
            raise ValueError("Baseline tag and embedded workspace version disagree.")
    changed = (git(root, "diff", "--name-only", "-z", baseline_commit, source) if baseline_commit else
               git(root, "ls-tree", "-r", "--name-only", "-z", source)).split("\0")
    changed = sorted(path for path in changed if path and shipped_path(path))
    selection = {"baseline_tag": baseline_tag, "baseline_commit": baseline_commit,
                 "shipped_changes": changed, "change_kind": None, "next_version": current}
    if not changed:
        return selection
    revisions = git(root, "log", "--no-merges", "--format=%H",
                    f"{baseline_commit}..{source}" if baseline_commit else source).splitlines()
    kind = "patch"
    for revision in revisions:
        paths = git(root, "diff-tree", "--root", "--no-commit-id", "--name-only", "-r", "-z", revision).split("\0")
        if not set(paths).intersection(changed):
            continue
        message = git(root, "show", "-s", "--format=%B", revision)
        title = message.splitlines()[0]
        check_title(title)
        if re.match(r"^[a-z]+(?:\([^\n]+\))?!:", title) or re.search(r"^BREAKING[ -]CHANGE:", message, re.M):
            kind = "breaking"
        elif kind != "breaking" and re.match(r"^feat(?:\([^\n]+\))?:", title):
            kind = "minor"
    if suffix == "dev":
        next_version = ".".join(map(str, base)) + "-preview.1"
    elif suffix:
        next_version = ".".join(map(str, base)) + f"-preview.{number + 1}"
    else:
        major, minor, patch = base
        if kind == "breaking" and major:
            major, minor, patch = major + 1, 0, 0
        elif kind in {"breaking", "minor"}:
            minor, patch = minor + 1, 0
        else:
            patch += 1
        next_version = f"{major}.{minor}.{patch}-preview.1"
    if f"v{next_version}" in tags:
        raise ValueError("Proposed version already has a tag; reconcile it instead of reusing the version.")
    selection.update(change_kind=kind, next_version=next_version)
    return selection


def preview_changelog(changelog, version, release_date):
    headings = list(re.finditer(r"^## .+$", changelog, re.M))
    matches = [i for i, heading in enumerate(headings) if heading.group() == "## [Unreleased]"]
    if len(matches) != 1 or matches[0] != 0:
        raise ValueError("Expected exactly one leading Unreleased changelog section.")
    heading = headings[0]
    end = headings[1].start() if len(headings) > 1 else len(changelog)
    body = changelog[heading.end():end].strip()
    if any(h.group().startswith(f"## [{version}]") for h in headings):
        raise ValueError("Proposed version already has changelog notes; reconcile the existing proposal.")
    section = f"## [{version}] - {release_date.isoformat()}\n\n{body}\n\n"
    # Reuse the release-note content/date/issue checks; -dev remains ineligible.
    extract_notes(version, version, section)
    result = changelog[:heading.end()] + "\n\n" + section + changelog[end:]
    return result if end < len(changelog) else result.rstrip() + "\n"
