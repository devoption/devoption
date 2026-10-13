import json
import os
from pathlib import Path
import tempfile
import unittest

from rehearsal_artifact_metadata import MAX_ARTIFACT_BYTES, describe_artifact_metadata


class RehearsalArtifactMetadataTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="devoption-metadata-")
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / "fixture"

    def report(self):
        with self.path.open("rb") as artifact:
            return describe_artifact_metadata(os.fstat(artifact.fileno()))

    def test_regular_nonempty_single_link_reports_metadata_without_claiming_hash(self):
        self.path.write_bytes(b"fixture")
        report = self.report()
        self.assertTrue(report["metadata_accepted"])
        self.assertEqual(report["failed_checks"], [])
        self.assertEqual(report["link_count"], 1)
        self.assertEqual(report["size_bytes"], 7)
        self.assertEqual(report["max_size_bytes"], 4 * 1024 * 1024)
        self.assertEqual(set(report["checks"]), {
            "regular_file", "single_link", "nonempty", "within_size_limit",
        })
        self.assertNotIn("sha256", report)
        self.assertNotIn(str(self.path), json.dumps(report))

    def test_hard_link_preserves_rejection_and_identifies_only_link_count(self):
        self.path.write_bytes(b"fixture")
        os.link(self.path, self.path.with_name("second-link"))
        report = self.report()
        self.assertFalse(report["metadata_accepted"])
        self.assertEqual(report["failed_checks"], ["single_link"])
        self.assertEqual(report["link_count"], 2)
        self.assertEqual(report["size_bytes"], 7)

    def test_empty_hard_link_reports_both_failures(self):
        self.path.touch()
        os.link(self.path, self.path.with_name("second-link"))
        report = self.report()
        self.assertFalse(report["metadata_accepted"])
        self.assertEqual(report["failed_checks"], ["single_link", "nonempty"])
        self.assertEqual(json.loads(json.dumps(report))["size_bytes"], 0)

    def test_exact_size_limit_passes_and_one_byte_over_reports_size_failure(self):
        for size, accepted in [(MAX_ARTIFACT_BYTES, True), (MAX_ARTIFACT_BYTES + 1, False)]:
            with self.subTest(size=size):
                with self.path.open("wb") as artifact:
                    artifact.truncate(size)
                report = self.report()
                self.assertEqual(report["metadata_accepted"], accepted)
                self.assertEqual(report["size_bytes"], size)
                self.assertEqual(report["failed_checks"], [] if accepted else ["within_size_limit"])

    def test_directory_and_symlink_are_reported_as_nonregular(self):
        target = self.path.with_name("target")
        target.write_bytes(b"fixture")
        self.path.symlink_to(target)
        for path in [self.path.parent, self.path]:
            with self.subTest(kind="symlink" if path == self.path else "directory"):
                report = describe_artifact_metadata(path.lstat())
                self.assertFalse(report["metadata_accepted"])
                self.assertIn("regular_file", report["failed_checks"])


if __name__ == "__main__":
    unittest.main()
