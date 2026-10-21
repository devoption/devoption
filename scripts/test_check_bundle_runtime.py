"""A local smoke must never execute bytes that changed after the trusted comparison."""
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

from check_bundle_runtime import check
from release_bundle import TARGET


class RuntimeSmokeTrustTests(unittest.TestCase):
    def test_changed_archive_is_rejected_before_opening_or_executing_worker(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            version = '0.1.0-preview.3'
            (output / f'devoption-{version}-{TARGET}.tar.gz').write_bytes(b'changed after comparison')
            with patch('check_bundle_runtime.check_local', return_value='0'*64), \
                    patch('check_bundle_runtime.workspace', return_value=(version, [])), \
                    patch('check_bundle_runtime.smoke') as execute:
                with self.assertRaisesRegex(ValueError, 'changed before worker smoke'):
                    check(output, output/'platform', output/'runtime', output, output/'fixture.wasm')
            execute.assert_not_called()

    def test_failed_source_comparison_never_reads_or_executes_archive(self):
        with patch('check_bundle_runtime.check_local', side_effect=ValueError('untrusted inputs')), \
                patch('check_bundle_runtime.snapshot') as read, patch('check_bundle_runtime.smoke') as execute:
            with self.assertRaisesRegex(ValueError, 'untrusted'):
                check(Path('/unused'), Path('/unused'), None, Path('/unused'), Path('/unused'))
            read.assert_not_called()
            execute.assert_not_called()
