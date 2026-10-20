"""Authenticated reboot evidence cannot lose its independently reviewed identity."""
from copy import deepcopy
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from check_upgrade_reboot import host, STAGES
from signed_upgrade_fixture import validate_report, verifier_archive


class SignedEvidenceTests(unittest.TestCase):
    def test_reject_report_for_changed_packet_or_unproven_cli(self):
        previous = {'candidate': {'archive_sha256': 'a'*64}}
        candidate = {'candidate': {'archive_sha256': 'b'*64}}
        value = {'success': True, 'stages': list(STAGES), 'boot_ids': [
            f'00000000-0000-0000-0000-{number:012d}' for number in range(4)],
            'authenticated_packets': True, 'production_cli': True, 'corrupt_packet_rejected': True,
            'previous_identity': previous, 'candidate_identity': candidate}
        self.assertEqual(validate_report(value, previous, candidate), value)
        for key, changed in (('authenticated_packets', False), ('production_cli', None),
                             ('corrupt_packet_rejected', False), ('previous_identity', candidate),
                             ('candidate_identity', previous), ('boot_ids', [value['boot_ids'][0]]*4)):
            result = deepcopy(value); result[key] = changed
            with self.subTest(key=key), self.assertRaises(ValueError):
                validate_report(result, previous, candidate)

    def test_bad_verifier_pin_and_failed_authentication_never_boot(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'gh.tar.gz').write_bytes(b'not the pinned verifier')
            with self.assertRaisesRegex(ValueError, 'pinned Linux'):
                verifier_archive(root)
            with patch('signed_upgrade_fixture.installers', side_effect=ValueError('signature rejected')), patch('check_upgrade_reboot.image') as image:
                with self.assertRaisesRegex(ValueError, 'signature rejected'):
                    host(root, root/'report.json', signed=True, gh=root/'trusted-gh')
                image.assert_not_called()


if __name__ == '__main__':
    unittest.main()
