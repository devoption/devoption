"""Upgrade transaction fault injection in disposable paths; signature policy is separate."""
from copy import deepcopy
import json
import os
from pathlib import Path
import socket
import ssl
import sys
import unittest
from unittest.mock import patch

from install_bundle import Installer
from installer_host import Host
from release_bundle import canonical
from release_metadata import validate_metadata, render_metadata
import test_install_bundle as fixtures
from upgrade_bundle import Upgrade, UpgradeHost, UNIT, RECEIPT
from test_release_plan import METADATA
from upgrade_notice import NOTICE


class UpgradeTests(unittest.TestCase):
    fixture_metadata = METADATA
    repack = fixtures.InstallerTests.repack
    response = fixtures.InstallerTests.response
    commit = fixtures.InstallerTests.commit
    rezip = fixtures.InstallerTests.rezip
    build = fixtures.InstallerTests.build

    def setUp(self):
        fixtures.InstallerTests.setUp(self)
        self.host.stop = lambda: setattr(self.host, 'running', False)
        self.host.probe = lambda binary, version: self.assertTrue(self.installer.path(binary).is_file())
        self.host.public_health = lambda hostname: None
        self.host.enable = lambda: None
        self.host.no_app_services = lambda: None
        self.installer.install()
        self.installer.enroll()
        self.installer.start()
        self.old_owner = (self.image/'var/lib/devoption/data/owner.bin').read_bytes()
        self.saved = self.image/'var/lib/devoption/repositories/saved.txt'
        self.saved.write_bytes(b'original checkout bytes')
        manifest = deepcopy(self.manifest)
        manifest['platform_version'] = '0.1.0-preview.2'
        metadata = manifest['release_metadata']
        metadata['schema_version'] = 2
        metadata['version'] = '0.1.0-preview.2'
        metadata['compatibility'].update(installation='fresh-and-upgrade', upgrade_from=['0.1.0-preview.1'],
                                         data_schema=1, configuration_schema=1)
        record = deepcopy(self.record)
        record['candidate']['platform_version'] = manifest['platform_version']
        self.candidate = Installer(self.image, self.host, record, manifest, self.content,
                                   self.installer.output[self.installer.caddy][0], self.installer.hostname)
        self.upgrade = Upgrade(self.installer, self.candidate)

    def test_success_retry_and_explicit_full_state_recovery(self):
        result = self.upgrade.apply()
        self.assertEqual(result['phase'], 'healthy')
        self.assertEqual(result['installed_version'], '0.1.0-preview.2')
        self.assertEqual(self.saved.read_bytes(), b'original checkout bytes')
        self.assertEqual(self.upgrade.apply()['phase'], 'healthy')
        self.saved.write_bytes(b'post-upgrade writes')
        with self.assertRaisesRegex(ValueError, 'discards post-backup'):
            self.upgrade.recover()
        result = self.upgrade.recover(confirm=True)
        self.assertEqual(result['phase'], 'restored')
        self.assertEqual(result['installed_version'], '0.1.0-preview.1')
        self.assertEqual(self.saved.read_bytes(), b'original checkout bytes')
        self.assertEqual((self.image/'var/lib/devoption/data/owner.bin').read_bytes(), self.old_owner)
        displaced = list(self.upgrade.directory.glob('restore-*/displaced-platform/repositories/saved.txt'))
        self.assertEqual(len(displaced), 1)
        self.assertEqual(displaced[0].read_bytes(), b'post-upgrade writes')
        self.saved.write_bytes(b'new writes after recovery')
        self.assertEqual(self.upgrade.recover(confirm=True)['phase'], 'restored')
        self.assertEqual(self.saved.read_bytes(), b'new writes after recovery')
        with self.assertRaisesRegex(ValueError, 'new --attempt'):
            self.upgrade.apply()
        self.assertEqual(Upgrade(self.installer, self.candidate, attempt=2).apply()['phase'], 'healthy')

    def test_owner_notice_follows_verified_plan_upgrade_and_full_state_recovery(self):
        self.upgrade.notify = True
        path = self.installer.path(NOTICE)
        self.assertFalse(path.exists())
        self.upgrade.preflight(starting=True)
        notice = json.loads(path.read_bytes())
        self.assertEqual(notice['phase'], 'not-started')
        self.assertEqual(notice['candidate_archive_sha256'], self.candidate.record['candidate']['archive_sha256'])
        self.assertEqual(notice['candidate_source_commit'], self.candidate.record['candidate']['source_commit'])
        self.assertEqual(notice['transaction'], self.upgrade.id)
        self.assertFalse(notice['publication_eligible'])
        self.assertEqual(path.stat().st_mode & 0o777, 0o644)
        self.assertEqual(path.parent.stat().st_mode & 0o777, 0o755)
        for mode, phase in [(self.upgrade.apply, 'healthy'),
                            (lambda: self.upgrade.recover(confirm=True), 'restored')]:
            mode()
            self.assertEqual(json.loads(path.read_bytes())['phase'], phase)
        self.assertEqual(self.saved.read_bytes(), b'original checkout bytes')
        # Public observations contain no owner state, hostnames, inputs or commands.
        self.assertEqual(set(json.loads(path.read_bytes())), {'schema_version', 'previous_version',
            'candidate_version', 'candidate_archive_sha256', 'candidate_source_commit', 'transaction',
            'attempt', 'phase', 'checked_at', 'publication_eligible'})

    def test_notice_stop_refusal_failure_interruption_and_unsafe_destination(self):
        self.upgrade.notify = True
        path = self.installer.path(NOTICE)
        self.upgrade.preflight(starting=True)
        before = path.read_bytes()
        path.chmod(0o666)
        with self.assertRaisesRegex(ValueError, 'Unsafe upgrade notice'):
            self.upgrade.apply()
        self.assertFalse(self.upgrade.directory.exists())
        self.assertTrue(self.host.running)
        path.chmod(0o644)
        path.unlink()
        path.symlink_to(self.saved)
        with self.assertRaises(ValueError):
            self.upgrade.apply()
        self.assertEqual(self.saved.read_bytes(), b'original checkout bytes')
        path.unlink()
        path.write_bytes(before); path.chmod(0o644)
        with patch.object(self.host, 'probe', side_effect=ValueError('controlled startup failure')):
            with self.assertRaisesRegex(ValueError, 'startup failure'):
                self.upgrade.apply()
        self.assertEqual(json.loads(path.read_bytes())['phase'], 'failed')
        self.upgrade.recover(confirm=True)
        self.assertEqual(json.loads(path.read_bytes())['phase'], 'restored')

    def test_startup_failure_keeps_backup_and_explicit_recovery_restores_previous(self):
        def health(binary, version):
            if version == self.candidate.version:
                self.saved.write_bytes(b'candidate changed data')
                raise ValueError('injected startup failure')
        self.host.probe = health
        with self.assertRaisesRegex(ValueError, 'startup failure'):
            self.upgrade.apply()
        self.assertFalse(self.host.running)
        self.assertEqual(self.upgrade.record()['phase'], 'failed')
        self.upgrade.recover(confirm=True)
        self.assertTrue(self.host.running)
        self.assertEqual(self.saved.read_bytes(), b'original checkout bytes')

    def test_matching_http_responses_cannot_hide_a_service_stopping(self):
        def unhealthy_after_response(hostname):
            if self.installer.path(RECEIPT).read_bytes()==canonical(self.candidate.identity):
                self.host.running=False
        self.host.public_health=unhealthy_after_response
        with self.assertRaisesRegex(ValueError,'stopped during readiness'):
            self.upgrade.apply()
        self.assertEqual(self.upgrade.record()['phase'],'failed')
        self.upgrade.recover(confirm=True)

    def test_power_interruption_after_unit_before_receipt_can_recover(self):
        original = self.upgrade.replace
        def interrupt(path, content, mode, allowed=None):
            original(path, content, mode, allowed)
            if path == self.installer.path(UNIT):
                raise SystemExit('injected process interruption')
        with patch.object(self.upgrade, 'replace', side_effect=interrupt), self.assertRaises(SystemExit):
            self.upgrade.apply()
        self.assertEqual(self.upgrade.record()['phase'], 'backed-up')
        self.assertEqual(self.installer.path(UNIT).read_bytes(), self.candidate.output[UNIT][0])
        self.assertEqual(self.installer.path(RECEIPT).read_bytes(), canonical(self.installer.identity))
        self.assertEqual(self.upgrade.recover(confirm=True)['phase'], 'restored')

    def test_interrupted_staging_and_unsealed_backup_preserve_original_data(self):
        original = self.candidate.write
        def interrupt(*args):
            original(*args)
            raise SystemExit('injected payload interruption')
        with patch.object(self.candidate, 'write', side_effect=interrupt), self.assertRaises(SystemExit):
            self.upgrade.apply()
        with self.assertRaisesRegex(ValueError, 'already exists'):
            self.upgrade.apply()
        self.upgrade.recover(confirm=True)
        self.assertEqual(self.saved.read_bytes(), b'original checkout bytes')

    def test_interrupted_unsealed_backup_does_not_require_restoring_unchanged_live_data(self):
        with patch('upgrade_bundle.copy_records', side_effect=SystemExit('during backup')), self.assertRaises(SystemExit):
            self.upgrade.apply()
        self.assertEqual(self.upgrade.record()['phase'], 'prepared')
        self.upgrade.recover(confirm=True)
        self.assertEqual(self.saved.read_bytes(), b'original checkout bytes')

    def test_interrupted_restore_between_directory_moves_is_repeatable(self):
        self.upgrade.apply()
        original = os.rename
        calls = []
        def interrupt(source, target):
            original(source, target)
            calls.append(1)
            if len(calls) == 1:
                raise SystemExit('after live state displacement')
        with patch('upgrade_bundle.os.rename', side_effect=interrupt), self.assertRaises(SystemExit):
            self.upgrade.recover(confirm=True)
        self.assertFalse(self.installer.path('/var/lib/devoption').exists())
        self.assertEqual(self.upgrade.recover(confirm=True)['phase'], 'restored')
        self.assertEqual(self.saved.read_bytes(), b'original checkout bytes')

    def test_corrupt_backup_cannot_replace_live_state(self):
        self.upgrade.apply()
        backup = self.upgrade.backup/'platform/repositories/saved.txt'
        backup.write_bytes(b'corrupt')
        self.saved.write_bytes(b'preserve current live bytes')
        with self.assertRaisesRegex(ValueError, 'Backup contents'):
            self.upgrade.recover(confirm=True)
        self.assertEqual(self.saved.read_bytes(), b'preserve current live bytes')

    def test_restore_damaged_owner_and_live_data_permissions_from_verified_backup(self):
        self.upgrade.apply()
        owner=self.image/'var/lib/devoption/data/owner.bin'
        owner.write_bytes(b'')
        owner.chmod(0o644)
        (self.image/'var/lib/devoption/data').chmod(0o755)
        self.upgrade.phase('failed')
        self.assertEqual(self.upgrade.preflight()['phase'],'failed')
        self.upgrade.recover(confirm=True)
        self.assertEqual(owner.read_bytes(),self.old_owner)
        self.assertEqual(owner.stat().st_mode&0o777,0o600)
        self.assertEqual(owner.parent.stat().st_mode&0o777,0o700)

    def test_foreign_files_units_receipts_and_symlinks_stop_before_backup(self):
        for path, data in [(self.installer.path(UNIT), b'foreign unit'),
                           (self.installer.path(RECEIPT), b'{}')]:
            original = path.read_bytes()
            path.write_bytes(data)
            with self.assertRaises(ValueError):
                self.upgrade.apply()
            path.write_bytes(original)
            self.assertFalse(self.upgrade.directory.exists())
        self.saved.unlink()
        self.saved.symlink_to(self.installer.path(RECEIPT))
        with self.assertRaises(ValueError):
            self.upgrade.apply()
        self.assertFalse(self.upgrade.directory.exists())

    def test_other_incomplete_attempt_and_app_opt_ins_cannot_be_bypassed(self):
        self.upgrade.initialize()
        with self.assertRaisesRegex(ValueError, 'Another upgrade'):
            Upgrade(self.installer, self.candidate, attempt=2).apply()
        self.upgrade.recover(confirm=True)
        app_unit = self.installer.path('/etc/systemd/system/devoption-app.service')
        app_unit.write_text('app fixture')
        with self.assertRaisesRegex(ValueError, 'App/build opt-ins'):
            Upgrade(self.installer, self.candidate, attempt=3).apply()

    def test_proxy_schema_and_unsupported_version_paths_are_rejected(self):
        for change in ('path', 'schema', 'proxy'):
            candidate = deepcopy(self.candidate)
            candidate.host = self.host
            if change == 'path':
                candidate.manifest['release_metadata']['compatibility']['upgrade_from'] = ['0.0.1-preview.1']
            elif change == 'schema':
                candidate.manifest['release_metadata']['compatibility']['data_schema'] = 2
            else:
                candidate.output[candidate.caddy] = (b'other proxy', 0o755)
            with self.assertRaises(ValueError):
                Upgrade(self.installer, candidate)

    def test_metadata_schema_two_paths_render_honest_notes(self):
        metadata = self.candidate.manifest['release_metadata']
        validate_metadata(metadata, self.candidate.version)
        rendered = render_metadata(metadata, '0'*64)
        self.assertIn('same-schema upgrade from `0.1.0-preview.1`', rendered)
        self.assertIn('Binary rollback: unsupported', rendered)
        self.assertIn('Migration: none implemented', rendered)
        later=deepcopy(metadata)
        later['version']='0.1.0-preview.3'
        validate_metadata(later,'0.1.0-preview.3')  # Explicitly skip an unpublished failed candidate.
        for key, value in [('upgrade_from', ['0.1.0-preview.1','0.1.0-preview.2']),
                           ('data_schema', True), ('configuration_schema', 2)]:
            bad = deepcopy(metadata)
            bad['compatibility'][key] = value
            with self.assertRaises(ValueError):
                validate_metadata(bad, self.candidate.version)

    def test_plan_disk_failure_and_stop_refusal_never_stage_candidate(self):
        self.assertEqual(self.upgrade.preflight(starting=True)['phase'], 'not-started')
        self.assertFalse(self.upgrade.directory.exists())
        with patch.object(self.host, 'preflight', side_effect=ValueError('insufficient disk')), self.assertRaisesRegex(ValueError, 'disk'):
            self.upgrade.apply()
        self.assertFalse(self.upgrade.directory.exists())
        with patch.object(self.host, 'stop', side_effect=ValueError('stop refused')), patch.object(self.upgrade, 'capture') as backup, self.assertRaisesRegex(ValueError, 'stop refused'):
            self.upgrade.apply()
        backup.assert_not_called()
        self.assertFalse(self.candidate.path(self.candidate.release).exists())
        self.assertEqual(self.saved.read_bytes(), b'original checkout bytes')
        self.upgrade.recover(confirm=True)

    def test_corrupt_config_backup_and_private_journal_cannot_be_used(self):
        self.upgrade.apply()
        config=self.upgrade.backup/'configuration/Caddyfile'
        config.write_bytes(b'changed backup')
        with self.assertRaisesRegex(ValueError, 'Backup configuration'):
            self.upgrade.recover(confirm=True)
        self.assertEqual(self.saved.read_bytes(), b'original checkout bytes')
        self.upgrade.journal.chmod(0o644)
        with self.assertRaisesRegex(ValueError, 'Unsafe upgrade record'):
            self.upgrade.preflight()

    def test_cli_authenticates_both_packets_before_any_host_mutation(self):
        import contextlib
        import io
        from upgrade_bundle import main
        args=['upgrade_bundle.py','apply','--previous-reservation','/prior.json','--candidate-reservation','/next.json',
              '--previous-packet','/prior.zip','--candidate-packet','/next.zip','--gh','/trusted/gh',
              '--caddy-archive','/caddy.tar.gz','--hostname',self.installer.hostname,'--allow-validation-candidate']
        with patch('sys.argv',args), patch('upgrade_bundle.UpgradeHost') as host, patch('upgrade_bundle.snapshot',return_value=canonical(self.record)), \
                patch('upgrade_bundle.verified_files',side_effect=[(self.manifest,self.content),ValueError('invalid signature')]), \
                patch('upgrade_bundle.proxy_binary',return_value=b'fixture'), contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            main()
        host.return_value.preflight.assert_not_called()
        host.return_value.stop.assert_not_called()
        self.assertFalse(self.upgrade.directory.exists())


class HostTests(unittest.TestCase):
    @unittest.skipUnless(sys.platform == 'linux', 'Linux service rebinding semantics')
    def test_stopped_service_time_wait_does_not_mask_a_real_listener(self):
        listener = socket.socket()
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(('127.0.0.1', 0)); listener.listen()
        port = listener.getsockname()[1]
        with self.assertRaises(ValueError):
            Host().free_port(port)
        client = socket.create_connection(('127.0.0.1', port))
        accepted, _ = listener.accept()
        accepted.close(); client.close(); listener.close()
        Host().free_port(port)

    def test_unconfirmed_shutdown_is_an_error(self):
        host = UpgradeHost()
        from subprocess import CompletedProcess
        with patch.object(host, 'run', side_effect=[CompletedProcess([],0),CompletedProcess([],0,stdout=b'ActiveState=inactive\nControlGroup=/foreign\n')]), self.assertRaisesRegex(ValueError,'unconfirmed'):
            host.stop()

    def test_loaded_or_transient_workers_are_rejected_even_without_etc_unit_files(self):
        from subprocess import CompletedProcess
        with patch('upgrade_bundle.subprocess.run', return_value=CompletedProcess([],0,stdout=b'loaded\n')), self.assertRaisesRegex(ValueError,'coordinated upgrade'):
            UpgradeHost().no_app_services()

    def test_public_readiness_waits_for_verified_proxy_startup(self):
        from unittest.mock import MagicMock
        signin=MagicMock()
        signin.getresponse.return_value.status=200
        projects=MagicMock()
        projects.getresponse.return_value.status=303
        projects.getresponse.return_value.getheader.return_value='/sign-in'
        with patch('upgrade_bundle.http.client.HTTPSConnection',side_effect=[OSError('warming'),signin,projects]) as connection, patch('upgrade_bundle.time.sleep'):
            UpgradeHost().public_health('platform.example.com')
        self.assertEqual(connection.call_count,3)
        self.assertTrue(connection.call_args.kwargs['context'].check_hostname)
        signin.close.assert_called_once()
        projects.close.assert_called_once()

    def test_failed_tls_verification_cannot_pass_readiness(self):
        with patch('upgrade_bundle.http.client.HTTPSConnection',side_effect=ssl.SSLCertVerificationError('untrusted')), \
                patch('upgrade_bundle.time.monotonic',side_effect=[0,1,61]),patch('upgrade_bundle.time.sleep'),self.assertRaisesRegex(ValueError,'deadline exceeded'):
            UpgradeHost().public_health('platform.example.com')
