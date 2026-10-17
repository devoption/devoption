"""Offline prepared-dependency verification; no compiler, root or network."""
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

from prepare_build_config import verify_vendor


class PreparedBundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.vendor = root / 'vendor'
        self.package = self.vendor / 'example'
        self.package.mkdir(parents=True)
        self.archives = root / 'archives'
        self.archives.mkdir()
        files = {'Cargo.toml': b'[package]\nname="example"\nversion="1.0.0"\n',
                 'src/lib.rs': b'pub fn example() {}\n'}
        self.archive = self.archives / 'example-1.0.0.crate'
        with tarfile.open(self.archive, 'w:gz') as tar:
            for name, data in files.items():
                member = tarfile.TarInfo('example-1.0.0/' + name)
                member.size = len(data)
                tar.addfile(member, io.BytesIO(data))
                target = self.package / name
                target.parent.mkdir(exist_ok=True)
                target.write_bytes(data)
        self.digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.manifest = {'package': self.digest, 'files': {
            name: hashlib.sha256(data).hexdigest() for name, data in files.items()}}
        self.checksum = self.package / '.cargo-checksum.json'
        self.checksum.write_text(json.dumps(self.manifest))
        self.lock = root / 'Cargo.lock'
        self.lock.write_text('version=4\n[[package]]\nname="example"\nversion="1.0.0"\n'
                             'source="registry+https://github.com/rust-lang/crates.io-index"\n'
                             f'checksum="{self.digest}"\n')

    def verify(self):
        verify_vendor(self.lock, self.vendor, self.archives)

    def test_complete_bundle(self):
        self.verify()

    def test_forged_local_manifest_does_not_authenticate_changed_source(self):
        changed = b'changed source'
        (self.package / 'src/lib.rs').write_bytes(changed)
        self.manifest['files']['src/lib.rs'] = hashlib.sha256(changed).hexdigest()
        self.checksum.write_text(json.dumps(self.manifest))
        with self.assertRaisesRegex(ValueError, 'verified archive'):
            self.verify()

    def test_corrupt_archive(self):
        self.archive.write_bytes(b'corrupt')
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            self.verify()

    def test_extra_file(self):
        (self.package / 'build.rs').write_text('unexpected')
        with self.assertRaises(ValueError):
            self.verify()

    def test_missing_package(self):
        self.lock.write_text(self.lock.read_text() + '\n[[package]]\nname="missing"\nversion="1"\n'
                             'source="registry+https://github.com/rust-lang/crates.io-index"\n'
                             f'checksum="{self.digest}"\n')
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            self.verify()

    def test_source_symlink(self):
        source = self.package / 'src/lib.rs'
        data = source.read_bytes()
        source.unlink()
        external = self.package.parent.parent / 'outside'
        external.write_bytes(data)
        source.symlink_to(external)
        with self.assertRaisesRegex(ValueError, 'symlinks'):
            self.verify()


if __name__ == '__main__':
    unittest.main()
