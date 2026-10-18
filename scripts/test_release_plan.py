"""Disposable release-plz fixtures and fail-closed proposal boundary checks."""

from datetime import date
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch

from release_plan import check_config, git, prepare, workspace

ROOT = Path(__file__).resolve().parents[1]
CONFIG = (ROOT / "release-plz.toml").read_text()
NAMES = [p["name"] for p in tomllib.loads(CONFIG)["package"]]


class Fixture(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        members = [f"crates/{name}" for name in NAMES]
        (self.root / "Cargo.toml").write_text(
            '[workspace]\nresolver = "3"\nmembers = ' + str(members).replace("'", '"') +
            '\n[workspace.package]\nversion = "0.2.0"\npublish = false\n')
        for name, member in zip(NAMES, members):
            path = self.root / member
            (path / "src").mkdir(parents=True)
            (path / "src/lib.rs").write_text("pub fn fixture() {}\n")
            (path / "Cargo.toml").write_text(
                f'[package]\nname = "{name}"\nedition = "2024"\n'
                'version.workspace = true\npublish.workspace = true\n')
        (self.root / "CHANGELOG.md").write_text(
            '# Changelog\n\n## [Unreleased]\n\n## [0.2.0] - 2026-01-01\n\n'
            '- Initial fixture ([#260](https://github.com/devoption/devoption/issues/260)).\n')
        (self.root / "release-plz.toml").write_text(CONFIG)
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=self.root,
                       check=True, capture_output=True)
        git(self.root, "init", "--quiet")
        git(self.root, "config", "user.name", "Release fixture")
        git(self.root, "config", "user.email", "fixture@example.invalid")
        git(self.root, "config", "commit.gpgSign", "false")
        git(self.root, "config", "tag.gpgSign", "false")
        self.commit("feat: initial fixture")
        git(self.root, "tag", "v0.2.0")

    def commit(self, title):
        git(self.root, "add", ".")
        git(self.root, "commit", "--quiet", "-m", title, "-m", "Refs #260")

    def change(self, title, name="devoption-cli"):
        path = self.root / "crates" / name / "src/lib.rs"
        path.write_text(path.read_text() + "pub fn changed() {}\n")
        self.commit(title)


class BoundaryTests(Fixture):
    def test_rejects_dirty_source_before_running_tool(self):
        (self.root / "uncommitted").write_text("work in progress")
        with self.assertRaisesRegex(ValueError, "Commit changes"), patch("release_plan.run") as run:
            # Preserve Git calls while checking that no release tool can run.
            run.side_effect = lambda command, cwd, env=None: subprocess.check_output(command, cwd=cwd, text=True)
            prepare(self.root, Path("/absent/tool"), date(2026, 1, 2))
        self.assertTrue(all(call.args[0][0] == "git" for call in run.call_args_list))

    def test_rejects_wrong_tool_before_preparation(self):
        from release_plan import run as real_run
        def run(command, cwd, env=None):
            return "release-plz 0.0.1\n" if command[-1] == "--version" else real_run(command, cwd, env)
        with patch("release_plan.run", side_effect=run), self.assertRaisesRegex(ValueError, "exactly"):
            prepare(self.root, Path("/absent/tool"), date(2026, 1, 2))

    def test_requires_all_crates_in_one_nonpublishing_group(self):
        _, members = workspace(self.root)
        check_config(self.root, members)
        cases = [CONFIG.replace("git_only = true", "git_only = false"),
                 CONFIG.replace("git_tag_enable = false", "git_tag_enable = true"),
                 CONFIG.replace('version_group = "platform"', 'version_group = "other"', 1),
                 CONFIG.replace('name = "devoption-ui"', 'name = "missing"'),
                 CONFIG + '\npublish = true\n']
        for config in cases:
            with self.subTest(config=config):
                (self.root / "release-plz.toml").write_text(config)
                with self.assertRaises(ValueError):
                    check_config(self.root, members)

    def test_rejects_lockfile_version_drift(self):
        path = self.root / "Cargo.lock"
        path.write_text(path.read_text().replace('version = "0.2.0"', 'version = "0.2.1"', 1))
        with self.assertRaisesRegex(ValueError, "Cargo.lock"):
            workspace(self.root)


@unittest.skipUnless(os.environ.get("DEVOPTION_RELEASE_PLZ"), "set DEVOPTION_RELEASE_PLZ to the pinned binary")
class ToolTests(Fixture):
    def plan(self):
        before = git(self.root, "rev-parse", "HEAD")
        tags = git(self.root, "tag", "--list")
        report, proposal = prepare(self.root, Path(os.environ["DEVOPTION_RELEASE_PLZ"]), date(2026, 1, 2))
        self.assertEqual(git(self.root, "status", "--porcelain"), "")
        self.assertEqual(git(self.root, "rev-parse", "HEAD"), before)
        self.assertEqual(git(self.root, "tag", "--list"), tags)
        self.assertFalse(report["publication_authorized"])
        return report, proposal

    def test_fix_bumps_all_crates_and_retry_is_identical(self):
        self.change("fix: correct fixture", "devoption-ui")
        report, proposal = self.plan()
        self.assertEqual(report["proposed_version"], "0.2.1")
        self.assertIn("Cargo.lock", report["changed_files"])
        self.assertIn("correct fixture", proposal)
        self.assertEqual(self.plan(), (report, proposal))

    def test_docs_only_does_not_release(self):
        self.change("docs: explain fixture")
        report, proposal = self.plan()
        self.assertEqual(report["proposed_version"], "0.2.0")
        self.assertEqual(proposal, "")

    def test_feature_in_zero_major_bumps_minor(self):
        self.change("feat: extend fixture")
        report, _ = self.plan()
        self.assertEqual(report["proposed_version"], "0.3.0")

    def test_breaking_change_in_zero_major_bumps_minor(self):
        self.change("refactor!: replace fixture API")
        report, _ = self.plan()
        self.assertEqual(report["proposed_version"], "0.3.0")

    def test_breaking_footer_triggers_release(self):
        self.change("refactor: replace fixture API\n\nBREAKING CHANGE: remove old API")
        report, _ = self.plan()
        self.assertEqual(report["proposed_version"], "0.3.0")

    def test_existing_preview_requires_explicit_version_selection(self):
        for path in (self.root / "Cargo.toml", self.root / "Cargo.lock", self.root / "CHANGELOG.md"):
            path.write_text(path.read_text().replace("0.2.0", "0.3.0-preview.1"))
        self.commit("chore: prepare preview fixture")
        git(self.root, "tag", "v0.3.0-preview.1")
        self.change("fix: correct preview")
        report, _ = self.plan()
        self.assertEqual(report["proposed_version"], "0.3.0-preview.1")
        self.assertTrue(report["limitations"])

    def test_initial_development_is_not_automatically_promoted(self):
        git(self.root, "tag", "--delete", "v0.2.0")
        for path in (self.root / "Cargo.toml", self.root / "Cargo.lock", self.root / "CHANGELOG.md"):
            path.write_text(path.read_text().replace("0.2.0", "0.1.0-dev"))
        self.commit("chore: development fixture")
        self.change("feat: initial development feature")
        report, _ = self.plan()
        self.assertEqual(report["proposed_version"], "0.1.0-dev")

    def test_tool_failure_returns_no_successful_proposal(self):
        from release_plan import run as real_run
        def run(command, cwd, env=None):
            if "update" in command:
                raise subprocess.CalledProcessError(1, command, stderr="fixture failure")
            return real_run(command, cwd, env)
        with patch("release_plan.run", side_effect=run), self.assertRaises(subprocess.CalledProcessError):
            self.plan()
