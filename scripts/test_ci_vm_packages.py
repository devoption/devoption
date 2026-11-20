"""Disposable apt setup refuses implicit use and propagates setup failures."""
import hashlib
import itertools
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('ci_vm_packages.sh')
OS_RELEASE = Path('/etc/os-release')
UBUNTU = OS_RELEASE.exists() and 'VERSION_CODENAME=noble' in OS_RELEASE.read_text()


class VmPackageTests(unittest.TestCase):
    def test_requires_explicit_disposable_opt_in(self):
        env = dict(os.environ)
        env.pop('DEVOPTION_DISPOSABLE_VM_PACKAGES', None)
        result = subprocess.run(['bash', str(SCRIPT), 'vm-tools'], env=env, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'explicit disposable-host opt-in', result.stderr)

    @unittest.skipUnless(UBUNTU, 'Ubuntu 24.04 fixture command wiring')
    def test_signed_sources_bounded_phases_and_failed_update_stops_install(self):
        for mode, failure in itertools.product(('vm-tools', 'installer-guest', 'installer-guest-download'), ('', 'update', 'install')):
            with self.subTest(mode=mode, failure=failure), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                log = root / 'commands.jsonl'
                (root / 'id').write_text('#!/bin/sh\necho 0\n')
                (root / 'id').chmod(0o755)
                # Never invoke apt/root/network commands: capture the timeout
                # invocation and the exact temporary source list it would use.
                (root / 'timeout').write_text('#!' + sys.executable + '\n' + '''import json, os, pathlib, sys
args = sys.argv[1:]
source = next(a.split('=', 1)[1] for a in args if a.startswith('Dir::Etc::sourcelist='))
with open(os.environ['VM_TEST_LOG'], 'a') as stream:
    stream.write(json.dumps(dict(args=args, source=pathlib.Path(source).read_text())) + '\\n')
sys.exit(1 if os.environ['VM_TEST_FAILURE'] in ('update', 'install') and os.environ['VM_TEST_FAILURE'] in args else 0)
''')
                (root / 'timeout').chmod(0o755)
                env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH'],
                           DEVOPTION_DISPOSABLE_VM_PACKAGES='1', VM_TEST_LOG=str(log), VM_TEST_FAILURE=failure)
                command = ['bash', str(SCRIPT), mode]
                if mode == 'installer-guest-download':
                    command.append(str(root / 'packages'))
                result = subprocess.run(command, env=env, capture_output=True)
                self.assertEqual(result.returncode, 1 if failure else 0, result.stderr)
                commands = [json.loads(line) for line in log.read_text().splitlines()]
                self.assertEqual(len(commands), 1 if failure == 'update' else 2)
                update = commands[0]['args']
                self.assertEqual(update[:3], ['--kill-after=10s', '120s', 'apt-get'])
                self.assertIn('APT::Update::Error-Mode=any', update)
                self.assertIn('Dir::Etc::sourceparts=-', update)
                self.assertIn('Acquire::https::Timeout=15', update)
                for command in commands:
                    source = command['source']
                    self.assertNotIn('http://', source)
                    self.assertNotIn('trusted=yes', source)
                    self.assertEqual(source.count('signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg'), 3)
                    filename = next(a.split('=', 1)[1] for a in command['args'] if a.startswith('Dir::Etc::sourcelist='))
                    self.assertFalse(Path(filename).exists())
                if len(commands) == 2:
                    self.assertEqual(commands[1]['args'][:2], ['--kill-after=10s', '150s'])
                    expected = ['qemu-system-x86', 'qemu-utils', 'xorriso'] if mode == 'vm-tools' else ['git', 'sudo', 'build-essential']
                    if mode == 'installer-guest-download':
                        expected += ['systemd', 'systemd-sysv', 'systemd-resolved',
                                     'systemd-timesyncd', 'udev', 'libpam-systemd']
                    args = commands[1]['args']
                    self.assertEqual(args[args.index('--no-install-recommends') + 1:], expected)
                    self.assertIn('--no-remove', args)
                    if mode == 'installer-guest-download':
                        args = commands[1]['args']
                        self.assertIn('--download-only', args)
                        self.assertIn('Dir::Cache::archives=' + str(root / 'packages'), args)
                        status = next(a.split('=', 1)[1] for a in args if a.startswith('Dir::State::status='))
                        self.assertNotEqual(status, '/var/lib/dpkg/status')
                        self.assertFalse(Path(status).exists())

    @unittest.skipUnless(UBUNTU, 'Ubuntu 24.04 fixture command wiring')
    def test_offline_guest_verifies_checksums_and_forbids_downloads(self):
        for corrupt in (False, True):
            with self.subTest(corrupt=corrupt), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                package = root / 'fixture_1_amd64.deb'
                package.write_bytes(b'public synthetic package')
                (root / 'SHA256SUMS').write_text(hashlib.sha256(package.read_bytes()).hexdigest() + '  ' + package.name + '\n')
                if corrupt:
                    package.write_bytes(b'changed')
                (root / 'id').write_text('#!/bin/sh\necho 0\n')
                (root / 'id').chmod(0o755)
                (root / 'timeout').write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$VM_TEST_LOG"\n')
                (root / 'timeout').chmod(0o755)
                log = root / 'commands'
                env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH'],
                           DEVOPTION_DISPOSABLE_VM_PACKAGES='1', VM_TEST_LOG=str(log))
                result = subprocess.run(['bash', str(SCRIPT), 'installer-guest-offline', str(root)], env=env, capture_output=True)
                self.assertEqual(result.returncode, 1 if corrupt else 0, result.stderr)
                if corrupt:
                    self.assertFalse(log.exists())
                else:
                    args = log.read_text().splitlines()
                    self.assertIn('--no-download', args)
                    self.assertIn('--no-remove', args)
                    self.assertIn('Dir::Etc::sourcelist=/dev/null', args)
                    self.assertIn(str(package), args)
                    self.assertIn('Dir::Cache::archives=' + str(root), args)
