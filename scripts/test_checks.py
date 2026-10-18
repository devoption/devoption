"""Behavioral tests for release boundaries and contribution metadata."""

import unittest

from check_pr import check_title, issue_references
from release_notes import extract_notes

NOTES = """# Changelog

## [Unreleased]

- A later change that must not leak into this release.

## [0.1.0] - 2026-01-01

- Add help ([#1](https://github.com/devoption/devoption/issues/1)).

## [0.0.1] - 2025-01-01

- An older change that must not leak into this release.
"""


class ReleaseTests(unittest.TestCase):
    def test_numbered_previews_are_valid_but_development_versions_are_not(self):
        notes = NOTES.replace("0.1.0", "0.1.0-preview.1")
        self.assertIn("Add help", extract_notes("0.1.0-preview.1", "0.1.0-preview.1", notes))
        for version in ["0.1.0-dev", "0.1.0-preview.0", "0.1.0-preview.01", "0.1.0-alpha.1"]:
            with self.subTest(version=version), self.assertRaises(ValueError):
                extract_notes(version, version, NOTES.replace("0.1.0", version))

    def test_extracts_only_the_requested_release(self):
        notes = extract_notes("0.1.0", "0.1.0", NOTES)
        self.assertIn("Add help", notes)
        self.assertNotIn("later change", notes)
        self.assertNotIn("older change", notes)

    def test_rejects_unready_or_ambiguous_releases(self):
        cases = [
            ("../bad", "../bad", NOTES),
            ("0.1.0", "0.1.0-dev", NOTES),
            ("0.2.0", "0.2.0", NOTES),
            ("0.1.0", "0.1.0", NOTES + "\n## [0.1.0] - 2026-01-01\n"),
            ("0.1.0", "0.1.0", NOTES.replace("2026-01-01", "9999-01-01")),
            ("0.1.0", "0.1.0", NOTES.replace("2026-01-01", "2026-02-30")),
            ("0.1.0", "0.1.0", NOTES.replace("issues/1", "pull/1")),
        ]
        for case in cases:
            with self.subTest(case=case), self.assertRaises(ValueError):
                extract_notes(*case)


class ContributionTests(unittest.TestCase):
    def test_accepts_scoped_conventional_title_and_issue_footer(self):
        check_title("feat(cli)!: change command output")
        self.assertEqual(issue_references("Description\n\nCloses #12\nRefs #3"), {"12", "3"})

    def test_rejects_placeholder_issue_and_unstructured_title(self):
        with self.assertRaises(ValueError):
            issue_references("Refs #<issue-number>")
        with self.assertRaises(ValueError):
            check_title("misc changes")


if __name__ == "__main__":
    unittest.main()
