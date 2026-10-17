#!/usr/bin/python3
"""Verify an explicitly prepared Cargo vendor tree and print installable config.

No downloads, service changes, sudo rules, accounts or credentials are created.
"""
import argparse
import io
import tarfile
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib


def verify_vendor(lockfile, vendor, archives):
    lock = tomllib.loads(lockfile.read_text())
    packages = {(p['name'], p['version']): p['checksum'] for p in lock['package'] if 'source' in p}
    if any(p.get('source') != 'registry+https://github.com/rust-lang/crates.io-index' for p in lock['package'] if 'source' in p):
        raise ValueError('only crates.io packages are supported')
    found = set()
    for package in vendor.iterdir():
        if package.is_symlink() or not package.is_dir():
            raise ValueError('unexpected vendor entry')
        metadata = tomllib.loads((package / 'Cargo.toml').read_text())['package']
        identity = metadata['name'], metadata['version']
        if identity in found or identity not in packages:
            raise ValueError('unexpected or repeated vendored package')
        found.add(identity)
        checksums = json.loads((package / '.cargo-checksum.json').read_text())
        if checksums['package'] != packages[identity]:
            raise ValueError('vendor archive identity does not match lockfile')
        archive = archives / (identity[0] + '-' + identity[1] + '.crate')
        if archive.stat().st_size > 8 * 1024 * 1024:
            raise ValueError('dependency archive exceeds setup limit')
        encoded = archive.read_bytes()
        if hashlib.sha256(encoded).hexdigest() != packages[identity]:
            raise ValueError('dependency archive checksum mismatch')
        archived = {}
        total = 0
        with tarfile.open(fileobj=io.BytesIO(encoded), mode='r:gz') as tar:
            for member in tar:
                prefix = identity[0] + '-' + identity[1] + '/'
                if not member.name.startswith(prefix):
                    raise ValueError('invalid archive root')
                relative = member.name[len(prefix):]
                if any(part in ('', '.', '..') for part in relative.split('/')):
                    raise ValueError('invalid archive path')
                if member.isdir():
                    continue
                total += member.size
                if not member.isfile() or total > 16 * 1024 * 1024 or len(archived) >= 10000 or relative in archived:
                    raise ValueError('unsupported or oversized archive')
                archived[relative] = hashlib.sha256(tar.extractfile(member).read()).hexdigest()
        # Cargo vendor omits the archive's root .gitignore; no executable/input file may differ.
        if set(archived) - set(checksums['files']) not in (set(), {'.gitignore'}) or any(archived.get(name) != digest for name, digest in checksums['files'].items()):
            raise ValueError('vendor checksum manifest does not match verified archive')
        actual = {}
        for path in package.rglob('*'):
            if path.is_symlink():
                raise ValueError('symlinks are not supported')
            if path.is_file() and path != package / '.cargo-checksum.json':
                actual[path.relative_to(package).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
            elif not path.is_file() and not path.is_dir():
                raise ValueError('special vendor files are not supported')
        if actual != checksums['files']:
            raise ValueError('vendor files do not match checksums')
    if found != set(packages):
        raise ValueError('prepared dependency bundle is incomplete')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('devoption', 'toolchain', 'vendor', 'lockfile', 'archives', 'work', 'owner-data', 'repositories', 'history'):
        parser.add_argument('--' + name, required=True, type=Path)
    args = parser.parse_args()
    if os.geteuid() == 0:
        raise ValueError('prepare inputs as the unprivileged platform user')
    for value in vars(args).values():
        if not value.is_absolute() or value.resolve(strict=True) != value or not re.fullmatch(r'/[A-Za-z0-9_./-]+', str(value)):
            raise ValueError('paths must be existing canonical ASCII paths without spaces')
    if args.work.stat().st_uid != os.geteuid() or args.work.stat().st_mode & 0o777 != 0o700:
        raise ValueError('work directory must be private and owned by the platform user')
    for tool in ('rustc', 'cargo'):
        version = subprocess.check_output([str(args.toolchain / 'bin' / tool), '--version'],
                                         env={'PATH': '/usr/bin', 'LANG': 'C.UTF-8'}, text=True)
        if not version.startswith(tool + ' 1.99.0 '):
            raise ValueError('Rust and Cargo 1.99.0 are required')
    if not any((args.toolchain / 'lib/rustlib/wasm32-wasip2/lib').glob('*.rlib')):
        raise ValueError('wasm32-wasip2 standard library is required')
    verify_vendor(args.lockfile, args.vendor, args.archives)
    def digest(path):
        result = subprocess.check_output([str(args.devoption), 'build-input-digest', str(path)], text=True).strip()
        if not re.fullmatch('[0-9a-f]{64}', result):
            raise ValueError('input digest unavailable')
        return result
    config = {'uid': os.geteuid(), 'work': str(args.work), 'owner_data': str(args.owner_data),
              'repositories': str(args.repositories), 'history': str(args.history),
              'toolchain': str(args.toolchain), 'vendor': str(args.vendor),
              'toolchain_sha256': digest(args.toolchain), 'vendor_sha256': digest(args.vendor),
              'lock_sha256': hashlib.sha256(args.lockfile.read_bytes()).hexdigest()}
    for key, value in config.items():
        print(f'{key} = {json.dumps(value)}')


if __name__ == '__main__':
    main()
