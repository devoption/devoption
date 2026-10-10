"""Validate issue-linked PR/commit metadata and require a changelog change."""

import json
import os
from pathlib import Path
import re
import subprocess

CONVENTIONAL = re.compile(
    r"(?:feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)"
    r"(?:\([a-zA-Z0-9_/-]+\))?!?: .+"
)
ISSUE = re.compile(r"^(?:Refs|Closes|Fixes|Resolves) #([1-9][0-9]*)[ \t]*$", re.M | re.I)


def issue_references(text):
    numbers = set(ISSUE.findall(text))
    if not numbers:
        raise ValueError("Include a line such as Refs #123 or Closes #123.")
    return numbers


def check_title(title):
    if not CONVENTIONAL.fullmatch(title):
        raise ValueError("Use a Conventional Commit title, such as docs: explain setup.")


def git(*args):
    return subprocess.check_output(["git", *args], text=True)


def main():
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
    pr = event["pull_request"]
    check_title(pr["title"])
    issues = issue_references(pr.get("body") or "")
    base, head = pr["base"]["sha"], pr["head"]["sha"]
    for sha in (base, head):
        if not re.fullmatch(r"[0-9a-f]{40}", sha):
            raise ValueError("Expected a full Git commit SHA.")
    changes = git("diff", "--name-only", f"{base}...{head}").splitlines()
    if "CHANGELOG.md" not in changes:
        raise ValueError("Update CHANGELOG.md with the actual change and issue link.")
    for message in git("log", "--no-merges", "--format=%B%x00", f"{base}..{head}").split("\0"):
        if message.strip():
            check_title(message.strip().splitlines()[0])
            issues.update(issue_references(message))
    repository = event["repository"]["full_name"]
    for number in sorted(issues):
        issue = json.loads(subprocess.check_output(
            ["gh", "api", f"repos/{repository}/issues/{number}"], text=True
        ))
        if "pull_request" in issue:
            raise ValueError(f"#{number} is a pull request; link a repository issue.")
    print("PR title, commit messages, issue references, and changelog change passed.")


if __name__ == "__main__":
    main()
