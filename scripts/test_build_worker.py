"""Bounded artifact handoff checks; no compiler or privileged worker is run."""
from pathlib import Path
import os
import tempfile
import unittest
from build_worker import ARTIFACT_LIMIT, WorkerFailure, build_target, read_component_candidate


class TargetTests(unittest.TestCase):
    def test_existing_and_named_apps_select_only_their_library_artifact(self):
        for directory, package, library in [('web-app', 'devoption-web-fixture', 'devoption_web_fixture'),
                                             ('static-site', 'sample-site', 'sample_site')]:
            manifest = Path('examples', directory, 'Cargo.toml').read_bytes()
            command, artifact = build_target(manifest)
            self.assertEqual(command, ['/toolchain/bin/cargo', 'build', '--release', '--locked',
                                       '--offline', '--target', 'wasm32-wasip2', '--package', package, '--lib'])
            self.assertEqual(artifact, Path(f'wasm32-wasip2/release/{library}.wasm'))

    def test_paths_extra_targets_and_argument_substitution_are_rejected(self):
        original = Path('examples/static-site/Cargo.toml').read_bytes()
        mutations = [
            original.replace(b'sample-site', value) for value in
            (b'../other', b'--all', b'other/site', b'other site', b'other\\nsite', b'x' * 129)
        ] + [
            original.replace(b'sample_site', b'other'),
            original.replace(b'build = false', b'build = true'),
            original.replace(b'crate-type = ["cdylib"]', b'crate-type = ["cdylib", "rlib"]'),
            original.replace(b'[lib]', b'[lib]\npath = "other.rs"'),
            original.replace(b'[lib]', b'[lib]\nname2 = "other"'),
            original + b'\n[[bin]]\nname = "other"\n',
            original + b'\n[workspace]\n',
            b'package = "bad"\nlib = []', b'\xff', b'x' * 65537,
        ]
        for manifest in mutations:
            with self.subTest(manifest=manifest[:100]), self.assertRaises(WorkerFailure):
                build_target(manifest)


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

class LibraryTests(unittest.TestCase):
    def test_library_has_three_fixed_commands_and_no_component_path(self):
        from build_worker import selected_commands
        original = Path('examples/static-site/Cargo.toml').read_bytes()
        manifest = original.replace(b'["cdylib"]', b'["rlib"]')
        commands, artifact = selected_commands(manifest)
        self.assertIsNone(artifact)
        base = ['--release', '--locked', '--offline', '--package', 'sample-site', '--lib']
        self.assertEqual(commands, [['/toolchain/bin/cargo', 'build'] + base,
                                    ['/toolchain/bin/cargo', 'test'] + base,
                                    ['/toolchain/bin/cargo', 'build'] + base + ['--target', 'wasm32-wasip2']])
        with self.assertRaises(WorkerFailure):
            build_target(manifest)
        self.assertEqual(selected_commands(original)[0], [build_target(original)[0]])

    def test_library_target_substitution_is_rejected(self):
        from build_worker import library_commands
        original = Path('examples/static-site/Cargo.toml').read_bytes().replace(b'["cdylib"]', b'["rlib"]')
        for manifest in [original.replace(b'sample_site', b'other'),
                         original.replace(b'["rlib"]', b'["rlib", "cdylib"]'),
                         original.replace(b'[lib]', b'[lib]\npath = "../other.rs"'),
                         original.replace(b'build = false', b'build = true'),
                         original + b'\n[workspace]\n', b'x' * 65537]:
            with self.subTest(manifest=manifest[:100]), self.assertRaises(WorkerFailure):
                library_commands(manifest)


class CompilerSequenceTests(unittest.TestCase):
    def run_commands(self, sources, limit=1048576, seconds=55):
        import io
        import sys
        from unittest.mock import patch
        import build_worker
        output = io.TextIOWrapper(io.BytesIO())
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(build_worker, 'LIMIT', limit), \
                patch.object(build_worker, 'COMPILER_SECONDS', seconds), \
                patch.object(build_worker.sys, 'stderr', output):
            result = build_worker.run_compilers([[sys.executable, '-c', source] for source in sources],
                                                {}, directory)
        output.flush()
        return result, output.buffer.getvalue()

    def test_failed_phase_stops_later_commands(self):
        result, output = self.run_commands(['print("build")', 'raise SystemExit(1)', 'print("must not run")'])
        self.assertFalse(result)
        self.assertEqual(output, b'build\n')

    def test_log_budget_is_shared_across_phases(self):
        with self.assertRaises(WorkerFailure) as failure:
            self.run_commands(['print("12345678")'] * 2, limit=16)
        self.assertEqual(failure.exception.code, 125)

    def test_one_deadline_covers_the_whole_sequence(self):
        with self.assertRaises(WorkerFailure) as failure:
            self.run_commands(['import time; time.sleep(0.15)'] * 3, seconds=0.25)
        self.assertEqual(failure.exception.code, 124)


if __name__ == '__main__':
    unittest.main()
