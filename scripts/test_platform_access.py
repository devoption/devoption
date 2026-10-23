"""Review-only proxy generation must preserve route separation and reject injection."""
from pathlib import Path
import tempfile
import subprocess
import sys
import unittest

from configure_app_hostname import render
from configure_app_hostname import generate_platform as generate, load_policy, render_platform, validate_policy

POLICY = {'client_ca_file': '/etc/caddy/access/client-ca.pem',
          'allowed_clients_file': '/etc/caddy/access/allowed-clients.pem'}


class PlatformAccessTests(unittest.TestCase):
    def test_policy_requires_verification_and_exact_leaf_allowlist_only_on_platform(self):
        proxy = render({'platform_hostname': 'platform.example.com', 'app_hostname': 'app.example.com',
                        'project': 'a'*32, 'platform_port': 3000}, 15000, POLICY)
        platform, app = proxy.split('app.example.com {')
        self.assertIn('mode require_and_verify', platform)
        self.assertIn('trust_pool file /etc/caddy/access/client-ca.pem', platform)
        self.assertIn('verifier leaf file /etc/caddy/access/allowed-clients.pem', platform)
        self.assertIn('header_up -X-Client-*', platform)
        self.assertIn('header_up -SSL-Client-*', platform)
        self.assertIn('strict_sni_host on', platform)
        self.assertNotIn('client_auth', app)
        self.assertIn('header_up -Cookie', app)
        self.assertIn('reverse_proxy 127.0.0.1:15000', app)
        self.assertNotIn('tls_insecure_skip_verify', proxy)

    def test_plain_https_remains_an_explicit_ungated_option(self):
        self.assertNotIn('client_auth', render_platform('platform.example.com', 3000))

    def test_invalid_policy_paths_and_extra_fields_are_rejected(self):
        for path in ['relative.pem', '/a/../b', '/a/./b', '/a//b', '//etc/client.pem', '/a/', '/', '/a b',
                     '/a\nrespond 200', '/a{env.SECRET}', '/a"', '/a; b', '/a*', None, True]:
            for key in POLICY:
                with self.subTest(path=path, key=key), self.assertRaises(ValueError):
                    validate_policy(POLICY | {key: path})
        for policy in [None, {}, POLICY | {'mode': 'request'},
                       POLICY | {'allowed_clients_file': POLICY['client_ca_file']}]:
            with self.assertRaises(ValueError):
                validate_policy(policy)

    def test_no_output_created_for_bad_host_port_or_policy(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)/'review'
            for host, port, policy in [('*.example.com', 3000, POLICY),
                    ('platform.example.com', True, POLICY), ('platform.example.com', 443, POLICY),
                    ('platform.example.com', 3000, {}), ('platform.example.com', 3000, None)]:
                with self.assertRaises(ValueError):
                    generate(host, port, policy, output)
                self.assertFalse(output.exists())

    def test_policy_load_is_bounded_and_generation_never_overwrites(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            policy = root/'policy.toml'
            policy.write_text(''.join(f'{key} = "{value}"\n' for key, value in POLICY.items()))
            self.assertEqual(load_policy(policy), POLICY)
            output = root/'review'
            generate('platform.example.com', 3000, load_policy(policy), output)
            self.assertEqual(output.stat().st_mode & 0o777, 0o700)
            before = (output/'Caddyfile').read_bytes()
            with self.assertRaises(FileExistsError):
                generate('other.example.com', 3000, POLICY, output)
            self.assertEqual((output/'Caddyfile').read_bytes(), before)
            policy.write_bytes(b'#'*8193)
            with self.assertRaises(ValueError):
                load_policy(policy)

    def test_cli_requires_explicit_access_choice_and_rejects_ungated_platform_only(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)/'review'
            command = [sys.executable, 'scripts/configure_app_hostname.py', '--platform-hostname',
                       'platform.example.com', '--platform-only', '--output', str(output)]
            for extra in ([], ['--public-platform']):
                result = subprocess.run(command+extra, capture_output=True, timeout=5)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(output.exists())
