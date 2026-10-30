"""Acceptance evidence must be complete and bound to the selected isolated VM."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from check_app_installation import CASES, IMAGE_SHA256, host, require_guest, validate_report


def report():
    return dict(success=True, cases=list(CASES), source_commit='a' * 40,
                binary_sha256='b' * 64, runtime_sha256='c' * 64,
                image_sha256=IMAGE_SHA256, production_cli=True,
                private_test_ca=True, public_dns_acme_verified=False)


class AppAcceptanceTests(unittest.TestCase):
    def test_missing_failed_reordered_and_unidentified_evidence_cannot_pass(self):
        good = report()
        self.assertEqual(validate_report(good), good)
        for key, value in (('success', False), ('cases', CASES[:-1]), ('cases', list(reversed(CASES))),
                           ('production_cli', False), ('private_test_ca', False),
                           ('public_dns_acme_verified', True), ('source_commit', 'main'),
                           ('binary_sha256', ''), ('runtime_sha256', 'x' * 64), ('image_sha256', '0' * 64)):
            with self.subTest(key=key, value=value):
                candidate = dict(good, **{key: value})
                with self.assertRaises(ValueError):
                    validate_report(candidate)

    def test_guest_mode_requires_all_explicit_opt_in_guards(self):
        with patch('check_app_installation.os.geteuid', return_value=1000):
            with self.assertRaises(ValueError):
                require_guest()
        with patch('check_app_installation.os.geteuid', return_value=0), patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(ValueError):
                require_guest()

    def test_existing_report_is_preserved_before_any_vm_action(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'report.json'
            path.write_bytes(b'prior evidence')
            with patch('check_app_installation.run') as command:
                with self.assertRaisesRegex(ValueError, 'fresh report'):
                    host(Path(temporary), path)
                command.assert_not_called()
            self.assertEqual(path.read_bytes(), b'prior evidence')

    def test_vm_only_exports_selected_readonly_inputs_and_private_result_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            inputs = root / 'inputs'; inputs.mkdir()
            (inputs / 'metadata.json').write_text(json.dumps(report()))
            output = root / 'report.json'
            def launch(arguments, **kwargs):
                shares = [arguments[i + 1] for i, arg in enumerate(arguments) if arg == '-virtfs']
                self.assertEqual(len(shares), 2)
                self.assertIn('path=' + str(inputs) + ',', shares[0])
                self.assertTrue(shares[0].endswith(',readonly=on'))
                self.assertIn('mount_tag=output', shares[1])
                self.assertNotIn('hostfwd', ' '.join(arguments))
                results = Path(shares[1].split('path=', 1)[1].split(',', 1)[0])
                (results / 'report.json').write_text(json.dumps(report()))
                class Process:
                    returncode = 0
                    def wait(self, timeout):
                        return 0
                    def poll(self):
                        return 0
                return Process()
            with patch('check_app_installation.run'), patch('check_app_installation.image'), \
                    patch('check_app_installation.shutil.which', return_value='/fixture/xorriso'), \
                    patch('check_app_installation.subprocess.Popen', side_effect=launch):
                host(inputs, output)
            self.assertEqual(json.loads(output.read_text()), report())

    def test_mismatched_vm_identity_cannot_produce_successful_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            inputs = root / 'inputs'; inputs.mkdir()
            (inputs / 'metadata.json').write_text(json.dumps(dict(report(), binary_sha256='d' * 64)))
            output = root / 'report.json'
            def launch(arguments, **kwargs):
                result_share = arguments[arguments.index('-virtfs', arguments.index('-virtfs') + 1) + 1]
                result_dir = Path(result_share.split('path=', 1)[1].split(',', 1)[0])
                (result_dir / 'report.json').write_text(json.dumps(report()))
                class Process:
                    returncode = 0
                    def wait(self, timeout):
                        return 0
                    def poll(self):
                        return 0
                return Process()
            with patch('check_app_installation.run'), patch('check_app_installation.image'), \
                    patch('check_app_installation.shutil.which', return_value='/fixture/xorriso'), \
                    patch('check_app_installation.subprocess.Popen', side_effect=launch):
                with self.assertRaisesRegex(ValueError, 'selected prepared inputs'):
                    host(inputs, output)
            self.assertFalse(output.exists())


if __name__ == '__main__':
    unittest.main()
