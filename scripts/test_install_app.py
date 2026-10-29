"""Unified installation fault/preservation tests using isolated files, no services."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch

from install_app import AppInstaller, SUDOERS, input_tree, load_config, sha256
from install_bundle import STATE
from test_install_bundle import FixtureHost


class AppFixtureHost(FixtureHost):
    def app_preflight(self, resumed):
        if not resumed:
            self.free_port(15000)

    def validate_app(self, binary, toolchain, vendor):
        self.calls.append(('validate-app',))


class AppInstallTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.image = self.root / 'image'
        self.image.mkdir(mode=0o700)
        for name in ('opt', 'var/lib', 'etc/systemd/system', 'usr/local'):
            (self.image / name).mkdir(parents=True)
        self.inputs = self.root / 'inputs'
        self.inputs.mkdir()
        # Temporary roots have OS-owned ancestors. Model that host-only boundary;
        # the actual recursive input inventory and content validation still run.
        ancestor_check = patch('install_app.protected')
        ancestor_check.start()
        self.addCleanup(ancestor_check.stop)
        for name in ('toolchain', 'vendor', 'archives'):
            (self.inputs / name).mkdir()
        (self.inputs / 'toolchain/lib/rustlib/wasm32-wasip2/lib').mkdir(parents=True)
        (self.inputs / 'toolchain/lib/rustlib/wasm32-wasip2/lib/std.rlib').write_bytes(b'not executed fixture')
        (self.inputs / 'toolchain/bin').mkdir()
        for name in ('rustc', 'cargo'):
            path = self.inputs / 'toolchain/bin' / name
            path.write_bytes(b'not executed fixture')
            path.chmod(0o755)
        for name in ('binary', 'runtime', 'caddy_archive'):
            (self.inputs / name).write_bytes(b'not executed ' + name.encode())
        (self.inputs / 'lockfile').write_text('version = 4\npackage = []\n')
        self.config = {'schema_version': 1, 'hostname': 'platform.example.com', 'source_commit': 'a' * 40}
        self.config.update({name: str(self.inputs / name) for name in
                            ('binary', 'runtime', 'caddy_archive', 'toolchain', 'vendor', 'archives', 'lockfile')})
        for name in ('binary', 'runtime'):
            self.config[name + '_sha256'] = sha256((self.inputs / name).read_bytes())
        self.config['lock_sha256'] = sha256((self.inputs / 'lockfile').read_bytes())
        for name in ('toolchain', 'vendor'):
            self.config[name + '_sha256'] = input_tree(self.inputs / name, os.getuid())
        self.host = AppFixtureHost(self.image)
        self.proxy = patch('install_app.proxy_binary', return_value=b'pinned proxy fixture')
        self.proxy.start()
        self.addCleanup(self.proxy.stop)

    def installer(self, config=None):
        return AppInstaller(self.image, self.host, config or self.config)

    def write_config(self, config):
        path = self.root / 'install.toml'
        path.write_text(''.join(f'{key} = {json.dumps(value)}\n' for key, value in config.items()))
        return path

    def inventory(self):
        return {str(p.relative_to(self.image)): (p.lstat().st_mode, p.lstat().st_ino,
                p.read_bytes() if p.is_file() and not p.is_symlink() else None)
                for p in self.image.rglob('*')}

    def test_one_config_plans_without_writes_then_installs_consistent_app_support(self):
        config = load_config(self.write_config(self.config))
        installer = self.installer(config)
        before = self.inventory()
        plan = installer.preflight()
        self.assertEqual(self.inventory(), before)
        self.assertEqual(self.host.calls, [])
        self.assertEqual(plan['next_action'], 'apply')
        result = installer.install()
        self.assertEqual(result['missing_files'], [])
        self.assertEqual(result['next_action'], 'enroll')
        build = tomllib.loads(installer.path('/etc/devoption/build.toml').read_text())
        deployment = tomllib.loads(installer.path('/etc/devoption/deployment.toml').read_text())
        self.assertEqual(build['uid'], os.getuid())
        for key in ('uid', 'owner_data', 'repositories', 'history'):
            self.assertEqual(build[key], deployment[key])
        self.assertEqual(build['work'], deployment['build_work'])
        self.assertEqual(deployment['worker'], installer.runtime)
        unit = installer.path('/etc/systemd/system/devoption-public.service').read_text()
        for argument in ('--build-execution enabled', '--app-deployment enabled', '--managed-repository-root'):
            self.assertIn(argument, unit)
        self.assertFalse(result['app_ingress_configured'])
        self.assertNotIn(('start',), self.host.calls)
        self.assertIn(('validate-app',), self.host.calls)
        self.assertLess(self.host.calls.index(('validate-app',)), self.host.calls.index(('reload',)))
        self.assertEqual(installer.path(SUDOERS).stat().st_mode & 0o777, 0o440)

    def test_identical_retry_preserves_owner_catalog_repositories_builds_releases_and_unrelated_data(self):
        installer = self.installer()
        installer.install()
        installer.enroll()
        for name in ('var/lib/devoption/data/catalog.bin', 'var/lib/devoption/repositories/app/source',
                     'var/lib/devoption/history/build.json', 'var/lib/devoption/jobs/attempt/log',
                     'var/lib/devoption-app/release/component.wasm', 'srv/unrelated/data'):
            path = self.image / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'preserve ' + name.encode())
        before = self.inventory()
        installer.install()
        installer.enroll()
        self.assertEqual(self.inventory(), before)
        self.assertEqual(self.host.calls.count(('enroll',)), 1)

    def test_committed_partial_install_resumes_and_does_not_replace_existing_files(self):
        installer = self.installer()
        write = installer.write
        def interrupt(name, *args):
            write(name, *args)
            if name == '/usr/local/libexec/devoption-build-worker':
                raise RuntimeError('simulated interruption')
        with patch.object(installer, 'write', side_effect=interrupt):
            with self.assertRaisesRegex(RuntimeError, 'interruption'):
                installer.install()
        before = self.inventory()
        result = self.installer().install()
        self.assertTrue(result['resumed'])
        self.assertEqual(result['missing_files'], [])
        after = self.inventory()
        for name, value in before.items():
            self.assertEqual(after[name], value)

    def test_preexisting_installations_and_helpers_are_refused_without_any_changes(self):
        conflicts = ('opt/devoption', 'etc/devoption', 'var/lib/devoption-app',
                     'usr/local/libexec/devoption-build-broker', 'etc/sudoers.d/devoption-app')
        for name in conflicts:
            with self.subTest(name=name):
                path = self.image / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b'existing installation data')
                before = self.inventory()
                with self.assertRaises(ValueError):
                    self.installer().install()
                self.assertEqual(self.inventory(), before)
                self.assertEqual(self.host.calls, [])
                path.unlink()

    def test_different_receipt_and_changed_configuration_cannot_adopt_existing_install(self):
        installer = self.installer()
        installer.install()
        installer.enroll()
        before = self.inventory()
        changed = dict(self.config, hostname='different.example.com')
        with self.assertRaisesRegex(ValueError, 'identity differs'):
            self.installer(changed).install()
        self.assertEqual(self.inventory(), before)
        receipt = installer.path(STATE + '/installation.json')
        receipt.write_text('{"schema_version": 1, "reservation": {}}')
        before = self.inventory()
        with self.assertRaisesRegex(ValueError, 'identity differs'):
            self.installer().install()
        self.assertEqual(self.inventory(), before)

    def test_failed_installed_probe_never_reloads_or_starts_and_matching_retry_can_finish(self):
        installer = self.installer()
        with patch.object(self.host, 'validate_app', side_effect=ValueError('probe failed')):
            with self.assertRaisesRegex(ValueError, 'probe failed'):
                installer.install()
        self.assertNotIn(('reload',), self.host.calls)
        self.assertNotIn(('start',), self.host.calls)
        before = self.inventory()
        self.installer().install()
        self.assertEqual(self.inventory(), before)
        self.assertIn(('reload',), self.host.calls)

    def test_unrelated_shared_directory_files_and_modes_survive_installation(self):
        for name in ('etc/sudoers.d', 'usr/local/libexec'):
            directory = self.image / name
            directory.mkdir(mode=0o755)
            (directory / 'unrelated').write_bytes(b'preserved shared resource')
        before = self.inventory()
        self.installer().install()
        after = self.inventory()
        for name, value in before.items():
            self.assertEqual(after[name], value)

    def test_modified_generated_config_and_symlinks_preserve_all_state(self):
        installer = self.installer()
        installer.install()
        path = installer.path('/etc/devoption/build.toml')
        path.write_bytes(b'existing operator change')
        before = self.inventory()
        with self.assertRaisesRegex(ValueError, 'never replace'):
            self.installer().install()
        self.assertEqual(self.inventory(), before)
        path.unlink()
        sentinel = self.root / 'sentinel'
        sentinel.write_bytes(b'do not change')
        path.symlink_to(sentinel)
        before = self.inventory()
        with self.assertRaises(ValueError):
            self.installer().install()
        self.assertEqual(self.inventory(), before)
        self.assertEqual(sentinel.read_bytes(), b'do not change')

    def test_changed_prepared_bytes_or_extra_input_never_mutate_installation(self):
        installer = self.installer()
        installer.install()
        before = self.inventory()
        (self.inputs / 'toolchain/extra').write_bytes(b'changed input')
        with self.assertRaisesRegex(ValueError, 'tree differs'):
            self.installer().install()
        self.assertEqual(self.inventory(), before)
        (self.inputs / 'toolchain/extra').unlink()
        (self.inputs / 'runtime').write_bytes(b'changed executable')
        with self.assertRaisesRegex(ValueError, 'executable differs'):
            self.installer().install()
        self.assertEqual(self.inventory(), before)

    def test_missing_target_corrupt_dependencies_and_occupied_app_port_fail_before_receipt(self):
        self.host.blocked_port = 15000
        with self.assertRaisesRegex(ValueError, 'occupied'):
            self.installer().install()
        self.assertFalse((self.image / STATE.lstrip('/')).exists())
        self.host.blocked_port = None
        standard = self.inputs / 'toolchain/lib/rustlib/wasm32-wasip2/lib/std.rlib'
        standard.unlink()
        config = dict(self.config, toolchain_sha256=input_tree(self.inputs / 'toolchain', os.getuid()))
        with self.assertRaisesRegex(ValueError, 'standard library'):
            self.installer(config)
        self.assertEqual(self.host.calls, [])
        standard.write_bytes(b'not executed fixture')
        (self.inputs / 'vendor/extra').mkdir()
        config = dict(self.config, vendor_sha256=input_tree(self.inputs / 'vendor', os.getuid()))
        with self.assertRaises((ValueError, FileNotFoundError)):
            self.installer(config)
        self.assertFalse((self.image / STATE.lstrip('/')).exists())

    def test_config_rejects_unknown_fields_bad_hashes_and_reserved_or_linked_inputs(self):
        for config in (dict(self.config, extra=True), dict(self.config, binary_sha256='x'*64),
                       dict(self.config, schema_version=True), dict(self.config, toolchain='/opt/devoption/inputs')):
            with self.subTest(config=config):
                with self.assertRaises((ValueError, FileNotFoundError)):
                    load_config(self.write_config(config))
        linked = self.inputs / 'linked-runtime'
        linked.symlink_to(self.inputs / 'runtime')
        with self.assertRaisesRegex(ValueError, 'canonical'):
            load_config(self.write_config(dict(self.config, runtime=str(linked))))

    def test_tree_identity_binds_file_bytes_paths_and_modes_and_rejects_links(self):
        root = self.inputs / 'vendor'
        source = root / 'file'
        source.write_bytes(b'hello')
        initial = input_tree(root, os.getuid())
        source.chmod(0o744)
        self.assertNotEqual(input_tree(root, os.getuid()), initial)
        source.rename(root / 'renamed')
        self.assertNotEqual(input_tree(root, os.getuid()), initial)
        (root / 'link').symlink_to(root / 'renamed')
        with self.assertRaisesRegex(ValueError, 'links'):
            input_tree(root, os.getuid())

    @unittest.skipUnless(os.environ.get('DEVOPTION_TEST_BINARY'), 'explicit compiled platform needed for Rust/Python input identity check')
    def test_prepared_input_identity_matches_the_real_rust_reader(self):
        root = self.inputs / 'vendor'
        (root / 'empty').write_bytes(b'')
        (root / 'unicode-é').write_bytes(b'UTF-8 filename identity')
        (root / 'nested').mkdir()
        (root / 'nested/file').write_bytes(b'byte identity')
        (root / 'nested/file').chmod(0o744)
        actual = subprocess.check_output([os.environ['DEVOPTION_TEST_BINARY'], 'build-input-digest', str(root)],
                                         text=True, timeout=30).strip()
        self.assertEqual(actual, input_tree(root, os.getuid()))

    @unittest.skipUnless(sys.platform == 'linux', 'real systemd/sudo parsers run in Linux CI')
    def test_generated_service_and_sudo_rules_pass_real_linux_parsers(self):
        installer = self.installer()
        installer.install()
        unit = installer.path('/etc/systemd/system/devoption-public.service').read_text()
        import pwd
        import grp
        unit = unit.replace('User=devoption', 'User=' + pwd.getpwuid(os.getuid()).pw_name)
        unit = unit.replace('Group=devoption', 'Group=' + grp.getgrgid(os.getgid()).gr_name)
        unit = unit.replace(installer.binary, '/usr/bin/true')
        unit = unit.replace('/var/lib/devoption', str(self.image / 'var/lib/devoption'))
        staged = self.root / 'devoption-public.service'
        staged.write_text(unit)
        subprocess.run(['systemd-analyze', '--man=no', '--recursive-errors=no', 'verify', str(staged)],
                       check=True, capture_output=True, timeout=30)
        subprocess.run([shutil.which('visudo') or '/usr/sbin/visudo', '-c', '-f', str(installer.path(SUDOERS))],
                       check=True, capture_output=True, timeout=30)


if __name__ == '__main__':
    unittest.main()
