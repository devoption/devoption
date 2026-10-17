import json
from pathlib import Path
import tempfile
import unittest
from prepare_platform_service import generate, render


class PlatformServiceTests(unittest.TestCase):
    def setUp(self):
        self.build = dict(uid=1000, owner_data='/var/lib/devoption/data', repositories='/var/lib/devoption/repositories', history='/var/lib/devoption/history', work='/var/lib/devoption/jobs', toolchain='/opt/inputs/rust', vendor='/opt/inputs/vendor')
        self.deployment = {key: self.build[key] for key in ('uid', 'owner_data', 'repositories', 'history')}
        self.deployment.update(build_work=self.build['work'], port=15000, state='/var/lib/devoption-app')
        self.options = dict(binary='/opt/devoption/devoption', account='devoption', hostname='platform.example.com', port=3000)

    def test_enabled_capabilities_and_writable_paths_follow_installed_config(self):
        unit = render(self.build, self.deployment, **self.options)
        self.assertIn('--build-execution enabled --app-deployment enabled', unit)
        self.assertIn('ExecStartPre=/usr/bin/test -s /var/lib/devoption/data/owner.bin', unit)
        self.assertIn('ReadWritePaths=/var/lib/devoption/data /var/lib/devoption/repositories /var/lib/devoption/history /var/lib/devoption/jobs /var/lib/devoption-app\n', unit)
        self.assertIn('ProtectHome=read-only', unit)
        self.assertNotIn('--app-deployment', render(self.build, None, **self.options))

    def test_mismatched_config_and_ingress_rejected(self):
        for key in ('uid', 'owner_data', 'history', 'repositories', 'build_work'):
            with self.subTest(key=key), self.assertRaises(ValueError):
                render(self.build, self.deployment | {key: 'different'}, **self.options)
        for port in (True, 80, 3000, 65536):
            with self.subTest(port=port), self.assertRaises(ValueError):
                render(self.build, self.deployment | {'port': port}, **self.options)
        for state in ('/var/lib', '/tmp/%n', '/opt/devoption'):
            with self.subTest(state=state), self.assertRaises(ValueError):
                render(self.build, self.deployment | {'state': state}, **self.options)
        ingress = dict(platform_hostname='other.example.com', app_hostname='app.example.com', platform_port=3000, project='a'*32)
        with self.assertRaises(ValueError):
            render(self.build, self.deployment | {'ingress': ingress}, **self.options)

    def test_injection_and_overlapping_storage_rejected(self):
        for path in ('/tmp/%n', '/tmp/$VAR', '/tmp/a\nExecStart=/evil', '/tmp/../data', '/tmp//data'):
            with self.subTest(path=path), self.assertRaises(ValueError):
                render(self.build | {'work': path}, None, **self.options)
        with self.assertRaises(ValueError):
            render(self.build | {'work': self.build['history']}, None, **self.options)
        for account in ('root', 'bad\nUser=root', 'user name'):
            with self.assertRaises(ValueError):
                render(self.build, None, **(self.options | {'account': account}))

    def test_staging_never_overwrites_and_invalid_input_creates_nothing(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root/'build.toml'
            source.write_text(''.join(f'{key} = {json.dumps(value)}\n' for key, value in self.build.items()))
            output = root/'review'
            generate(source, None, output, **self.options)
            original = (output/'devoption-public.service').read_bytes()
            with self.assertRaises(FileExistsError):
                generate(source, None, output, **self.options)
            self.assertEqual(original, (output/'devoption-public.service').read_bytes())
            with self.assertRaises(ValueError):
                generate(source, None, root/'invalid', **(self.options | {'account': 'root'}))
            self.assertFalse((root/'invalid').exists())
