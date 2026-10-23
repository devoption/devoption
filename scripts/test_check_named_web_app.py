"""Reject unsafe fixture hosts before any installation or build starts."""
from pathlib import Path
import unittest
from unittest.mock import patch

from check_named_web_app import check


class HostGuardTests(unittest.TestCase):
    def test_non_linux_or_root_never_runs_a_command(self):
        for platform, uid in [('darwin', 1000), ('linux', 0)]:
            with self.subTest(platform=platform, uid=uid), \
                    patch('check_named_web_app.sys.platform', platform), \
                    patch('check_named_web_app.os.geteuid', return_value=uid), \
                    patch('check_named_web_app.run') as command:
                with self.assertRaisesRegex(ValueError, 'unprivileged'):
                    check(Path('/unused'), Path('/unused'))
                command.assert_not_called()

    def test_existing_installation_never_runs_a_command(self):
        with patch('check_named_web_app.sys.platform', 'linux'), \
                patch('check_named_web_app.os.geteuid', return_value=1000), \
                patch.object(Path, 'exists', return_value=True), \
                patch('check_named_web_app.run') as command:
            with self.assertRaisesRegex(ValueError, 'Existing build installation'):
                check(Path('/unused'), Path('/unused'))
            command.assert_not_called()
