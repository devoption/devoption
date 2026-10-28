"""Project-slot isolation, strict configuration and independent recovery."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import test_deployment_broker as fixtures
broker = fixtures.broker
from configure_app_hostname import generate_apps, render_apps
from prepare_platform_service import render as render_service
import test_prepare_platform_service as service_fixtures


def configuration():
    return dict(uid=1000, build_work='/builds', state='/state', worker='/worker',
                owner_data='/owner', history='/history', repositories='/repositories',
                apps=[dict(project=project, port=15000+i,
                           ingress=dict(project=project, app_hostname=f'app{i}.example.test',
                                        platform_hostname='platform.example.test', platform_port=3000))
                      for i, project in enumerate(('a'*32, 'b'*32))])


class ConfigurationTests(unittest.TestCase):
    def test_duplicate_names_ports_projects_and_ambiguous_bindings_fail_closed(self):
        for field in ('port', 'project', 'app_hostname', 'platform_hostname', 'platform_port'):
            config = configuration()
            if field in ('port', 'project'):
                config['apps'][1][field] = config['apps'][0][field]
            elif field == 'app_hostname':
                config['apps'][1]['ingress'][field] = config['apps'][0]['ingress'][field]
            else:
                config['apps'][1]['ingress'][field] = 'other.example.test' if field.endswith('hostname') else 3001
            with self.subTest(field=field), self.assertRaises(ValueError):
                broker.deployment_slots(config)
        for change in ({'port': 3000}, {'port': True}, {'project': '../other'}, {'unit': 'foreign.service'}):
            config = configuration(); config['apps'][0].update(change)
            with self.assertRaises(ValueError): broker.deployment_slots(config)
        for count in (0, 9):
            config = configuration(); config['apps'] = config['apps'][:1] * count
            with self.assertRaises(ValueError): broker.deployment_slots(config)
        config = configuration(); config['port'] = 15002
        with self.assertRaises(ValueError): broker.deployment_slots(config)

    def test_sibling_changes_do_not_change_selected_slot_configuration(self):
        before = broker.deployment_slots(configuration())
        config = configuration(); config['apps'][1]['port'] = 15100
        after = broker.deployment_slots(config)
        self.assertEqual(before[0], after[0])
        self.assertNotEqual(before[1], after[1])
        self.assertNotEqual(before[0]['state'], before[1]['state'])

    def test_generated_proxy_preserves_protected_platform_and_every_app_boundary(self):
        config = configuration()
        policy = dict(client_ca_file='/ca.pem', allowed_clients_file='/allowed.pem')
        proxy = render_apps(config, policy)
        self.assertEqual(proxy.count('mode require_and_verify'), 1)
        for app in config['apps']:
            self.assertIn(app['ingress']['app_hostname']+' {', proxy)
            self.assertIn(f"reverse_proxy 127.0.0.1:{app['port']}", proxy)
        self.assertEqual(proxy.count('header_up -Cookie'), 2)
        self.assertEqual(proxy.count('respond @control 404'), 2)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); source = root/'deployment.toml'
            raw = ''.join(f'{k}={json.dumps(v)}\n' for k, v in config.items() if k != 'apps')
            for app in config['apps']:
                raw += f'\n[[apps]]\nproject="{app["project"]}"\nport={app["port"]}\n[apps.ingress]\n'
                raw += ''.join(f'{k}={json.dumps(v)}\n' for k, v in app['ingress'].items())
            source.write_text(raw)
            generate_apps(source, root/'review', 'platform.example.test', 3000, policy)
            self.assertEqual((root/'review/deployment.toml').read_text(), raw)
            self.assertEqual((root/'review/Caddyfile').read_text(), proxy)
            with self.assertRaises(FileExistsError):
                generate_apps(source, root/'review', 'platform.example.test', 3000, policy)

    def test_platform_service_checks_every_slot_origin_and_listener(self):
        fixture = service_fixtures.PlatformServiceTests(); fixture.setUp()
        config = fixture.deployment.copy(); config.pop('port')
        config['apps'] = configuration()['apps']
        options = fixture.options | {'hostname': 'platform.example.test'}
        self.assertIn('--app-deployment enabled', render_service(fixture.build, config, **options))
        config['apps'][1]['port'] = 3000
        with self.assertRaises(ValueError): render_service(fixture.build, config, **options)


class IsolationTests(unittest.TestCase):
    setUp = fixtures.DeploymentTests.setUp
    write_receipt = fixtures.DeploymentTests.write_receipt
    promote = fixtures.DeploymentTests.promote

    def test_installed_slots_have_independent_fingerprints_units_and_grants(self):
        root = self.config['state']
        config = configuration()
        config.update(uid=os.geteuid() or 1000, state=str(root))
        for name in ('build_work', 'owner_data', 'history', 'repositories'):
            path = root.parent/name
            path.mkdir(mode=0o700, exist_ok=True)
            config[name] = str(path)
        for app in config['apps']: (root/app['project']).mkdir(mode=0o700)
        worker, executable, source = root.parent/'worker', root.parent/'broker', root.parent/'config.toml'
        worker.write_bytes(b'worker'); executable.write_bytes(b'broker')
        config['worker'] = str(worker)
        def write_config():
            raw = ''.join(f'{k}={json.dumps(v)}\n' for k, v in config.items() if k != 'apps')
            for app in config['apps']:
                raw += f'\n[[apps]]\nproject="{app["project"]}"\nport={app["port"]}\n[apps.ingress]\n'
                raw += ''.join(f'{k}={json.dumps(v)}\n' for k, v in app['ingress'].items())
            source.write_text(raw)
        write_config()
        with patch.object(broker, 'CONFIG', source), patch.object(broker, 'BROKER', executable), patch.dict(os.environ, SUDO_UID=str(config['uid'])):
            before = broker.configuration()['apps']
            self.assertNotEqual(before[0]['unit'], before[1]['unit'])
            self.assertNotEqual(before[0]['runtime_directory'], before[1]['runtime_directory'])
            config['apps'][1]['port'] = 15100; write_config()
            after = broker.configuration()['apps']
            self.assertEqual(before[0]['fingerprint'], after[0]['fingerprint'])
            self.assertNotEqual(before[1]['fingerprint'], after[1]['fingerprint'])
            config['worker'] = str(root/'worker'); write_config()
            with self.assertRaisesRegex(ValueError, 'separate from platform'): broker.configuration()
            config['worker'] = str(worker); write_config()
            # A legacy record cannot be silently abandoned when changing modes.
            (root/'desired.json').write_text('{}')
            with self.assertRaisesRegex(ValueError, 'explicit migration'): broker.configuration()

    def slots(self):
        slots = []
        for i, project in enumerate(('c'*32, 'e'*32)):
            slot = self.config | dict(project=project, unit='devoption-app-'+project+'.service',
                                     validate_unit='devoption-app-'+project+'-validate.service',
                                     runtime_directory='devoption-app-'+project, port=18080+i,
                                     state=self.config['state']/project)
            slot['state'].mkdir(mode=0o700)
            slots.append(slot)
        return slots

    def test_two_projects_update_rollback_stop_and_recover_independently(self):
        a, b = self.slots(); root = {'apps': [a, b]}
        provenance = lambda: tuple(str(self.receipt[k]) for k in ('project', 'repository', 'commit', 'source_sha256', 'artifact_bytes'))
        release_a = broker.promote(a, self.job, broker.digest(self.component), provenance())
        self.receipt['project'] = b['project']; self.write_receipt()
        release_b = broker.promote(b, self.job, broker.digest(self.component), provenance())
        observed = {'ActiveState': 'active', 'InvocationID': 'a'*32}
        with patch.object(broker, 'launch'), patch.object(broker, 'stop_unit') as stop, patch.object(broker, 'unit_state', return_value=observed), patch.object(broker, 'health', return_value=True):
            broker.dispatch(root, ['project', a['project'], 'start', release_a, 'ok A', a['fingerprint'], broker.digest(b'null')])
            broker.dispatch(root, ['project', b['project'], 'start', release_b, 'ok A', b['fingerprint'], broker.digest(b'null')])
            before = (b['state']/'desired.json').read_bytes()
            # A same-project update followed by saved-release rollback.
            self.receipt.update(project=a['project'], commit='f'*40); self.write_receipt()
            newer = broker.promote(a, self.job, broker.digest(self.component), provenance())
            for release in (newer, release_a):
                broker.start(a, release, 'ok A', a['fingerprint'])
                self.assertEqual((b['state']/'desired.json').read_bytes(), before)
            stop.reset_mock()
            broker.dispatch(root, ['project', a['project'], 'stop'])
            stop.assert_called_once_with(a['unit'])
            self.assertEqual((b['state']/'desired.json').read_bytes(), before)
            with patch.object(broker, 'start', wraps=broker.start) as start:
                broker.dispatch(root, ['project', a['project'], 'reconcile'])
                broker.dispatch(root, ['project', b['project'], 'reconcile'])
                self.assertEqual(start.call_count, 1)
                self.assertEqual(start.call_args.args[0]['project'], b['project'])
            self.assertEqual(broker.desired(a)['intent'], 'stopped')
            self.assertEqual(broker.desired(b)['intent'], 'running')
            # Failure of A recovery cannot alter B intent or saved bytes.
            broker.start(a, release_a, 'ok A', a['fingerprint'])
            before = (b['state']/'desired.json').read_bytes()
            with patch.object(broker, 'start', side_effect=ValueError('health failed')):
                broker.dispatch(root, ['project', a['project'], 'reconcile'])
            self.assertEqual(broker.desired(a)['observed'], 'recovery failed')
            self.assertEqual((b['state']/'desired.json').read_bytes(), before)

    def test_unscoped_unknown_and_cross_project_operations_never_launch(self):
        a, b = self.slots(); root = {'apps': [a, b]}
        release = broker.promote(a, self.job, broker.digest(self.component), tuple(str(self.receipt[k]) for k in ('project', 'repository', 'commit', 'source_sha256', 'artifact_bytes')))
        with patch.object(broker, 'launch') as launch, patch.object(broker, 'stop_unit') as stop:
            for args in (['stop'], ['reconcile'], ['project', 'f'*32, 'stop'], ['project', b['project'], 'start', release, 'ok A', b['fingerprint'], broker.digest(b'null')]):
                with self.subTest(args=args), self.assertRaises((ValueError, OSError)):
                    broker.dispatch(root, args)
            launch.assert_not_called(); stop.assert_not_called()
        self.assertTrue(broker.dispatch(root, ['project', 'f'*32, 'status'])['unconfigured'])
        with self.assertRaises(ValueError):
            broker.promote(b, self.job, broker.digest(self.component), tuple(str(self.receipt[k]) for k in ('project', 'repository', 'commit', 'source_sha256', 'artifact_bytes')))


if __name__ == '__main__': unittest.main()
