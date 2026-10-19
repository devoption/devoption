"""Version-bound release metadata, rendered-note integrity and rejection fixtures."""

from copy import deepcopy
from datetime import date
import hashlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import release_notes
from release_metadata import END, START, load_metadata, prepare_metadata, render_metadata, validate_metadata, validate_notes
from release_preview import preview_changelog
from test_release_plan import Fixture, METADATA

VERSION = "0.1.0-preview.1"
HUMAN = "- Retain change notes ([#260](https://github.com/devoption/devoption/issues/260))."


class MetadataTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.path = self.root / "release-metadata.toml"
        self.path.write_text(METADATA)
        self.data, self.digest = load_metadata(self.root, VERSION)

    def notes(self):
        changelog = f"# Changelog\n\n## [Unreleased]\n\n{HUMAN}\n"
        return preview_changelog(changelog, VERSION, date(2026, 1, 2), render_metadata(self.data, self.digest))

    def test_complete_metadata_is_bound_to_bytes_and_all_required_notes(self):
        self.assertEqual(self.digest, hashlib.sha256(self.path.read_bytes()).hexdigest())
        notes = self.notes()
        self.assertEqual(validate_notes(self.root, VERSION, notes), self.digest)
        for warning in self.data["support"]["warnings"]:
            self.assertIn(warning, notes)
        for section, field in [("compatibility", "data"), ("compatibility", "configuration"),
                               ("migration", "instructions"), ("recovery", "instructions")]:
            self.assertIn(self.data[section][field], notes)
        self.assertIn(HUMAN, notes)

    def test_preparation_automatically_binds_next_preview_without_editing_reviewed_statements(self):
        before = self.path.read_bytes()
        data, digest = prepare_metadata(self.root, VERSION, "0.1.0-preview.2")
        self.assertEqual(data, self.data | {"version": "0.1.0-preview.2"})
        self.assertEqual(self.path.read_bytes(), before.replace(VERSION.encode(), b"0.1.0-preview.2", 1))
        self.assertEqual(load_metadata(self.root, "0.1.0-preview.2"), (data, digest))

    def test_unrelated_metadata_and_unsupported_claims_cannot_be_carried_forward(self):
        before = self.path.read_bytes()
        with self.assertRaisesRegex(ValueError, "unrelated stale"):
            prepare_metadata(self.root, "0.1.0-preview.8", "0.1.0-preview.9")
        self.assertEqual(self.path.read_bytes(), before)
        self.path.write_text(METADATA.replace("required = false", "required = true"))
        bad = self.path.read_bytes()
        with self.assertRaisesRegex(ValueError, "No platform migration"):
            prepare_metadata(self.root, VERSION, "0.1.0-preview.2")
        self.assertEqual(self.path.read_bytes(), bad)

    def test_missing_or_misspelled_fields_and_wrong_types_are_rejected(self):
        paths = [(field,) for field in self.data]
        paths += [(section, field) for section in ("compatibility", "migration", "recovery", "support")
                  for field in self.data[section]]
        for keys in paths:
            with self.subTest(keys=keys):
                data = deepcopy(self.data)
                table = data if len(keys) == 1 else data[keys[0]]
                table.pop(keys[-1])
                with self.assertRaises(ValueError):
                    validate_metadata(data, VERSION)
        cases = [("schema_version", True), ("schema_version", 2), ("channel", "stable"),
                 ("platforms", ["ubuntu-24.04-arm64"]), ("support", "preview"),
                 ("extra", "unknown")]
        for key, value in cases:
            with self.subTest(key=key, value=value):
                data = deepcopy(self.data)
                data[key] = value
                with self.assertRaises(ValueError):
                    validate_metadata(data, VERSION)

    def test_stale_versions_and_stable_channels_are_not_candidates(self):
        for version in ["0.1.0-preview.2", "0.1.0", "0.1.0-dev", "0.1.0-preview.01"]:
            with self.subTest(version=version), self.assertRaises(ValueError):
                validate_metadata(self.data, version)

    def test_unsupported_upgrade_migration_and_rollback_claims_are_rejected(self):
        for section, field, value in [
                ("compatibility", "installation", "upgrade"),
                ("compatibility", "upgrade_from", ["0.0.1"]),
                ("migration", "required", True), ("migration", "required", 0),
                ("recovery", "binary_rollback", True), ("recovery", "binary_rollback", 0),
                ("support", "status", "production")]:
            with self.subTest(section=section, field=field, value=value):
                data = deepcopy(self.data)
                data[section][field] = value
                with self.assertRaises(ValueError):
                    validate_metadata(data, VERSION)

    def test_empty_malformed_or_ambiguous_warnings_are_rejected(self):
        for warnings in [[], "warning", [""], ["one", "one"], ["warning\nother"],
                         [True], ["a" * 2001], ["x"] * 11, [START]]:
            with self.subTest(warnings=warnings):
                data = deepcopy(self.data)
                data["support"]["warnings"] = warnings
                with self.assertRaises(ValueError):
                    validate_metadata(data, VERSION)

    def test_required_compatibility_and_recovery_instructions_cannot_be_blank(self):
        for section, field in [("compatibility", "data"), ("compatibility", "configuration"),
                               ("migration", "instructions"), ("recovery", "instructions")]:
            for value in ["", "   ", False, "first\nsecond"]:
                with self.subTest(section=section, field=field, value=value):
                    data = deepcopy(self.data)
                    data[section][field] = value
                    with self.assertRaises(ValueError):
                        validate_metadata(data, VERSION)

    def test_missing_nonregular_oversized_and_invalid_toml_files_are_rejected(self):
        for raw in [b"not toml", b"\xff", b"x" * 32769, b"schema_version = 1\nschema_version = 2"]:
            self.path.write_bytes(raw)
            with self.subTest(raw=raw[:20]), self.assertRaises(ValueError):
                load_metadata(self.root, VERSION)
        self.path.unlink()
        with self.assertRaisesRegex(ValueError, "required"):
            load_metadata(self.root, VERSION)
        target = self.root / "other.toml"
        target.write_text(METADATA)
        self.path.symlink_to(target)
        with self.assertRaisesRegex(ValueError, "regular"):
            load_metadata(self.root, VERSION)

    def test_omitted_edited_duplicated_and_stale_note_blocks_are_rejected(self):
        notes = self.notes()
        block = render_metadata(self.data, self.digest)
        cases = [notes.replace(block, ""), notes.replace("Binary rollback: unsupported", "Binary rollback: supported"),
                 notes + "\n" + block, notes.replace(END, ""), notes.replace(self.digest, "0" * 64)]
        for changed in cases:
            with self.subTest(changed=changed[-100:]), self.assertRaises(ValueError):
                validate_notes(self.root, VERSION, changed)
        self.path.write_text(METADATA + "\n# Reviewed again\n")
        with self.assertRaisesRegex(ValueError, "current validated"):
            validate_notes(self.root, VERSION, notes)

    def test_existing_metadata_in_unreleased_notes_is_not_silently_reused(self):
        changelog = f"# Changelog\n\n## [Unreleased]\n\n{HUMAN}\n\n{START}\n"
        with self.assertRaisesRegex(ValueError, "stale block"):
            preview_changelog(changelog, VERSION, date(2026, 1, 2), render_metadata(self.data, self.digest))

    def test_historical_metadata_blocks_do_not_contaminate_new_release_validation(self):
        old = f"## [0.0.1-preview.1] - 2026-01-01\n\n{START}\nOld warnings\n{END}\n"
        changelog = f"# Changelog\n\n## [Unreleased]\n\n{HUMAN}\n\n{old}"
        generated = preview_changelog(changelog, VERSION, date(2026, 1, 2), render_metadata(self.data, self.digest))
        notes = release_notes.extract_notes(VERSION, VERSION, generated)
        self.assertEqual(validate_notes(self.root, VERSION, notes), self.digest)
        self.assertTrue(generated.endswith(old))

    def test_release_note_cli_requires_metadata_and_prints_nothing_on_failure(self):
        (self.root / "Cargo.toml").write_text(f'[workspace.package]\nversion = "{VERSION}"\n')
        (self.root / "CHANGELOG.md").write_text(self.notes())
        output = io.StringIO()
        with patch.object(release_notes, "ROOT", self.root), patch("sys.argv", ["release_notes.py", VERSION]), patch("sys.stdout", output):
            release_notes.main()
        self.assertIn(self.digest, output.getvalue())
        self.path.unlink()
        output = io.StringIO()
        with patch.object(release_notes, "ROOT", self.root), patch("sys.argv", ["release_notes.py", VERSION]), patch("sys.stdout", output):
            with self.assertRaises(ValueError):
                release_notes.main()
        self.assertEqual(output.getvalue(), "")


class MetadataPlanTests(Fixture):
    def test_missing_stale_or_unsupported_metadata_blocks_before_version_edit_and_output(self):
        from release_plan import main, run as real_run
        cases = [None, METADATA, METADATA.replace('"0.1.0-preview.1"', '"0.2.1-preview.1"').replace("required = false", "required = true")]
        for content in cases:
            with self.subTest(content=content):
                path = self.root / "release-metadata.toml"
                if content is None:
                    path.unlink()
                else:
                    path.write_text(content)
                self.commit("fix: review release metadata")
                def run(command, cwd, env=None):
                    if command[-1] == "--version":
                        return "release-plz 0.3.172\n"
                    self.assertNotIn("set-version", command)
                    return real_run(command, cwd, env)
                output_parent = tempfile.TemporaryDirectory()
                self.addCleanup(output_parent.cleanup)
                output = Path(output_parent.name) / "proposal"
                arguments = ["release_plan.py", "--repository", str(self.root), "--release-plz", "/absent/tool",
                             "--date", "2026-01-02", "--output", str(output)]
                with patch("release_plan.run", side_effect=run), patch("sys.argv", arguments), self.assertRaises(ValueError):
                    main()
                self.assertFalse(output.exists())
                self.assertEqual(real_run(["git", "status", "--porcelain"], self.root), "")
