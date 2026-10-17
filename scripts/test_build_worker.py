"""Bounded artifact handoff checks; no compiler or privileged worker is run."""
from pathlib import Path
import os
import tempfile
import unittest
from build_worker import ARTIFACT_LIMIT, WorkerFailure, read_component_candidate


class ArtifactTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.path = self.root / 'component.wasm'

    def test_copy_is_independent_of_original_file(self):
        self.path.write_bytes(b'\0asm\x0d\0\x01\0')
        data = read_component_candidate(self.path)
        self.path.write_bytes(b'changed')
        self.assertEqual(data, b'\0asm\x0d\0\x01\0')

    def test_native_core_module_empty_and_oversized_artifacts_rejected(self):
        for data in (b'', b'\x7fELFfake native', b'\0asm\x01\0\0\0'):
            self.path.write_bytes(data)
            with self.assertRaises(WorkerFailure):
                read_component_candidate(self.path)
        with self.path.open('wb') as file:
            file.truncate(ARTIFACT_LIMIT + 1)
        with self.assertRaises(WorkerFailure):
            read_component_candidate(self.path)

    def test_symlink_and_fifo_rejected(self):
        source = self.root / 'source'
        source.write_bytes(b'\0asm\x0d\0\x01\0')
        self.path.symlink_to(source)
        with self.assertRaises(WorkerFailure):
            read_component_candidate(self.path)
        self.path.unlink()
        os.mkfifo(self.path)
        with self.assertRaises(WorkerFailure):
            read_component_candidate(self.path)


if __name__ == '__main__':
    unittest.main()
