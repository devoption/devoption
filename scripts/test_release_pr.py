"""Draft writer state transitions against a disposable GitHub API fixture."""

from copy import deepcopy
from datetime import date
import io
import os
from pathlib import Path
import unittest
from unittest.mock import patch
from urllib.error import HTTPError
from urllib.parse import parse_qs, urlsplit

from release_pr import BOT, PREFIX, REPOSITORY, GitHub, candidate, create_draft
from test_release_preview import PreviewFixture

SOURCE, TREE, BASE, HEAD = "1" * 40, "2" * 40, "3" * 40, "4" * 40
PLAN = {"source": SOURCE, "tree": TREE, "base_tree": BASE, "version": "0.1.0-preview.1",
        "branch": PREFIX + "0.1.0-preview.1", "title": "chore: prepare platform v0.1.0-preview.1",
        "message": "chore: prepare platform v0.1.0-preview.1\n\nRefs #260", "body": "Refs #260\nReview #21.",
        "changes": [{"path": path, "mode": "100644", "type": "blob", "content": "fixture\n"}
                    for path in ["CHANGELOG.md", "Cargo.lock", "Cargo.toml"]]}


def environment(source=SOURCE):
    return {"GITHUB_ACTIONS": "true", "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REF": "refs/heads/main",
            "GITHUB_REPOSITORY": REPOSITORY, "GITHUB_SERVER_URL": "https://github.com", "GITHUB_SHA": source,
            "RELEASE_CREATE_DRAFT": "true", "RELEASE_CHECKS_RESULT": "success", "RELEASE_PLAN_RESULT": "success"}


class Remote:
    def __init__(self, plan):
        self.plan = plan
        self.main = plan["source"]
        self.tag = None
        self.branch = None
        self.prs = []
        self.commits = {}
        self.calls = []
        self.lose_response = None
        self.tree = plan["tree"]
        self.author = BOT

    def request(self, method, path, payload=None, missing=False):
        self.calls.append((method, path, deepcopy(payload)))
        parsed = urlsplit(path)
        query = parse_qs(parsed.query)
        if method == "GET":
            if path == "git/ref/heads/main":
                return {"object": {"sha": self.main}}
            if path.startswith("git/ref/tags/"):
                return self.tag
            if path == "issues/260":
                return {"number": 260}
            if parsed.path == "pulls":
                prs = self.prs
                if query.get("state") == ["open"]:
                    prs = [pr for pr in prs if pr["state"] == "open"]
                if "head" in query:
                    prs = [pr for pr in prs if pr["head"]["ref"] == query["head"][0].split(":", 1)[1]]
                return deepcopy(prs)
            if path.startswith("git/ref/heads/"):
                return {"object": {"sha": self.branch}} if self.branch else None
            if path.startswith("git/commits/"):
                return {"tree": {"sha": self.plan["base_tree"]}}
            if path.startswith("commits/"):
                return deepcopy(self.commits[path.split("/")[1]])
        elif method == "POST":
            if path == "git/trees":
                return {"sha": self.tree}
            if path == "git/commits":
                self.commits[HEAD] = {"parents": [{"sha": sha} for sha in payload["parents"]],
                                      "commit": {"tree": {"sha": payload["tree"]}, "message": payload["message"]},
                                      "author": {"login": self.author}}
                return {"sha": HEAD}
            if path == "git/refs":
                if self.branch:
                    raise ValueError("Reference already exists")
                self.branch = payload["sha"]
                result = {"object": {"sha": self.branch}}
            elif path == "pulls":
                result = {"number": 321, "html_url": "https://github.com/devoption/devoption/pull/321",
                          "state": "open", "draft": payload["draft"], "user": {"login": BOT},
                          "title": payload["title"], "body": payload["body"],
                          "head": {"ref": payload["head"], "sha": self.branch, "repo": {"full_name": REPOSITORY}},
                          "base": {"ref": payload["base"], "repo": {"full_name": REPOSITORY}}}
                self.prs.append(result)
            else:
                raise AssertionError((method, path, payload))
            if self.lose_response == path:
                self.lose_response = None
                raise ValueError("Fixture lost response after remote success")
            return deepcopy(result)
        else:
            raise AssertionError("No update/delete/merge operation is allowed")
        raise AssertionError((method, path, payload))

    def mutations(self):
        return [call for call in self.calls if call[0] != "GET"]


class DraftTests(unittest.TestCase):
    def setUp(self):
        self.env = patch.dict(os.environ, environment())
        self.env.start()
        self.addCleanup(self.env.stop)
        self.plan = deepcopy(PLAN)
        self.api = Remote(self.plan)

    def test_creates_only_expected_tree_commit_branch_and_draft(self):
        url = create_draft(self.plan, self.api)
        self.assertTrue(url.endswith("/321"))
        self.assertEqual([call[1] for call in self.api.mutations()], ["git/trees", "git/commits", "git/refs", "pulls"])
        self.assertEqual(self.api.mutations()[0][2], {"base_tree": BASE, "tree": self.plan["changes"]})
        self.assertEqual(self.api.mutations()[1][2]["parents"], [SOURCE])
        self.assertEqual(self.api.mutations()[2][2]["ref"], "refs/heads/" + self.plan["branch"])
        self.assertTrue(self.api.mutations()[3][2]["draft"])
        self.assertFalse(self.api.mutations()[3][2]["maintainer_can_modify"])
        self.assertEqual(self.api.main, SOURCE)

    def test_identical_retry_has_no_mutations(self):
        first = create_draft(self.plan, self.api)
        self.api.calls.clear()
        self.assertEqual(create_draft(self.plan, self.api), first)
        self.assertEqual(self.api.mutations(), [])

    def test_lost_branch_or_pr_response_is_reconciled_without_duplicate(self):
        for operation in ["git/refs", "pulls"]:
            with self.subTest(operation=operation):
                api = Remote(self.plan)
                api.lose_response = operation
                with self.assertRaisesRegex(ValueError, "lost response"):
                    create_draft(self.plan, api)
                api.calls.clear()
                create_draft(self.plan, api)
                self.assertEqual(len(api.prs), 1)
                self.assertEqual([c[1] for c in api.mutations()], ["pulls"] if operation == "git/refs" else [])

    def test_untrusted_context_or_failed_checks_stops_before_api_calls(self):
        for key, value in [("GITHUB_REF", "refs/heads/topic"), ("GITHUB_EVENT_NAME", "pull_request"),
                           ("GITHUB_REPOSITORY", "other/repo"), ("GITHUB_SHA", "5" * 40),
                           ("RELEASE_CREATE_DRAFT", "false"), ("RELEASE_CHECKS_RESULT", "failure"),
                           ("RELEASE_CHECKS_RESULT", "cancelled"), ("RELEASE_PLAN_RESULT", "skipped")]:
            with self.subTest(key=key, value=value), patch.dict(os.environ, {key: value}), self.assertRaises(ValueError):
                create_draft(self.plan, self.api)
        self.assertEqual(self.api.calls, [])

    def test_stale_main_and_remote_tag_prevent_writes(self):
        self.api.main = "5" * 40
        with self.assertRaisesRegex(ValueError, "Main advanced"):
            create_draft(self.plan, self.api)
        self.assertEqual(self.api.mutations(), [])
        self.api.main = SOURCE
        self.api.tag = {"object": {"sha": SOURCE}}
        with self.assertRaisesRegex(ValueError, "remote tag"):
            create_draft(self.plan, self.api)
        self.assertEqual(self.api.mutations(), [])

    def test_writer_rejects_main_or_expanded_file_scope_before_api_calls(self):
        invalid = deepcopy(self.plan)
        invalid["branch"] = "main"
        with self.assertRaisesRegex(ValueError, "scope"):
            create_draft(invalid, self.api)
        invalid = deepcopy(self.plan)
        invalid["changes"][0]["path"] = ".github/workflows/ci.yml"
        with self.assertRaisesRegex(ValueError, "scope"):
            create_draft(invalid, self.api)
        self.assertEqual(self.api.calls, [])

    def test_main_advancing_after_commit_does_not_create_branch(self):
        request = self.api.request
        def advance(method, path, payload=None, missing=False):
            result = request(method, path, payload, missing)
            if path == "git/commits" and method == "POST":
                self.api.main = "5" * 40
            return result
        self.api.request = advance
        with self.assertRaisesRegex(ValueError, "Main advanced"):
            create_draft(self.plan, self.api)
        self.assertIsNone(self.api.branch)

    def test_closed_ready_human_or_edited_pr_is_left_untouched(self):
        for field, value in [("state", "closed"), ("draft", False), ("title", "Human edits"), ("body", "Edited notes"),
                             ("user", {"login": "maintainer"})]:
            with self.subTest(field=field):
                api = Remote(self.plan)
                create_draft(self.plan, api)
                api.prs[0][field] = value
                api.calls.clear()
                with self.assertRaises(ValueError):
                    create_draft(self.plan, api)
                self.assertEqual(api.mutations(), [])

    def test_different_commit_or_author_is_not_overwritten(self):
        create_draft(self.plan, self.api)
        for field, value in [("message", "different"), ("tree", {"sha": "9" * 40})]:
            original = deepcopy(self.api.commits[HEAD])
            self.api.commits[HEAD]["commit"][field] = value
            self.api.calls.clear()
            with self.assertRaises(ValueError):
                create_draft(self.plan, self.api)
            self.assertEqual(self.api.mutations(), [])
            self.api.commits[HEAD] = original
        self.api.commits[HEAD]["author"]["login"] = "maintainer"
        self.api.calls.clear()
        with self.assertRaises(ValueError):
            create_draft(self.plan, self.api)
        self.assertEqual(self.api.mutations(), [])

    def test_unexpected_tree_or_commit_author_never_creates_ref(self):
        for tree, author in [("9" * 40, BOT), (TREE, "maintainer")]:
            with self.subTest(tree=tree, author=author):
                api = Remote(self.plan)
                api.tree, api.author = tree, author
                with self.assertRaises(ValueError):
                    create_draft(self.plan, api)
                self.assertIsNone(api.branch)
                self.assertEqual(api.prs, [])

    def test_other_open_release_candidate_blocks_new_version(self):
        create_draft(self.plan, self.api)
        self.api.prs[0]["head"]["ref"] = PREFIX + "0.0.9-preview.1"
        self.api.calls.clear()
        with self.assertRaisesRegex(ValueError, "Another release candidate"):
            create_draft(self.plan, self.api)
        self.assertEqual(self.api.mutations(), [])


class TransportTests(unittest.TestCase):
    def test_http_failure_is_not_retried_or_allowed_to_print_token_or_response_body(self):
        with patch.dict(os.environ, {"GH_TOKEN": "fixture-not-a-real-token"}):
            api = GitHub()
        error = HTTPError("https://api.github.com/", 403, "forbidden", {}, io.BytesIO(b"private fixture response"))
        with patch.object(api.opener, "open", side_effect=error) as request:
            with self.assertRaises(ValueError) as failure:
                api.request("POST", "git/refs", {"ref": "refs/heads/fixture", "sha": SOURCE})
            self.assertEqual(request.call_count, 1)
        self.assertIn("403", str(failure.exception))
        self.assertNotIn("fixture-not-a-real-token", str(failure.exception))
        self.assertNotIn("private fixture response", str(failure.exception))

    def test_only_explicit_optional_not_found_is_treated_as_absent(self):
        with patch.dict(os.environ, {"GH_TOKEN": "fixture-not-a-real-token"}):
            api = GitHub()
        for code in [404, 403]:
            error = HTTPError("https://api.github.com/", code, "fixture", {}, None)
            with patch.object(api.opener, "open", side_effect=error):
                if code == 404:
                    self.assertIsNone(api.request("GET", "git/ref/heads/fixture", missing=True))
                else:
                    with self.assertRaises(ValueError):
                        api.request("GET", "git/ref/heads/fixture", missing=True)


@unittest.skipUnless(os.environ.get("DEVOPTION_RELEASE_PLZ"), "set DEVOPTION_RELEASE_PLZ to the pinned binary")
class CandidateTests(PreviewFixture):
    def test_real_proposal_builds_expected_tree_and_issue_linked_draft_payload(self):
        self.notes()
        self.change("fix: correct release fixture")
        source = self.select()["baseline_commit"]
        self.assertIsNotNone(source)
        plan = candidate(self.root, Path(os.environ["DEVOPTION_RELEASE_PLZ"]), date(2026, 1, 2))
        self.assertEqual({file["path"] for file in plan["changes"]}, {"CHANGELOG.md", "Cargo.lock", "Cargo.toml"})
        self.assertEqual(plan["version"], "0.2.1-preview.1")
        self.assertIn("Refs #260", plan["message"])
        self.assertIn("Release-Source: " + plan["source"], plan["message"])
        self.assertIn("required checks for this PR's new commit", plan["body"])
        with patch.dict(os.environ, environment(plan["source"])):
            self.assertTrue(create_draft(plan, Remote(plan)).endswith("/321"))
        self.assertEqual(candidate(self.root, Path(os.environ["DEVOPTION_RELEASE_PLZ"]), date(2026, 1, 2)), plan)

    def test_no_release_has_no_writer_payload(self):
        self.notes()
        self.commit("docs: explain release fixture")
        self.assertIsNone(candidate(self.root, Path(os.environ["DEVOPTION_RELEASE_PLZ"]), date(2026, 1, 2)))
