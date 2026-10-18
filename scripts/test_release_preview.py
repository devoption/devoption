"""Preview numbering, repository-wide change coverage and retained release-note fixtures."""

from datetime import date
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

from release_plan import git, prepare
from release_preview import preview_changelog, select_preview, version_parts
from test_release_plan import Fixture

NOTES = "- Correct fixture behavior ([#260](https://github.com/devoption/devoption/issues/260))."


class PreviewFixture(Fixture):
    def select(self):
        from release_plan import workspace
        current, _ = workspace(self.root)
        return select_preview(self.root, current, git(self.root, "rev-parse", "HEAD").strip(), git)

    def notes(self):
        path = self.root / "CHANGELOG.md"
        path.write_text(path.read_text().replace("## [Unreleased]", f"## [Unreleased]\n\n{NOTES}"))

    def baseline(self, version, tagged=True):
        for path in (self.root / "Cargo.toml", self.root / "Cargo.lock", self.root / "CHANGELOG.md"):
            path.write_text(path.read_text().replace("0.2.0", version))
        self.commit("chore: prepare fixture version")
        if tagged:
            git(self.root, "tag", f"v{version}")


class PreviewTests(PreviewFixture):
    def test_stable_fix_selects_preview_on_next_patch(self):
        self.change("fix: correct fixture")
        selection = self.select()
        self.assertEqual(selection["next_version"], "0.2.1-preview.1")
        self.assertEqual(selection["baseline_tag"], "v0.2.0")
        self.assertEqual(selection["change_kind"], "patch")

    def test_feature_and_breaking_changes_select_next_minor_for_zero_major(self):
        self.change("feat: extend fixture")
        self.assertEqual(self.select()["next_version"], "0.3.0-preview.1")
        self.change("refactor!: replace API", "devoption-ui")
        self.assertEqual(self.select()["change_kind"], "breaking")
        self.assertEqual(self.select()["next_version"], "0.3.0-preview.1")

    def test_breaking_footer_selects_major_after_one(self):
        self.baseline("1.2.3")
        self.change("refactor: replace API\n\nBREAKING CHANGE: remove old API")
        self.assertEqual(self.select()["next_version"], "2.0.0-preview.1")

    def test_preview_keeps_base_and_advances_sequence_even_for_breaking_change(self):
        self.baseline("0.3.0-preview.4")
        self.change("feat!: replace preview API")
        self.assertEqual(self.select()["next_version"], "0.3.0-preview.5")
        self.assertEqual(self.select()["change_kind"], "breaking")

    def test_development_bootstrap_selects_first_preview_without_tag_creation(self):
        git(self.root, "tag", "--delete", "v0.2.0")
        self.baseline("0.1.0-dev", tagged=False)
        self.assertEqual(self.select()["next_version"], "0.1.0-preview.1")
        self.assertEqual(git(self.root, "tag", "--list"), "")

    def test_documentation_and_standalone_tests_do_not_release(self):
        for name in ["README.md", "docs/guide.md", "scripts/test_helper.py", "crates/devoption-cli/tests/extra.rs"]:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("fixture\n")
        self.notes()
        self.commit("docs: explain behavior")
        selection = self.select()
        self.assertEqual(selection["shipped_changes"], [])
        self.assertEqual(selection["next_version"], "0.2.0")

    def test_helpers_templates_unknown_paths_and_deleted_files_count(self):
        paths = ["scripts/deployment_broker.py", "examples/systemd/devoption.service", "new-assets/unknown"]
        for name in paths:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("fixture\n")
        self.commit("fix: update installation helpers")
        self.assertEqual(self.select()["shipped_changes"], sorted(paths))
        path = self.root / "crates/devoption-cli/src/lib.rs"
        path.unlink()
        self.commit("chore: remove unused source")
        self.assertIn("crates/devoption-cli/src/lib.rs", self.select()["shipped_changes"])

    def test_source_changes_under_docs_title_are_not_silently_skipped(self):
        self.change("docs: accidentally include source changes")
        self.assertEqual(self.select()["next_version"], "0.2.1-preview.1")

    def test_net_reverted_changes_do_not_release(self):
        self.change("feat: change fixture")
        git(self.root, "revert", "--no-commit", "HEAD")
        self.commit("revert: undo fixture change")
        self.assertEqual(self.select()["shipped_changes"], [])

    def test_untagged_preview_blocks_repeated_preparation(self):
        self.baseline("0.3.0-preview.1", tagged=False)
        self.change("fix: change fixture")
        with self.assertRaisesRegex(ValueError, "no baseline tag"):
            self.select()

    def test_newer_or_colliding_platform_tags_fail_closed(self):
        self.change("fix: change fixture")
        git(self.root, "tag", "v0.2.1-preview.1")
        with self.assertRaisesRegex(ValueError, "behind a known platform tag"):
            self.select()

    def test_baseline_tag_must_match_embedded_version(self):
        git(self.root, "tag", "--delete", "v0.2.0")
        self.baseline("0.3.0-preview.1", tagged=False)
        git(self.root, "tag", "v0.3.0-preview.1", "HEAD~1")
        with self.assertRaisesRegex(ValueError, "disagree"):
            self.select()

    def test_baseline_on_another_history_is_rejected(self):
        original = git(self.root, "rev-parse", "HEAD").strip()
        git(self.root, "tag", "--delete", "v0.2.0")
        git(self.root, "checkout", "--quiet", "--orphan", "other-history")
        self.commit("feat: independent history")
        git(self.root, "tag", "v0.2.0")
        git(self.root, "checkout", "--quiet", "--detach", original)
        with self.assertRaisesRegex(ValueError, "not an ancestor"):
            self.select()

    def test_shallow_history_is_rejected(self):
        self.change("fix: change fixture")
        shallow = self.root.parent / (self.root.name + "-shallow")
        import shutil
        self.addCleanup(shutil.rmtree, shallow, True)
        git(self.root, "clone", "--quiet", "--depth", "1", self.root.as_uri(), str(shallow))
        with self.assertRaisesRegex(ValueError, "complete history"):
            select_preview(shallow, "0.2.0", git(shallow, "rev-parse", "HEAD").strip(), git)

    def test_notes_preserve_human_text_and_history_exactly(self):
        self.notes()
        original = (self.root / "CHANGELOG.md").read_text()
        history = original[original.index("## [0.2.0]"):]
        notes = preview_changelog(original, "0.2.1-preview.1", date(2026, 1, 2))
        self.assertEqual(notes.count(NOTES), 1)
        self.assertTrue(notes.endswith(history))
        self.assertIn("## [Unreleased]\n\n## [0.2.1-preview.1] - 2026-01-02", notes)

    def test_initial_notes_do_not_add_blank_lines_at_eof(self):
        changelog = f"# Changelog\n\n## [Unreleased]\n\n{NOTES}\n"
        notes = preview_changelog(changelog, "0.1.0-preview.1", date(2026, 1, 2))
        self.assertTrue(notes.endswith(NOTES + "\n"))
        self.assertFalse(notes.endswith("\n\n"))

    def test_incomplete_ambiguous_or_future_notes_are_rejected(self):
        original = (self.root / "CHANGELOG.md").read_text()
        cases = [(original, date(2026, 1, 2)),
                 (original.replace("## [Unreleased]", "## [Unreleased]\n\n- Missing issue."), date(2026, 1, 2)),
                 (original + "\n## [Unreleased]\n", date(2026, 1, 2)),
                 (original.replace("## [Unreleased]", f"## [Unreleased]\n\n{NOTES}"), date(9999, 1, 1))]
        for changelog, day in cases:
            with self.subTest(changelog=changelog, day=day), self.assertRaises(ValueError):
                preview_changelog(changelog, "0.2.1-preview.1", day)

    def test_unknown_versions_are_not_promoted(self):
        for version in ["0.2.0-alpha.1", "0.2.0-preview.0", "0.2.0-preview.01", "v0.2.0", "01.2.0", "0.2.0+build"]:
            with self.subTest(version=version), self.assertRaises(ValueError):
                version_parts(version)


@unittest.skipUnless(os.environ.get("DEVOPTION_RELEASE_PLZ"), "set DEVOPTION_RELEASE_PLZ to the pinned binary")
class PreviewToolTests(PreviewFixture):
    def plan(self):
        before = git(self.root, "rev-parse", "HEAD")
        tags = git(self.root, "tag", "--list")
        report, proposal = prepare(self.root, Path(os.environ["DEVOPTION_RELEASE_PLZ"]), date(2026, 1, 2))
        self.assertEqual(git(self.root, "status", "--porcelain"), "")
        self.assertEqual(git(self.root, "rev-parse", "HEAD"), before)
        self.assertEqual(git(self.root, "tag", "--list"), tags)
        self.assertFalse(report["publication_authorized"])
        return report, proposal

    def test_real_tool_updates_workspace_and_lockfile_with_retained_notes(self):
        self.notes()
        self.change("fix: correct fixture")
        report, proposal = self.plan()
        self.assertEqual(report["proposed_version"], "0.2.1-preview.1")
        self.assertEqual(report["changed_files"], ["CHANGELOG.md", "Cargo.lock", "Cargo.toml"])
        self.assertIn(NOTES, proposal)
        self.assertEqual(proposal.count('+version = "0.2.1-preview.1"'), 8)
        self.assertEqual(self.plan(), (report, proposal))

    def test_helper_only_change_prepares_preview(self):
        self.notes()
        path = self.root / "scripts/helper.py"
        path.parent.mkdir()
        path.write_text("# Shipped helper fixture\n")
        self.commit("fix: correct shipped helper")
        report, _ = self.plan()
        self.assertEqual(report["proposed_version"], "0.2.1-preview.1")
        self.assertEqual(report["preview_selection"]["shipped_changes"], ["scripts/helper.py"])

    def test_no_release_produces_no_patch_and_keeps_notes(self):
        self.notes()
        self.commit("docs: document fixture")
        report, proposal = self.plan()
        self.assertEqual(report["proposed_version"], "0.2.0")
        self.assertEqual(proposal, "")

    def test_real_tool_bootstraps_development_preview(self):
        git(self.root, "tag", "--delete", "v0.2.0")
        self.baseline("0.1.0-dev", tagged=False)
        self.notes()
        self.commit("feat: complete development fixture")
        report, _ = self.plan()
        self.assertEqual(report["proposed_version"], "0.1.0-preview.1")

    def test_real_tool_advances_existing_preview(self):
        self.baseline("0.3.0-preview.1")
        self.notes()
        self.change("fix: correct preview fixture")
        report, _ = self.plan()
        self.assertEqual(report["proposed_version"], "0.3.0-preview.2")

    def test_missing_human_notes_blocks_proposal(self):
        self.change("fix: correct fixture")
        with self.assertRaisesRegex(ValueError, "change bullet"):
            self.plan()
        self.assertEqual(git(self.root, "status", "--porcelain"), "")

    def test_dependency_mutation_fails_without_changing_original(self):
        from release_plan import run as real_run
        self.notes()
        self.change("fix: correct fixture")
        def run(command, cwd, env=None):
            result = real_run(command, cwd, env)
            if "set-version" in command:
                manifest = cwd / "Cargo.toml"
                manifest.write_text(manifest.read_text() + '\nlicense = "MIT"\n')
            return result
        with patch("release_plan.run", side_effect=run), self.assertRaisesRegex(ValueError, "beyond the platform version"):
            self.plan()
        self.assertEqual(git(self.root, "status", "--porcelain"), "")

    def test_tool_failure_is_not_a_successful_proposal(self):
        from release_plan import run as real_run
        self.notes()
        self.change("fix: correct fixture")
        def run(command, cwd, env=None):
            if "set-version" in command:
                raise subprocess.CalledProcessError(1, command, stderr="fixture failure")
            return real_run(command, cwd, env)
        with patch("release_plan.run", side_effect=run), self.assertRaises(subprocess.CalledProcessError):
            self.plan()
        self.assertEqual(git(self.root, "status", "--porcelain"), "")
