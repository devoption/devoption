from contextlib import ExitStack, contextmanager
import hashlib
import os
from pathlib import Path
import stat
import tempfile
import unittest
from unittest import mock

import rehearsal_artifact_handoff as handoff
from rehearsal_artifact_metadata import describe_artifact_metadata


class ArtifactHandoffTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="devoption-handoff-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.output = self.root / "output"
        self.staging = self.root / "staging"
        for path in [self.output, self.staging, self.output / "release",
                     self.output / "release/deps"]:
            path.mkdir(mode=0o700)
        self.parent_fd = os.open(self.root, os.O_RDONLY | os.O_DIRECTORY)
        self.addCleanup(os.close, self.parent_fd)
        self.paths = ("release/fixture", "release/deps/fixture-hash")
        self.source = self.output / self.paths[0]
        self.alias = self.output / self.paths[1]
        self.destination = self.staging / handoff.ARTIFACT_NAME
        self.payload = b"trusted fixture bytes\n" * 4000
        self.source.write_bytes(self.payload)
        self.source.chmod(0o700)
        os.link(self.source, self.alias)

    @contextmanager
    def pinned(self):
        with ExitStack() as stack:
            source = stack.enter_context(handoff.PinnedDirectory(
                self.parent_fd, "output", os.geteuid()))
            staging = stack.enter_context(handoff.PinnedDirectory(
                self.parent_fd, "staging", os.geteuid()))
            yield source, staging

    def run_handoff(self, paths=None):
        with self.pinned() as (source, staging):
            with handoff.handoff_stopped_artifact(
                    source, self.paths if paths is None else paths, staging) as receipt:
                return receipt

    def assert_rejected(self, paths=None):
        with self.assertRaises(handoff.HandoffError):
            self.run_handoff(paths)
        self.assertFalse(self.destination.exists())

    @contextmanager
    def mutate_after_source_read(self, action):
        source_id = (self.source.stat().st_dev, self.source.stat().st_ino)
        read = os.read
        changed = False

        def intercepted(fd, count):
            nonlocal changed
            block = read(fd, count)
            opened = os.fstat(fd)
            if not changed and (opened.st_dev, opened.st_ino) == source_id:
                changed = True
                action()
            return block

        with mock.patch.object(handoff.os, "read", side_effect=intercepted):
            yield
        self.assertTrue(changed, "fixture must reach source copy")

    def test_two_link_output_yields_independent_private_single_link_destination(self):
        self.assertEqual(self.source.stat().st_nlink, 2)
        self.assertFalse(describe_artifact_metadata(self.source.stat())["metadata_accepted"])
        with self.pinned() as (source, staging):
            with handoff.handoff_stopped_artifact(source, self.paths, staging) as receipt:
                metadata = self.destination.stat()
                self.assertNotEqual(metadata.st_ino, self.source.stat().st_ino)
                self.assertEqual(metadata.st_nlink, 1)
                self.assertEqual(metadata.st_uid, os.geteuid())
                self.assertEqual(stat.S_IMODE(metadata.st_mode), 0o400)
                self.assertTrue(describe_artifact_metadata(metadata)["metadata_accepted"])
                self.assertEqual(receipt.bytes, len(self.payload))
                self.assertEqual(receipt.sha256, hashlib.sha256(self.payload).hexdigest())
                self.assertEqual(receipt.source_links, 2)
                # After the receipt, edits through either original name cannot
                # change the independent destination inode.
                self.alias.write_bytes(b"changed source after handoff")
                self.assertEqual(self.destination.read_bytes(), self.payload)
        self.assertFalse(self.destination.exists())

    def test_single_link_source_and_inclusive_size_cap(self):
        self.alias.unlink()
        with self.source.open("r+b") as file:
            file.truncate(handoff.MAX_ARTIFACT_BYTES)
        receipt = self.run_handoff((self.paths[0],))
        self.assertEqual(receipt.bytes, handoff.MAX_ARTIFACT_BYTES)
        self.assertEqual(receipt.source_links, 1)

    def test_empty_and_oversized_files_fail_before_destination_creation(self):
        for size in (0, handoff.MAX_ARTIFACT_BYTES + 1):
            with self.subTest(size=size):
                with self.source.open("r+b") as file:
                    file.truncate(size)
                self.assert_rejected()

    def test_outside_hard_link_is_not_accounted_for_by_in_root_names(self):
        outside = self.root / "outside"
        os.link(self.source, outside)
        self.assert_rejected()
        self.assertEqual(outside.read_bytes(), self.payload)

    def test_missing_duplicate_and_wrong_aliases_fail_closed(self):
        self.assert_rejected(())
        self.assert_rejected(self.paths + (self.paths[0],))
        self.assert_rejected((self.paths[0],))
        self.assert_rejected((self.paths[0], self.paths[0]))
        self.alias.unlink()
        self.alias.write_bytes(self.payload)
        self.alias.chmod(0o700)
        self.assert_rejected()

    def test_invalid_or_unbounded_paths_never_escape_source(self):
        for path in ("", "/outside", "../outside", "release/../fixture", "release//fixture",
                     "release/./fixture", "release/fixture\x00", "é", "a/" * 16 + "a",
                     "a" * 256, "a/" * 600):
            with self.subTest(path=path):
                self.assert_rejected((path,))

    def test_symlink_leaf_and_ancestor_are_not_followed(self):
        outside = self.root / "outside"
        outside.write_bytes(b"private outside bytes")
        self.source.unlink()
        self.source.symlink_to(outside)
        self.assert_rejected()
        self.source.unlink()
        (self.output / "release").rename(self.root / "moved-release")
        (self.output / "release").symlink_to(self.root / "moved-release")
        self.assert_rejected()
        self.assertEqual(outside.read_bytes(), b"private outside bytes")

    def test_fifo_directory_and_unsafe_file_mode_fail_without_reading(self):
        self.source.unlink()
        os.mkfifo(self.source, 0o600)
        self.assert_rejected()
        self.source.unlink()
        self.source.mkdir(mode=0o700)
        self.assert_rejected()
        self.source.rmdir()
        self.source.write_bytes(self.payload)
        self.source.chmod(0o777)
        self.assert_rejected()

    def test_replaced_pinned_source_root_is_rejected(self):
        with self.pinned() as (source, staging):
            self.output.rename(self.root / "original-output")
            self.output.mkdir(mode=0o700)
            with self.assertRaises(handoff.HandoffError):
                with handoff.handoff_stopped_artifact(source, self.paths, staging):
                    self.fail("replacement must not produce a receipt")
        self.assertFalse(self.destination.exists())

    def test_source_leaf_replacement_during_copy_is_rejected(self):
        def replace():
            self.source.unlink()
            self.source.write_bytes(b"replacement")
            self.source.chmod(0o700)
        with self.mutate_after_source_read(replace):
            self.assert_rejected()
        self.assertEqual(self.source.read_bytes(), b"replacement")

    def test_replacement_between_metadata_check_and_open_does_not_follow_symlink(self):
        outside = self.root / "outside"
        outside.write_bytes(b"outside data")
        open_file = os.open
        changed = False
        def intercepted(path, flags, *args, **kwargs):
            nonlocal changed
            if path == "fixture" and not changed:
                changed = True
                self.source.unlink()
                self.source.symlink_to(outside)
            return open_file(path, flags, *args, **kwargs)
        with mock.patch.object(handoff.os, "open", side_effect=intercepted):
            self.assert_rejected()
        self.assertTrue(changed)
        self.assertEqual(outside.read_bytes(), b"outside data")

    def test_source_directory_replacement_during_copy_is_rejected(self):
        def replace():
            (self.output / "release").rename(self.output / "old-release")
            (self.output / "release").symlink_to(self.output / "old-release")
        with self.mutate_after_source_read(replace):
            self.assert_rejected()

    def test_source_growth_shrink_and_same_size_changes_fail_closed(self):
        for change in ("grow", "shrink", "rewrite"):
            with self.subTest(change=change):
                self.source.write_bytes(self.payload)
                def mutate():
                    with self.source.open("r+b") as file:
                        if change == "grow":
                            file.truncate(len(self.payload) + 1)
                        elif change == "shrink":
                            file.truncate(1)
                        else:
                            file.write(b"changed bytes")
                with self.mutate_after_source_read(mutate):
                    self.assert_rejected()

    def test_existing_staging_entry_is_preserved(self):
        self.destination.write_bytes(b"existing user data")
        with self.assertRaises(handoff.HandoffError):
            self.run_handoff()
        self.assertEqual(self.destination.read_bytes(), b"existing user data")

    def test_replaced_staging_root_does_not_receive_or_lose_files(self):
        def replace():
            self.staging.rename(self.root / "original-staging")
            self.staging.mkdir(mode=0o700)
            self.destination.write_bytes(b"replacement data")
        with self.mutate_after_source_read(replace):
            with self.assertRaises(handoff.HandoffError):
                self.run_handoff()
        self.assertEqual(self.destination.read_bytes(), b"replacement data")
        self.assertEqual(list((self.root / "original-staging").iterdir()), [])

    def test_destination_replacement_requires_quarantine_and_preserves_replacement(self):
        def replace():
            self.destination.unlink()
            self.destination.write_bytes(b"replacement data")
        with self.mutate_after_source_read(replace):
            with self.assertRaisesRegex(handoff.HandoffError, "cleanup conflict"):
                self.run_handoff()
        self.assertEqual(self.destination.read_bytes(), b"replacement data")

    def test_new_destination_link_is_rejected_and_not_unlinked_as_unknown_data(self):
        alias = self.staging / "unexpected-link"
        with self.mutate_after_source_read(lambda: os.link(self.destination, alias)):
            with mock.patch.object(handoff.hashlib, "sha256") as hash_bytes:
                self.assert_rejected()
                hash_bytes.assert_not_called()
        self.assertTrue(alias.exists())

    def test_partial_write_failure_removes_only_the_owned_destination(self):
        write = os.write
        calls = 0
        def interrupted(fd, data):
            nonlocal calls
            calls += 1
            if calls > 1:
                raise OSError("fixture disk full")
            return write(fd, data[:5])
        with mock.patch.object(handoff.os, "write", side_effect=interrupted):
            self.assert_rejected()
        self.assertEqual(self.source.read_bytes(), self.payload)

    def test_destination_changes_during_hashing_fail_closed(self):
        read = os.read
        changed = False
        def intercepted(fd, count):
            nonlocal changed
            block = read(fd, count)
            if self.destination.exists() and not changed:
                opened, named = os.fstat(fd), self.destination.stat()
                if (opened.st_dev, opened.st_ino) == (named.st_dev, named.st_ino):
                    changed = True
                    self.destination.chmod(0o600)
                    with self.destination.open("r+b") as file:
                        file.write(b"tampered")
                    self.destination.chmod(0o400)
            return block
        with mock.patch.object(handoff.os, "read", side_effect=intercepted):
            self.assert_rejected()
        self.assertTrue(changed)

    def test_unsafe_or_wrong_owner_directories_and_non_siblings_fail(self):
        self.staging.chmod(0o755)
        with self.assertRaises(handoff.HandoffError):
            self.run_handoff()
        self.staging.chmod(0o700)
        with self.assertRaises(handoff.HandoffError):
            handoff.PinnedDirectory(self.parent_fd, "output", os.geteuid() + 1)
        with self.pinned() as (source, _):
            (self.output / "nested").mkdir(mode=0o700)
            with handoff.PinnedDirectory(source.fd, "nested", os.geteuid()) as nested:
                with self.assertRaises(handoff.HandoffError):
                    with handoff.handoff_stopped_artifact(source, self.paths, nested):
                        self.fail("nested staging must not be accepted")


if __name__ == "__main__":
    unittest.main()
