"""The reboot evidence gate must reject incomplete or same-kernel results."""
import unittest
from unittest.mock import patch

from check_upgrade_reboot import STAGES, require_guest, validate_report


class RebootEvidenceTests(unittest.TestCase):
    def test_guest_requires_both_root_and_explicit_disposable_opt_in(self):
        for uid, environment in ((1000, {}), (0, {}), (1000, {'DEVOPTION_DISPOSABLE_UPGRADE_FIXTURE': '1'})):
            with patch('os.geteuid', return_value=uid), patch.dict('os.environ', environment, clear=True):
                with self.assertRaises(ValueError):
                    require_guest()
        with patch('os.geteuid', return_value=0), patch.dict('os.environ', {'DEVOPTION_DISPOSABLE_UPGRADE_FIXTURE': '1'}, clear=True):
            require_guest()

    def record(self):
        return {'success': True, 'stages': list(STAGES), 'boot_ids': [
            f'00000000-0000-0000-0000-{number:012d}' for number in range(4)]}

    def test_complete_four_distinct_boots(self):
        value = self.record()
        self.assertEqual(validate_report(value), value)

    def test_incomplete_failed_reordered_or_same_kernel_cannot_pass(self):
        for key, value in (('success', False), ('success', 1), ('stages', STAGES[:3]),
                           ('stages', list(reversed(STAGES))), ('boot_ids', ['bad']*4), ('boot_ids', None), ('boot_ids', 4),
                           ('boot_ids', self.record()['boot_ids'][:3]),
                           ('boot_ids', [self.record()['boot_ids'][0]]*4)):
            with self.subTest(key=key, value=value):
                result = self.record(); result[key] = value
                with self.assertRaises(ValueError):
                    validate_report(result)
        for result in ({}, None, [], {'success': True}):
            with self.assertRaises(ValueError):
                validate_report(result)


if __name__ == '__main__':
    unittest.main()
