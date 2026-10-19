"""Approved bot-only release merging, with disabled-policy and exact-head boundaries."""

from copy import deepcopy
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from release_auto_merge import CHECKS, check_policy, queue
from release_pr import BOT, create_draft
from test_release_pr import HEAD, PLAN, Remote, environment
from verify_bundle_provenance import GH_VERSION


class AutoRemote(Remote):
    def __init__(self, plan):
        super().__init__(plan)
        self.allow = True
        self.required = CHECKS.copy()
        self.transitions = []

    def request(self, method, path, payload=None, missing=False):
        if method == "GET" and path == "":
            return {"allow_auto_merge": self.allow}
        if method == "GET" and path == "branches/main/protection":
            return {"required_status_checks": {"contexts": list(self.required), "strict": True},
                    "enforce_admins": {"enabled": True}}
        if method == "GET" and path == "pulls/321":
            return deepcopy(self.prs[0])
        return super().request(method, path, payload, missing)

    def graphql(self, query, variables):
        self.transitions.append("ready")
        self.prs[0]["draft"] = False
        return {"markPullRequestReadyForReview": {"pullRequest": {"id": variables["id"],
                                                                  "isDraft": False, "headRefOid": HEAD}}}


class AutoMergeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.policy(True)
        self.plan = deepcopy(PLAN)
        self.api = AutoRemote(self.plan)
        self.env = patch.dict(os.environ, environment() | {"RELEASE_AUTO_MERGE": "true"})
        self.env.start()
        self.addCleanup(self.env.stop)
        self.url = create_draft(self.plan, self.api)
        self.api.prs[0] |= {"node_id": "PR_fixture", "auto_merge": None}

    def policy(self, enabled):
        (self.root / "release-automation.toml").write_text(
            'schema_version = 1\nauto_merge_release_prs = ' + str(enabled).lower() + '\nmerge_method = "squash"\n')

    def run_queue(self, merged=False):
        def command(arguments, **kwargs):
            if arguments[-1] == "--version":
                return subprocess.CompletedProcess(arguments, 0, stdout=GH_VERSION + "\n")
            self.assertEqual(arguments[1:3], ["pr", "merge"])
            self.assertIn("--auto", arguments)
            self.assertIn("--squash", arguments)
            self.assertEqual(arguments[arguments.index("--match-head-commit") + 1], HEAD)
            self.assertNotIn("--admin", arguments)
            self.assertNotIn("--delete-branch", arguments)
            self.api.transitions.append("protected_merge")
            if merged:
                self.api.prs[0] |= {"state": "closed", "merged": True}
            else:
                self.api.prs[0]["auto_merge"] = {"merge_method": "squash", "enabled_by": {"login": BOT}}
            return subprocess.CompletedProcess(arguments, 0, stdout="")
        with patch("release_auto_merge.subprocess.run", side_effect=command):
            return queue(self.root, self.plan, self.url, self.api, Path("/usr/bin/true"))

    def test_approved_policy_queues_only_identical_bot_pr_and_reuses_queue(self):
        self.assertEqual(self.run_queue(), "queued_subject_to_required_checks")
        self.assertEqual(self.api.transitions, ["ready", "protected_merge"])
        self.assertEqual(self.run_queue(), "already_queued")
        self.assertEqual(self.api.transitions, ["ready", "protected_merge"])

    def test_already_passing_checks_use_normal_protected_merge_without_admin(self):
        self.assertEqual(self.run_queue(merged=True), "merged_after_required_checks")

    def test_disabled_policy_missing_optin_or_repository_setting_stop_before_transitions(self):
        self.policy(False)
        with self.assertRaisesRegex(ValueError, "approved checked-in"):
            self.run_queue()
        self.policy(True)
        with patch.dict(os.environ, {"RELEASE_AUTO_MERGE": "false"}):
            with self.assertRaisesRegex(ValueError, "approved checked-in"):
                self.run_queue()
        self.api.allow = False
        with self.assertRaisesRegex(ValueError, "settings automatically"):
            self.run_queue()
        self.assertEqual(self.api.transitions, [])

    def test_missing_required_protection_human_or_modified_pr_cannot_enter_queue(self):
        self.api.required = {"Rust and repository checks"}
        with self.assertRaisesRegex(ValueError, "both required checks"):
            self.run_queue()
        self.api.required = CHECKS.copy()
        self.api.prs[0]["user"]["login"] = "human-fixture"
        with self.assertRaises(ValueError):
            self.run_queue()
        self.api.prs[0]["user"]["login"] = BOT
        self.api.prs[0]["head"]["sha"] = "f" * 40
        with self.assertRaises((ValueError, KeyError)):
            self.run_queue()
        self.assertEqual(self.api.transitions, [])

    def test_lost_ready_response_can_recover_identical_ready_pr_under_policy(self):
        self.api.prs[0]["draft"] = False
        with self.assertRaisesRegex(ValueError, "Existing PR"):
            create_draft(self.plan, self.api)
        self.assertEqual(create_draft(self.plan, self.api, allow_ready=True), self.url)
        self.assertEqual(self.run_queue(), "queued_subject_to_required_checks")
        self.assertEqual(self.api.transitions, ["protected_merge"])

    def test_changed_existing_auto_merge_request_is_not_adopted(self):
        self.api.prs[0]["auto_merge"] = {"merge_method": "merge", "enabled_by": {"login": "human-fixture"}}
        with self.assertRaisesRegex(ValueError, "another actor"):
            self.run_queue()
        self.assertEqual(self.api.transitions, [])


if __name__ == "__main__":
    unittest.main()
