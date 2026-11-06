"""Disposable-host guards and independent source archive inspection."""
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
from check_library_build import check, consume_archive


class HostGuardTests(unittest.TestCase):
    def test_non_linux_or_root_never_runs_a_command(self):
        for platform, uid in [('darwin', 1000), ('linux', 0)]:
            with patch('check_library_build.sys.platform', platform), \
                    patch('check_library_build.os.geteuid', return_value=uid), \
                    patch('check_library_build.run') as command:
                with self.assertRaisesRegex(ValueError, 'unprivileged'):
                    check(Path('/unused'), Path('/unused'))
                command.assert_not_called()

    def test_existing_installation_is_preserved(self):
        with patch('check_library_build.sys.platform', 'linux'), \
                patch('check_library_build.os.geteuid', return_value=1000), \
                patch.object(Path, 'exists', return_value=True), \
                patch('check_library_build.run') as command:
            with self.assertRaisesRegex(ValueError, 'Existing build installation'):
                check(Path('/unused'), Path('/unused'))
            command.assert_not_called()

    def test_archive_substitution_is_refused_before_consumer_execution(self):
        for name, content, kind in [('reusable-fixture-0.1.0/src/lib.rs', b'changed', tarfile.REGTYPE),
                                    ('../outside', b'escape', tarfile.REGTYPE),
                                    ('reusable-fixture-0.1.0/src/lib.rs', b'', tarfile.SYMTYPE)]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                original = root / 'original'
                (original / 'src').mkdir(parents=True)
                (original / 'src/lib.rs').write_bytes(b'original')
                archive = root / 'source.tar'
                with tarfile.open(archive, 'w') as package:
                    member = tarfile.TarInfo(name)
                    member.type = kind
                    member.size = len(content)
                    package.addfile(member, io.BytesIO(content))
                with patch('check_library_build.run') as run:
                    with self.assertRaises(ValueError):
                        consume_archive(archive, root, original, Path('/unused'))
                    run.assert_not_called()
                self.assertFalse((root / 'outside').exists())
