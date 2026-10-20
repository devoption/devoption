"""Disposable filesystem/host models; no system account, service or real credential changes."""

from copy import deepcopy
import errno
import hashlib
import io
import os
from pathlib import Path
import socket
import subprocess
import sys
import tarfile
import unittest
from unittest.mock import MagicMock, patch

from install_bundle import Installer, PLATFORM, STATE, fetch_packet, installation_lock, proxy_binary, verified_files
from installer_host import Host
from release_bundle import CADDY_SHA512, CADDY_VERSION, canonical, RUNTIME
import test_release_candidates as fixtures
from test_bundle_provenance import GH_VERSION


class FixtureHost:
    """Explicit test adapter, unreachable from the operator CLI."""
    def __init__(self, root):
        self.root = root
        self.root_uid = os.getuid()
        self.accounts = {"root": (os.getuid(), os.getgid())}
        self.calls = []
        self.running = False
        self.blocked_port = None

    def preflight(self, root, required_bytes):
        assert root == self.root and required_bytes > 1024**3

    def account(self, name):
        return self.accounts.get(name)

    def check_account(self, name, home):
        pass

    def create_account(self, name, home):
        if name not in self.accounts:
            self.accounts[name] = (os.getuid(), os.getgid())
            self.calls.append(("account", name))

    def own(self, path, uid, gid):
        assert uid == os.getuid() and gid == os.getgid()

    def sync(self, directory):
        pass

    def active(self, service):
        return self.running

    def free_port(self, port):
        if port == self.blocked_port:
            raise ValueError("occupied fixture port")

    def validate(self, binary, caddy, hostname):
        assert (self.root / binary.lstrip('/')).is_file()
        self.calls.append(("validate",))

    def runtime_probe(self, binary, version):
        assert (self.root / binary.lstrip('/')).is_file()
        self.calls.append(("runtime-probe", version))

    def reload(self):
        self.calls.append(("reload",))

    def enroll(self, binary):
        # Synthetic record only; exact packaged binary/PTY setup is tested by CI.
        path = self.root / "var/lib/devoption/data/owner.bin"
        path.write_bytes(b"disposable owner record fixture")
        path.chmod(0o600)
        self.calls.append(("enroll",))

    def start(self):
        self.running = True
        self.calls.append(("start",))

    def healthy(self, hostname):
        self.calls.append(("healthy",))


class InstallerTests(unittest.TestCase):
    repack = fixtures.ReservationTests.repack
    response = fixtures.ReservationTests.response
    commit = fixtures.ReservationTests.commit
    rezip = fixtures.ReservationTests.rezip

    def setUp(self):
        fixtures.ReservationTests.setUp(self)
        self.image = self.root / "image"
        self.image.mkdir(mode=0o700)
        for path in ('opt', 'var/lib', 'etc/systemd/system'):
            (self.image / path).mkdir(parents=True)
        self.host = FixtureHost(self.image)
        self.content = self.files.copy()
        # Reservation fixtures' self.files is the ZIP packet, not archive members.
        with tarfile.open(self.archive) as archive:
            self.content = {m.name: archive.extractfile(m).read() for m in archive}
        self.installer = self.build()

    def build(self, record=None, hostname="platform.example.com"):
        return Installer(self.image, self.host, record or self.record, self.manifest,
                         self.content, b"pinned proxy fixture; never executed", hostname)

    def test_dual_bundle_installs_worker_immutably_and_probes_before_reload(self):
        self.manifest['schema_version'] = 3
        self.content[RUNTIME] = b'verified worker bytes; host execution is modeled'
        self.content['manifest.json'] = canonical(self.manifest)
        installer = self.build()
        self.host.runtime_probe = lambda binary, version: (_ for _ in ()).throw(ValueError('worker version mismatch'))
        with self.assertRaisesRegex(ValueError, 'worker version'):
            installer.install()
        self.assertNotIn(('reload',), self.host.calls)
        path = installer.path(installer.release + '/' + RUNTIME)
        self.assertEqual(path.read_bytes(), self.content[RUNTIME])
        self.assertEqual(path.stat().st_mode & 0o777, 0o755)
        self.host.runtime_probe = lambda binary, version: self.host.calls.append(('runtime-probe', version))
        installer.install()
        self.assertIn(('runtime-probe', installer.version), self.host.calls)
        path.write_bytes(b'changed worker')
        with self.assertRaisesRegex(ValueError, 'differs'):
            installer.install()
        self.assertEqual(path.read_bytes(), b'changed worker')

    def test_fresh_install_enroll_start_and_identical_retry_preserve_data(self):
        before = self.installer.preflight()
        self.assertFalse(before['resumed'])
        self.assertFalse((self.image / STATE.lstrip('/')).exists())
        result = self.installer.install()
        self.assertEqual(result['missing_files'], [])
        self.assertFalse(result['owner_enrolled'])
        unit = (self.image / 'etc/systemd/system/devoption-public.service').read_text()
        self.assertIn(self.installer.binary, unit)
        self.assertIn('ExecStartPre=/usr/bin/test -s /var/lib/devoption/data/owner.bin', unit)
        self.assertIn('--public-origin https://platform.example.com', unit)
        with self.assertRaisesRegex(ValueError, 'Enroll'):
            self.installer.start()
        self.installer.enroll()
        data = self.image / 'var/lib/devoption/data'
        (data / 'projects.bin').write_bytes(b'preserved catalog fixture')
        (data / 'profile.bin').write_bytes(b'preserved profile fixture')
        saved = {p.name: p.read_bytes() for p in data.iterdir()}
        inodes = {name: self.installer.path(name).stat().st_ino for name in self.installer.output}
        self.installer.start()
        self.installer.install()
        self.installer.enroll()
        self.assertEqual([call for call in self.host.calls if call[0]=='enroll'], [('enroll',)])
        self.assertEqual({p.name: p.read_bytes() for p in data.iterdir()}, saved)
        self.assertEqual({name: self.installer.path(name).stat().st_ino for name in self.installer.output}, inodes)
        self.assertEqual(self.installer.preflight()['missing_files'], [])

    def test_interrupted_file_commits_resume_without_replacement(self):
        write = self.installer.write
        count = 0
        def interrupted(*args):
            nonlocal count
            write(*args)
            count += 1
            if count == 6:
                raise RuntimeError('simulated process interruption after durable file commit')
        with patch.object(self.installer, 'write', side_effect=interrupted):
            with self.assertRaisesRegex(RuntimeError, 'interruption'):
                self.installer.install()
        existing = {name: self.installer.path(name).stat().st_ino for name in self.installer.output if self.installer.path(name).exists()}
        self.assertTrue(self.installer.preflight()['resumed'])
        self.assertTrue(self.installer.preflight()['missing_files'])
        self.assertEqual(self.installer.install()['missing_files'], [])
        for name, inode in existing.items():
            self.assertEqual(self.installer.path(name).stat().st_ino, inode)

    def test_conflicts_and_occupied_ports_fail_before_mutation(self):
        self.host.blocked_port = 443
        with self.assertRaisesRegex(ValueError, 'occupied'):
            self.installer.install()
        self.assertEqual(self.host.calls, [])
        self.assertFalse(self.installer.path(STATE).exists())
        self.host.blocked_port = None
        self.host.accounts['devoption'] = (os.getuid(), os.getgid())
        with self.assertRaisesRegex(ValueError, 'service account'):
            self.installer.install()
        self.host.accounts.pop('devoption')
        foreign = self.image / 'opt/devoption'
        foreign.mkdir()
        (foreign / 'sentinel').write_text('existing installation')
        with self.assertRaisesRegex(ValueError, 'Unmanaged'):
            self.installer.install()
        self.assertEqual((foreign / 'sentinel').read_text(), 'existing installation')

    def test_changed_binary_unit_identity_symlink_and_hardlink_fail_closed(self):
        self.installer.install()
        path = self.installer.path(self.installer.binary)
        original = path.read_bytes()
        path.write_bytes(b'edited binary')
        with self.assertRaisesRegex(ValueError, 'never replace'):
            self.installer.install()
        path.write_bytes(original)
        alias = path.parent / 'extra-link'
        os.link(path, alias)
        with self.assertRaisesRegex(ValueError, 'never replace'):
            self.installer.install()
        alias.unlink()
        path.unlink()
        path.symlink_to(self.archive)
        with self.assertRaisesRegex(ValueError, 'never replace'):
            self.installer.install()
        path.unlink()
        path.write_bytes(original)
        path.chmod(0o755)
        with self.assertRaisesRegex(ValueError, 'upgrade/configuration'):
            self.build(hostname='other.example.com').install()
        record = deepcopy(self.record)
        record['artifact']['transport_sha256'] = '0'*64
        with self.assertRaisesRegex(ValueError, 'upgrade/configuration'):
            self.build(record=record).install()

    def test_writable_parent_and_invalid_owner_are_preserved(self):
        (self.image / 'opt').chmod(0o777)
        with self.assertRaisesRegex(ValueError, 'Unsafe'):
            self.installer.install()
        self.assertFalse(self.installer.path(STATE).exists())
        (self.image / 'opt').chmod(0o755)
        self.installer.install()
        owner = self.installer.path(PLATFORM + '/data/owner.bin')
        owner.write_bytes(b'')
        owner.chmod(0o600)
        with self.assertRaisesRegex(ValueError, 'preserve'):
            self.installer.enroll()
        self.assertEqual(owner.read_bytes(), b'')

    def test_partial_files_changed_modes_and_unexpected_inventory_require_inspection(self):
        self.installer.install()
        unit = self.installer.path('/etc/systemd/system/devoption-public.service')
        unit.chmod(0o666)
        with self.assertRaisesRegex(ValueError, 'never replace'):
            self.installer.install()
        unit.chmod(0o644)
        unknown = self.installer.path(self.installer.release+'/unexpected.py')
        unknown.write_bytes(b'unknown input')
        with self.assertRaisesRegex(ValueError, 'Unexpected'):
            self.installer.install()
        self.assertEqual(unknown.read_bytes(), b'unknown input')

    def test_unsupported_manifest_is_rejected_before_file_placement(self):
        self.manifest['schema_version'] = 999
        self.repack()
        self.candidate['archive_sha256'] = self.digest
        self.files[self.name] = self.archive.read_bytes()
        self.files['candidate.json'] = canonical(self.candidate)
        self.rezip()
        def command(args, env):
            return GH_VERSION.encode() if args[-1]=='--version' else self.response()
        with patch('verify_bundle_provenance.command', side_effect=command):
            with self.assertRaisesRegex(ValueError, 'manifest'):
                verified_files(self.transport,self.record,self.gh,True)
        self.assertEqual(self.host.calls,[])
        self.assertFalse(self.installer.path(STATE).exists())

    def test_verified_packet_and_signature_or_digest_failure_before_installation(self):
        def command(args, env):
            return GH_VERSION.encode() if args[-1]=='--version' else self.response()
        with patch('verify_bundle_provenance.command', side_effect=command):
            manifest, files = verified_files(self.transport, self.record, self.gh, True)
        self.assertEqual(manifest['source_commit'], self.source)
        self.assertEqual(files['bin/devoption'], self.content['bin/devoption'])
        with self.assertRaisesRegex(ValueError, 'validation candidate'):
            verified_files(self.transport, self.record, self.gh, False)
        with patch('verify_bundle_provenance.command', side_effect=subprocess.CalledProcessError(1,'gh')):
            with self.assertRaises(subprocess.CalledProcessError):
                verified_files(self.transport, self.record, self.gh, True)
        self.transport.write_bytes(b'corrupt downloaded packet')
        with self.assertRaisesRegex(ValueError, 'ZIP differs'):
            verified_files(self.transport, self.record, self.gh, True)
        self.assertFalse(self.installer.path(STATE).exists())
        self.assertEqual(self.host.calls, [])

    def test_proxy_archive_pin_and_download_failures_never_install(self):
        proxy = self.root / 'proxy.tar.gz'
        with tarfile.open(proxy, 'w:gz') as archive:
            info = tarfile.TarInfo('caddy')
            info.size = 64
            archive.addfile(info, io.BytesIO(b'x'*64))
        manifest = {'external_requirements': {'proxy': {'version': CADDY_VERSION, 'sha512': CADDY_SHA512}}}
        with self.assertRaisesRegex(ValueError, 'SHA-512'):
            proxy_binary(proxy, manifest)
        digest = hashlib.sha512(proxy.read_bytes()).hexdigest()
        manifest['external_requirements']['proxy']['sha512'] = digest
        with patch('install_bundle.CADDY_SHA512', digest):
            self.assertEqual(proxy_binary(proxy, manifest), b'x'*64)
        launch = subprocess.Popen
        def failure(*args, **kwargs):
            return launch([sys.executable,'-c','raise SystemExit(1)'], **kwargs)
        with patch('install_bundle.subprocess.Popen', side_effect=failure):
            with self.assertRaisesRegex(ValueError, 'download failed') as failure:
                fetch_packet('123', self.root / 'failed.zip', self.gh)
        self.assertNotIn('private', str(failure.exception))
        with self.assertRaises(ValueError):
            fetch_packet('../other', self.root / 'unused', self.gh)
        self.assertFalse(self.installer.path(STATE).exists())

    def test_download_stream_is_bounded_and_uses_fixed_release_asset_route(self):
        launch = subprocess.Popen
        def output(args, **kwargs):
            self.assertEqual(args[1:],["api","--hostname","github.com","--method","GET",
                                      "repos/devoption/devoption/releases/assets/123","-H","Accept: application/octet-stream"])
            return launch([sys.executable,'-c',"import sys; sys.stdout.buffer.write(b'x'*100)"], **kwargs)
        with patch('install_bundle.subprocess.Popen',side_effect=output), patch('install_bundle.PACKET_LIMIT',32):
            with self.assertRaisesRegex(ValueError,'size limit'):
                fetch_packet('123',self.root/'too-large.zip',self.gh)
        self.assertLessEqual((self.root/'too-large.zip').stat().st_size,32)
        with patch('install_bundle.subprocess.Popen',side_effect=output):
            fetch_packet('123',self.root/'download.zip',self.gh)
        self.assertEqual((self.root/'download.zip').read_bytes(),b'x'*100)

    def test_real_host_rejects_unsupported_host_and_occupied_listener(self):
        with patch('installer_host.os.geteuid', return_value=1000):
            with self.assertRaisesRegex(ValueError, 'Ubuntu'):
                Host().preflight(Path('/'),1024**3)
        with socket.socket() as listener:
            listener.bind(('127.0.0.1',0))
            with self.assertRaisesRegex(ValueError, 'unavailable'):
                Host().free_port(listener.getsockname()[1])
        with patch('installer_host.os.isatty',return_value=False):
            with self.assertRaisesRegex(ValueError, 'interactive SSH'):
                Host().enroll('/never/execute')

    def test_mutation_lock_serializes_installers_and_rejects_symlinks(self):
        lock = self.root / 'installer.lock'
        with installation_lock(lock,owner=os.getuid()):
            with self.assertRaises(BlockingIOError):
                with installation_lock(lock,owner=os.getuid()):
                    self.fail('Concurrent installer acquired the mutation lock')
        with installation_lock(lock,owner=os.getuid()):
            pass
        lock.unlink()
        sentinel = self.root / 'sentinel'
        sentinel.write_bytes(b'preserve unrelated file')
        lock.symlink_to(sentinel)
        with self.assertRaises(OSError):
            with installation_lock(lock,owner=os.getuid()):
                self.fail('Symlink lock accepted')
        self.assertEqual(sentinel.read_bytes(),b'preserve unrelated file')

    def test_port_preflight_accepts_a_host_with_ipv6_disabled(self):
        listener = MagicMock()
        listener.__enter__.return_value = listener
        def socket_for(family, kind):
            if family == socket.AF_INET6:
                raise OSError(errno.EAFNOSUPPORT,'IPv6 disabled on fixture host')
            return listener
        with patch('installer_host.socket.socket',side_effect=socket_for), patch('installer_host.socket.has_ipv6',True):
            Host().free_port(3000)
        listener.bind.assert_called_once_with(('0.0.0.0',3000))


if __name__ == '__main__':
    unittest.main()
