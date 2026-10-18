import importlib.util
import json
import os
from http.server import BaseHTTPRequestHandler, HTTPServer
import threading
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('deployment_broker', Path(__file__).with_name('deployment_broker.py'))
broker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(broker)


class DeploymentTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name).resolve()
        self.config = {'state': root/'state', 'build_work': root/'builds', 'uid': os.geteuid(), 'fingerprint': 'f'*64, 'port': 18080}
        for key in ['state', 'build_work']: self.config[key].mkdir(mode=0o700)
        self.job = 'a'*32
        directory = self.config['build_work']/('b'*32)/'artifact'
        directory.mkdir(parents=True, mode=0o700)
        directory.parent.chmod(0o700)
        self.artifact = directory/'component.wasm'
        self.component = b'\0asm\x0d\0\x01\0test'
        self.artifact.write_bytes(self.component); self.artifact.chmod(0o600)
        self.receipt = {'version': 2, 'toolchain_sha256': '3'*64, 'dependency_sha256': '4'*64, 'configuration_sha256': '5'*64, 'build_policy': 'WasmBuildV1', 'target': 'wasm32-wasip2', 'abi': 'wasi:http/proxy@0.2.4', 'job': self.job, 'project': 'c'*32, 'repository': 'd'*32, 'commit': 'e'*40, 'source_sha256': '1'*64, 'review_sha256': '2'*64, 'artifact_sha256': broker.digest(self.component), 'artifact_bytes': len(self.component), 'artifact': 'component.wasm'}
        self.receipt_path = directory/'receipt.toml'
        self.write_receipt()
        # Test fixture runs without root; retain all file-type/link/digest checks.
        original = broker.read_regular
        def read(path, limit, uid=None, dir_fd=None):
            return original(path, limit, os.geteuid() if uid == 0 else uid, dir_fd)
        self.read_patch = patch.object(broker, 'read_regular', side_effect=read)
        self.read_patch.start(); self.addCleanup(self.read_patch.stop)
        self.protected = patch.object(broker, 'protected')
        self.protected.start(); self.addCleanup(self.protected.stop)

    def write_receipt(self):
        self.receipt_path.write_text(''.join(f'{key} = {json.dumps(value)}\n' for key,value in self.receipt.items()))
        self.receipt_path.chmod(0o600)

    def promote(self):
        provenance = tuple(str(self.receipt[key]) for key in ['project','repository','commit','source_sha256','artifact_bytes'])
        return broker.promote(self.config, self.job, broker.digest(self.component), provenance)

    def test_promotion_copies_provenance_is_idempotent_and_survives_reload(self):
        release = self.promote()
        self.assertEqual(self.promote(), release)
        self.assertEqual(broker.load_release(self.config, release)['job'], self.job)
        self.artifact.write_bytes(b'changed original')
        self.assertEqual(broker.load_release(self.config, release)['artifact_sha256'], broker.digest(self.component))
        self.assertEqual((self.config['state']/release/'component.wasm').read_bytes(), self.component)

    def test_old_receipt_and_mismatched_history_provenance_are_rejected(self):
        self.receipt['version'] = 1; self.write_receipt()
        with self.assertRaises(ValueError): self.promote()
        self.receipt['version'] = 2; self.write_receipt()
        with self.assertRaises(ValueError): broker.promote(self.config, self.job, broker.digest(self.component), ('wrong history',))
        self.assertEqual(list(self.config['state'].iterdir()), [])

    def test_changed_deployment_generation_cannot_replace_an_app(self):
        with patch.object(broker,'launch') as launch, patch.object(broker,'stop_unit') as stop:
            with self.assertRaises(ValueError): broker.start(self.config,'0'*64,'ok A',self.config['fingerprint'],'stale')
            launch.assert_not_called(); stop.assert_not_called()

    def test_validation_failure_preserves_current_intent_and_records_result(self):
        release=self.promote()
        previous={'release':'7'*64,'intent':'running','observed':'healthy'}
        broker.save(self.config, previous)
        with patch.object(broker,'launch',side_effect=ValueError('invalid imports')), patch.object(broker,'stop_unit') as stop:
            with self.assertRaises(ValueError): broker.start(self.config, release, 'ok A', self.config['fingerprint'])
            stop.assert_not_called()
        self.assertEqual(broker.desired(self.config),previous)
        self.assertIn('compatibility validation failed',json.loads((self.config['state']/'last-action.json').read_text())['message'])

    def test_changed_component_manifest_and_provenance_fail_closed(self):
        self.artifact.write_bytes(b'not a component')
        with self.assertRaises(ValueError): self.promote()
        self.artifact.write_bytes(self.component)
        self.receipt['artifact_sha256'] = '0'*64; self.write_receipt()
        with self.assertRaises(ValueError): self.promote()
        self.receipt['artifact_sha256'] = broker.digest(self.component); self.write_receipt()
        release = self.promote()
        path = self.config['state']/release/'component.wasm'
        path.chmod(0o600); path.write_bytes(b'changed release')
        with self.assertRaises(ValueError): broker.load_release(self.config, release)

    def test_links_duplicate_receipts_and_unsafe_directories_are_rejected(self):
        original = self.artifact.with_name('original')
        self.artifact.rename(original); self.artifact.symlink_to(original)
        with self.assertRaises(OSError): self.promote()
        self.artifact.unlink(); original.rename(self.artifact)
        duplicate = self.config['build_work']/('3'*32)/'artifact'
        duplicate.mkdir(parents=True,mode=0o700); duplicate.parent.chmod(0o700)
        (duplicate/'receipt.toml').write_bytes(self.receipt_path.read_bytes())
        (duplicate/'receipt.toml').chmod(0o600)
        with self.assertRaises(ValueError): self.promote()

    def test_invalid_ids_and_interrupted_promotion_do_not_overwrite(self):
        with self.assertRaises(ValueError): broker.promote(self.config, '../bad', '0'*64, ())
        release = broker.digest(broker.canonical({**self.receipt, 'policy':broker.POLICY,'runtime':'wasmtime-49.0.3'}))
        pending = self.config['state']/(release+'.pending'); pending.mkdir(mode=0o700)
        (pending/'operator-note').write_text('preserve')
        with self.assertRaises(FileExistsError): self.promote()
        self.assertEqual((pending/'operator-note').read_text(),'preserve')

    def test_stale_configuration_never_starts_or_stops_a_unit(self):
        with patch.object(broker,'launch') as launch, patch.object(broker,'stop_unit') as stop:
            with self.assertRaises(ValueError): broker.start(self.config,'0'*64,'ok A','stale')
            launch.assert_not_called(); stop.assert_not_called()

    def test_failed_health_keeps_stopped_intent_and_previous_release(self):
        release = self.promote()
        broker.save(self.config,{'release':'7'*64,'intent':'running'})
        with patch.object(broker,'launch'), patch.object(broker,'stop_unit'), patch.object(broker,'unit_state',return_value={'ActiveState':'failed'}):
            with self.assertRaises(ValueError): broker.start(self.config, release, 'ok A', self.config['fingerprint'])
        state = broker.desired(self.config)
        self.assertEqual((state['intent'],state['observed'],state['previous']),('stopped','failed','7'*64))

    def test_success_records_invocation_and_only_running_intent_reconciles(self):
        release = self.promote()
        with patch.object(broker,'launch'), patch.object(broker,'stop_unit'), patch.object(broker,'unit_state',return_value={'ActiveState':'active','InvocationID':'verified'}), patch.object(broker,'health',return_value=True):
            broker.start(self.config,release,'ok A',self.config['fingerprint'])
        self.assertEqual(broker.desired(self.config)['invocation'],'verified')
        with patch.object(broker,'start') as start, patch.object(broker,'stop_unit'):
            broker.dispatch(self.config,['reconcile']); start.assert_called_once()
        with patch.object(broker,'stop_unit'):
            broker.dispatch(self.config,['stop'])
        with patch.object(broker,'start') as start, patch.object(broker,'stop_unit'):
            broker.dispatch(self.config,['reconcile']); start.assert_not_called()

    def test_stop_failure_cannot_be_reported_as_stopped(self):
        broker.save(self.config,{'release':'7'*64,'intent':'running','observed':'healthy'})
        with patch.object(broker,'stop_unit',side_effect=ValueError('unknown')):
            with self.assertRaises(ValueError): broker.dispatch(self.config,['stop'])
        self.assertEqual(broker.desired(self.config)['observed'],'stopping')


class IdentityTests(unittest.TestCase):
    def test_real_http_health_requires_nonempty_exact_invocation_and_fresh_unit(self):
        fields = {'header': 'a'*32}
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                if fields['header'] is not None:
                    self.send_header('X-DevOption-Invocation', fields['header'])
                self.end_headers()
                self.wfile.write(b'ok A')
            def log_message(self, *_): pass
        server = HTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        config = {'port': server.server_port}
        with patch.object(broker, 'unit_state', return_value={'InvocationID': 'a'*32}) as state:
            self.assertTrue(broker.health(config, 'ok A', 'a'*32))
            for marker in [None, 'b'*32]:
                fields['header'] = marker
                self.assertFalse(broker.health(config, 'ok A', 'a'*32))
            # Regression: absent HTTP header plus absent unit ID used to compare equal.
            fields['header'] = None
            state.return_value = {}
            self.assertFalse(broker.health(config, 'ok A', None))
            for identity in ['', 'invalid', 'a'*31, 42]:
                self.assertFalse(broker.health(config, 'ok A', identity))
            fields['header'] = 'a'*32
            state.return_value = {'InvocationID': 'b'*32}
            self.assertFalse(broker.health(config, 'ok A', 'a'*32))

    def test_unconfirmed_stop_blocks_reset_and_replacement_launch(self):
        for state in [{'ActiveState': 'active', 'ControlGroup': ''},
                      {'ActiveState': 'inactive', 'ControlGroup': '/system.slice/devoption-app.service'},
                      {}]:
            with self.subTest(state=state), patch.object(broker, 'command') as command, patch.object(broker, 'unit_state', return_value=state):
                with self.assertRaisesRegex(ValueError, 'stop is unconfirmed'):
                    broker.stop_unit()
                self.assertEqual(command.call_count, 1)
                self.assertEqual(command.call_args.args[0][1], 'stop')
                with patch.object(broker, 'load_release'):
                    with self.assertRaisesRegex(ValueError, 'stop is unconfirmed'):
                        broker.launch({'state': Path('/unused')}, 'a'*64)
                self.assertFalse(any(call.args[0][0] == '/usr/bin/systemd-run' for call in command.call_args_list))


if __name__ == '__main__': unittest.main()
