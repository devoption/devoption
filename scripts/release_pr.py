"""Create an immutable bot-owned draft from a freshly regenerated, validated preview."""

import argparse
from datetime import date
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
from urllib.error import HTTPError
from urllib.parse import urlencode
from urllib.request import HTTPRedirectHandler, Request, build_opener

from release_plan import git, prepare, run
from release_preview import version_parts
from release_metadata import FILENAME

REPOSITORY = "devoption/devoption"
BOT = "github-actions[bot]"
PREFIX = "codex/release-plz-"
FILES = {"Cargo.toml", "Cargo.lock", "CHANGELOG.md"}


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class GitHub:
    def __init__(self):
        token = os.environ.get("GH_TOKEN")
        if not token:
            raise ValueError("The draft job requires its short-lived GH_TOKEN.")
        self.headers = {"Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json",
                        "X-GitHub-Api-Version": "2026-03-10", "Content-Type": "application/json",
                        "User-Agent": "DevOption-release-preparation"}
        self.opener = build_opener(NoRedirect)

    def request(self, method, path, payload=None, missing=False):
        request = Request(f"https://api.github.com/repos/{REPOSITORY}" + ("/" + path if path else ""),
                          data=json.dumps(payload).encode() if payload is not None else None,
                          headers=self.headers, method=method)
        try:
            with self.opener.open(request, timeout=30) as response:
                raw = response.read(2097153)
            if len(raw) > 2097152:
                raise ValueError("GitHub response exceeded the draft writer limit.")
            return json.loads(raw)
        except HTTPError as error:
            if missing and error.code == 404:
                return None
            raise ValueError(f"GitHub {method} {path.split('?')[0]} failed ({error.code}); reconcile remote state before retrying.") from error

    def graphql(self, query, variables):
        request = Request("https://api.github.com/graphql", data=json.dumps({"query": query, "variables": variables}).encode(),
                          headers=self.headers, method="POST")
        try:
            with self.opener.open(request, timeout=30) as response:
                raw = response.read(2097153)
            if len(raw) > 2097152:
                raise ValueError("GraphQL response exceeds its bound.")
            result = json.loads(raw)
            if result.get("errors") or not isinstance(result.get("data"), dict):
                raise ValueError("GitHub rejected the guarded auto-merge transition; reconcile before retrying.")
            return result["data"]
        except HTTPError as error:
            raise ValueError(f"GitHub auto-merge transition failed ({error.code}); no policy bypass is permitted.") from error


def require_workflow(source):
    required = {"GITHUB_ACTIONS": "true", "GITHUB_EVENT_NAME": "workflow_dispatch",
                "GITHUB_REF": "refs/heads/main", "GITHUB_REPOSITORY": REPOSITORY,
                "GITHUB_SERVER_URL": "https://github.com", "GITHUB_SHA": source,
                "RELEASE_CREATE_DRAFT": "true", "RELEASE_CHECKS_RESULT": "success",
                "RELEASE_PLAN_RESULT": "success"}
    if not re.fullmatch(r"[0-9a-f]{40}", source) or any(os.environ.get(k) != v for k, v in required.items()):
        raise ValueError("Draft creation requires an opted-in main workflow with successful exact-revision checks and plan.")


def candidate(repository, executable, release_date):
    report, patch = prepare(repository, executable, release_date)
    if not report["changed_files"]:
        return None
    changed = set(report["changed_files"])
    if not FILES <= changed or changed - FILES - {FILENAME} or report["release_metadata"] is None:
        raise ValueError("Drafts contain the root version, lockfile, changelog and optional version-only metadata binding.")
    source = report["source_commit"]
    with tempfile.TemporaryDirectory(prefix="devoption-release-pr-") as directory:
        scratch = Path(directory) / "source"
        run(["git", "clone", "--quiet", "--no-hardlinks", "--no-checkout", str(repository.resolve()), str(scratch)], repository)
        git(scratch, "checkout", "--quiet", "--detach", source)
        subprocess.run(["git", "apply", "--whitespace=error", "-"], cwd=scratch, input=patch,
                       text=True, check=True, capture_output=True, timeout=30)
        git(scratch, "diff", "--check")
        if set(git(scratch, "diff", "--name-only").splitlines()) != changed:
            raise ValueError("Regenerated patch does not match its reported scope.")
        changes = []
        for path in sorted(changed):
            if not git(scratch, "ls-files", "-s", "--", path).startswith("100644 "):
                raise ValueError("Draft files must retain ordinary nonexecutable file modes.")
            content = (scratch / path).read_text()
            if len(content.encode()) > 262144:
                raise ValueError("Draft file exceeds 256 KiB.")
            changes.append({"path": path, "mode": "100644", "type": "blob", "content": content})
        base_tree = git(scratch, "rev-parse", f"{source}^{{tree}}").strip()
        git(scratch, "add", "--", *sorted(changed))
        tree = git(scratch, "write-tree").strip()
    version = report["proposed_version"]
    title = f"chore: prepare platform v{version}"
    digest = report["release_metadata"]["sha256"]
    message = f"{title}\n\nRefs #260\n\nRelease-Source: {source}\nRelease-Metadata-SHA256: {digest}"
    body = (f"Prepare DevOption {version} from `{source}` for review.\n\nRefs #260\n\n"
            f"Metadata SHA-256: `{digest}`. Proposal date: {report['release_date']}.\n"
            f"Expected tree: `{tree}`.\n\n"
            "The source-revision checks and metadata proposal passed before creation. "
            "The required checks for this PR's new commit must pass separately. "
            "GitHub may require a maintainer to approve the bot-created PR's workflow runs.\n\n"
            "Review CHANGELOG.md for change notes, compatibility, migration and recovery limits. "
            "This draft does not authorize merging, tagging, publication or upgrades; #21 remains separate.")
    return {"source": source, "version": version, "branch": PREFIX + version,
            "base_tree": base_tree, "tree": tree, "changes": changes,
            "message": message, "title": title, "body": body}


def pulls(api, **query):
    result = []
    for page in range(1, 11):
        batch = api.request("GET", "pulls?" + urlencode(query | {"per_page": 100, "page": page}))
        if not isinstance(batch, list):
            raise ValueError("GitHub returned an invalid pull request list.")
        result.extend(batch)
        if len(batch) < 100:
            return result
    raise ValueError("Too many PRs to reconcile safely; inspect the remote candidates manually.")


def current(api, plan):
    if api.request("GET", "git/ref/heads/main")["object"]["sha"] != plan["source"]:
        raise ValueError("Main advanced; regenerate and review a fresh proposal instead of publishing stale changes.")
    if api.request("GET", "git/ref/tags/v" + plan["version"], missing=True) is not None:
        raise ValueError("The proposed version already has a remote tag; reconcile publication first.")


def verify_commit(api, sha, plan):
    commit = api.request("GET", "commits/" + sha)
    if ([p["sha"] for p in commit["parents"]] != [plan["source"]] or
            commit["commit"]["tree"]["sha"] != plan["tree"] or
            commit["commit"]["message"] != plan["message"] or
            (commit.get("author") or {}).get("login") != BOT):
        raise ValueError("Existing candidate commit is not the identical bot-owned proposal; leave it untouched and reconcile manually.")


def verify_pr(pr, sha, plan, allow_ready=False):
    if (pr["state"] != "open" or (pr["draft"] is not True and not (allow_ready and pr["draft"] is False)) or pr["user"]["login"] != BOT or
            pr["head"]["ref"] != plan["branch"] or pr["head"]["sha"] != sha or
            pr["head"]["repo"]["full_name"] != REPOSITORY or pr["base"]["ref"] != "main" or
            pr["base"]["repo"]["full_name"] != REPOSITORY or
            pr["title"] != plan["title"] or pr["body"] != plan["body"]):
        raise ValueError("Existing PR is closed, ready, modified or not the identical bot-owned draft; reconcile manually.")
    return pr["html_url"]


def create_draft(plan, api, allow_ready=False):
    require_workflow(plan["source"])
    if allow_ready and os.environ.get("RELEASE_AUTO_MERGE") != "true":
        raise ValueError("Recovering a ready release PR requires the separately opted-in auto-merge path.")
    paths = {entry["path"] for entry in plan["changes"]}
    if (version_parts(plan["version"])[2] is None or plan["branch"] != PREFIX + plan["version"] or
            len(plan["changes"]) != len(paths) or not FILES <= paths or paths - FILES - {FILENAME} or
            any(entry["mode"] != "100644" or entry["type"] != "blob" for entry in plan["changes"]) or
            any(not re.fullmatch(r"[0-9a-f]{40}", plan[key]) for key in ("base_tree", "tree"))):
        raise ValueError("Draft writer scope must remain one preview branch and the narrow release/metadata files.")
    current(api, plan)
    issue = api.request("GET", "issues/260")
    if "pull_request" in issue:
        raise ValueError("The contribution reference must remain a repository issue.")
    for pr in pulls(api, state="open"):
        if (pr["head"]["ref"].startswith(PREFIX) and pr["head"]["repo"]["full_name"] == REPOSITORY and
                pr["head"]["ref"] != plan["branch"]):
            raise ValueError("Another release candidate is open; reconcile it before preparing a different version.")
    existing = pulls(api, state="all", head="devoption:" + plan["branch"], base="main")
    if len(existing) > 1:
        raise ValueError("Multiple PRs already reference this candidate; reconcile manually.")
    reference = "git/ref/heads/" + plan["branch"]
    branch = api.request("GET", reference, missing=True)
    if existing and not branch:
        raise ValueError("Candidate PR exists without its branch; reconcile manually.")
    if branch:
        sha = branch["object"]["sha"]
        verify_commit(api, sha, plan)
    else:
        base = api.request("GET", "git/commits/" + plan["source"])
        if base["tree"]["sha"] != plan["base_tree"]:
            raise ValueError("Remote source tree does not match the reviewed checkout.")
        tree = api.request("POST", "git/trees", {"base_tree": plan["base_tree"], "tree": plan["changes"]})
        if tree["sha"] != plan["tree"]:
            raise ValueError("GitHub constructed an unexpected tree; no branch was created.")
        commit = api.request("POST", "git/commits", {"message": plan["message"], "tree": tree["sha"], "parents": [plan["source"]]})
        sha = commit["sha"]
        verify_commit(api, sha, plan)
        current(api, plan)
        api.request("POST", "git/refs", {"ref": "refs/heads/" + plan["branch"], "sha": sha})
    current(api, plan)
    if existing:
        return verify_pr(existing[0], sha, plan, allow_ready=allow_ready)
    # No update/force/delete/merge endpoint is used. A lost response is reconciled on retry.
    pr = api.request("POST", "pulls", {"head": plan["branch"], "base": "main", "title": plan["title"],
                                       "body": plan["body"], "draft": True, "maintainer_can_modify": False})
    return verify_pr(pr, sha, plan)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-plz", type=Path, required=True)
    parser.add_argument("--date", type=date.fromisoformat, required=True)
    parser.add_argument("--auto-merge", action="store_true", help="Requires separate checked-in and repository policy approval")
    parser.add_argument("--gh", type=Path, help="Checksum-pinned CLI, required only for approved auto-merge")
    args = parser.parse_args()
    root = Path.cwd()
    require_workflow(git(root, "rev-parse", "HEAD").strip())
    plan = candidate(root, args.release_plz, args.date)
    if plan is None:
        print("No shipped changes; no draft release PR created.")
    else:
        api = GitHub()
        if args.auto_merge:
            from release_auto_merge import check_policy, queue
            if args.gh is None:
                raise ValueError("Approved auto-merge requires the pinned --gh path.")
            check_policy(root, api)  # Refuse disabled policy before creating a branch/PR.
        url = create_draft(plan, api, allow_ready=args.auto_merge)
        print(f"Release PR (publication remains gated): {url}")
        if args.auto_merge:
            print(f"Auto-merge: {queue(root, plan, url, api, args.gh)}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError) as error:
        raise SystemExit(f"Draft creation stopped: {error}") from error
