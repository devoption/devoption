"""Queue only an identical bot release PR under separately approved protected-main policy."""

import os
import re
import subprocess
import tomllib

from release_pr import BOT, REPOSITORY, require_workflow, verify_commit, verify_pr
from verify_bundle_provenance import GH_VERSION, snapshot

CHECKS = {"Rust and repository checks", "Issue and change documentation"}


def check_policy(root, api):
    policy = tomllib.loads(snapshot(root / "release-automation.toml", 8192).decode())
    if (set(policy) != {"schema_version", "auto_merge_release_prs", "merge_method"} or
            type(policy["schema_version"]) is not int or policy["schema_version"] != 1 or
            policy["auto_merge_release_prs"] is not True or policy["merge_method"] != "squash" or
            os.environ.get("RELEASE_AUTO_MERGE") != "true"):
        raise ValueError("Release PR auto-merge requires approved checked-in policy and explicit run opt-in.")
    if api.request("GET", "").get("allow_auto_merge") is not True:
        raise ValueError("Repository auto-merge is disabled; do not change settings automatically.")
    protection = api.request("GET", "branches/main/protection")
    status = protection.get("required_status_checks") or {}
    contexts = set(status.get("contexts", [])) | {check["context"] for check in status.get("checks", [])}
    if (not CHECKS <= contexts or status.get("strict") is not True or
            protection.get("enforce_admins", {}).get("enabled") is not True):
        raise ValueError("Protected main must enforce both required checks and up-to-date review, including administrators.")


def queue(root, plan, url, api, gh):
    require_workflow(plan["source"])
    check_policy(root, api)
    match = re.fullmatch(r"https://github\.com/devoption/devoption/pull/([1-9][0-9]*)", url)
    if not match:
        raise ValueError("Only the identical generated release PR can enter auto-merge.")
    gh = str(gh.resolve(strict=True))
    version = subprocess.run([gh, "--version"], check=True, capture_output=True, text=True, timeout=10).stdout.splitlines()
    if not version or version[0] != GH_VERSION:
        raise ValueError("Use the pinned GitHub CLI for guarded auto-merge.")
    path = "pulls/" + match[1]
    pr = api.request("GET", path)
    sha = pr["head"]["sha"]
    verify_commit(api, sha, plan)
    verify_pr(pr, sha, plan, allow_ready=True)
    if pr.get("auto_merge") is not None:
        if pr["auto_merge"]["merge_method"] != "squash" or pr["auto_merge"]["enabled_by"]["login"] != BOT:
            raise ValueError("Existing auto-merge was changed or requested by another actor; leave it untouched.")
        return "already_queued"
    if pr["draft"]:
        result = api.graphql("mutation($id:ID!){markPullRequestReadyForReview(input:{pullRequestId:$id})"
                             "{pullRequest{id isDraft headRefOid}}}", {"id": pr["node_id"]})
        ready = result["markPullRequestReadyForReview"]["pullRequest"]
        if ready["id"] != pr["node_id"] or ready["isDraft"] is not False or ready["headRefOid"] != sha:
            raise ValueError("Ready transition changed the reviewed PR identity.")
    # The pinned CLI queues pending checks, or performs the ordinary protected
    # merge when all requirements already pass. No administrator override is used.
    subprocess.run([gh, "pr", "merge", url, "--repo", "https://github.com/" + REPOSITORY,
                    "--squash", "--auto", "--match-head-commit", sha],
                   check=True, capture_output=True, text=True, timeout=60)
    observed = api.request("GET", path)
    verify_pr(observed | {"state": "open"}, sha, plan, allow_ready=True)
    if observed["state"] == "closed" and observed.get("merged") is True:
        return "merged_after_required_checks"
    if (observed["state"] != "open" or observed.get("auto_merge") is None or
            observed["auto_merge"]["merge_method"] != "squash" or observed["auto_merge"]["enabled_by"]["login"] != BOT):
        raise ValueError("Auto-merge was not confirmed for the identical bot-owned PR.")
    return "queued_subject_to_required_checks"
