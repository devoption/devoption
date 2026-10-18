import contextlib
import io
import json
import socket
import ssl
import unittest
from unittest.mock import MagicMock, patch

import check_public_app as checker


class PublicAppTests(unittest.TestCase):
    platform = 'platform.example.com'
    app = 'app.example.com'
    addresses = ['8.8.8.8', '2001:4860:4860::8888']
    # These are mocked addresses only: tests never query DNS or contact them.

    def response(self, address, server, path, host=None):
        if host:
            return 421, None, b''
        if server == self.app:
            return (200, None, b'ok A') if path == '/healthz' else (404, None, b'')
        return (303, '/sign-in', b'') if path == '/projects' else (200, None, b'login')

    def run_check(self, response=None, resolved=None):
        with patch.object(checker, 'resolve', side_effect=resolved or (lambda _: self.addresses)), \
             patch.object(checker, 'request', side_effect=response or self.response) as request:
            return checker.check(self.platform, self.app, self.addresses, 'ok A'), request

    def test_checks_both_address_families_and_control_separation(self):
        report, request = self.run_check()
        self.assertTrue(report['passed'])
        self.assertEqual(len(report['checks']), 22)
        self.assertEqual(request.call_count, 20)
        self.assertEqual({call.args[0] for call in request.call_args_list}, set(self.addresses))

    def test_resolver_deduplicates_addresses(self):
        records = [(socket.AF_INET, socket.SOCK_STREAM, 6, '', ('8.8.8.8', 443)),
                   (socket.AF_INET6, socket.SOCK_STREAM, 6, '', ('2001:4860:4860::8888', 443, 0, 0))]
        with patch.object(checker.socket, 'getaddrinfo', return_value=records + records) as resolve:
            self.assertEqual(set(checker.resolve(self.app)), set(self.addresses))
            resolve.assert_called_once_with(self.app, 443, type=socket.SOCK_STREAM)

    def test_one_bad_hostname_does_not_skip_the_other(self):
        report, request = self.run_check(resolved=lambda host: [] if host == self.platform else self.addresses)
        self.assertFalse(report['passed'])
        self.assertEqual(request.call_count, 14)
        self.assertTrue(all(call.args[1] == self.app for call in request.call_args_list))

    def test_stale_extra_or_missing_dns_never_connects(self):
        for found in ([], self.addresses[:1], self.addresses + ['1.1.1.1']):
            with self.subTest(found=found):
                report, request = self.run_check(resolved=lambda _: found)
                self.assertFalse(report['passed'])
                request.assert_not_called()
        report, request = self.run_check(resolved=lambda _: (_ for _ in ()).throw(socket.gaierror()))
        self.assertFalse(report['passed'])
        request.assert_not_called()

    def test_failures_are_recorded_and_other_probes_continue(self):
        for failure in (ssl.SSLCertVerificationError(), ssl.SSLError(), TimeoutError(), ValueError()):
            with self.subTest(failure=type(failure)):
                report, request = self.run_check(response=lambda *args: (_ for _ in ()).throw(failure))
                self.assertFalse(report['passed'])
                self.assertEqual(request.call_count, 20)

    def test_wrong_health_redirect_and_exposed_control_paths_fail(self):
        for target, replacement in [('/healthz', (200, None, b'ok B')),
                                    ('/projects', (303, 'https://other.example/sign-in', b'')),
                                    ('/auth/session', (200, None, b'admin'))]:
            def response(address, server, path, host=None):
                return replacement if path == target else self.response(address, server, path, host)
            report, _ = self.run_check(response=response)
            self.assertFalse(report['passed'])

    def test_invalid_inputs_fail_before_network(self):
        cases = [(self.platform, self.platform, self.addresses, 'ok A'),
                 ('https://platform.example.com', self.app, self.addresses, 'ok A'),
                 (self.platform, self.app, ['127.0.0.1'], 'ok A'),
                 (self.platform, self.app, ['192.168.1.1'], 'ok A'),
                 (self.platform, self.app, ['224.0.0.1'], 'ok A'),
                 (self.platform, self.app, ['ff0e::1'], 'ok A'),
                 (self.platform, self.app, ['2001:4860:4860::8888%en0'], 'ok A'),
                 (self.platform, self.app, [], 'ok A'),
                 (self.platform, self.app, self.addresses, ''),
                 (self.platform, self.app, self.addresses, 'x' * 1025)]
        with patch.object(checker, 'resolve') as resolve:
            for args in cases:
                with self.subTest(args=args), self.assertRaises(ValueError):
                    checker.check(*args)
            resolve.assert_not_called()

    def test_transport_pins_address_verifies_sni_and_sends_no_credentials(self):
        response = MagicMock()
        response.status = 200
        response.read.return_value = b'ok A'
        response.getheader.return_value = None
        response.__enter__.return_value = response
        with patch.object(checker.ssl, 'create_default_context') as context, \
             patch.object(checker.socket, 'create_connection') as connect, \
             patch.object(checker.http.client, 'HTTPResponse', return_value=response):
            self.assertEqual(checker.request('8.8.8.8', self.app, '/healthz'), (200, None, b'ok A'))
            connect.assert_called_once_with(('8.8.8.8', 443), timeout=5)
            context.assert_called_once_with()
            self.assertEqual(context.return_value.wrap_socket.call_args.kwargs['server_hostname'], self.app)
            stream = context.return_value.wrap_socket.return_value.__enter__.return_value
            self.assertEqual(stream.sendall.call_args.args[0],
                             b'GET /healthz HTTP/1.1\r\nHost: app.example.com\r\nConnection: close\r\nAccept-Encoding: identity\r\n\r\n')
            response.read.assert_called_with(checker.MAX_BODY + 1)
            response.read.return_value = b'x' * (checker.MAX_BODY + 1)
            with self.assertRaises(ValueError):
                checker.request('8.8.8.8', self.app, '/healthz')

    def test_cli_json_and_failure_exit(self):
        for passed in (True, False):
            output = io.StringIO()
            with patch.object(checker, 'check', return_value={'passed': passed}), contextlib.redirect_stdout(output):
                code = checker.main(['--platform-hostname', self.platform, '--app-hostname', self.app,
                                     '--expected-address', '8.8.8.8', '--expected-health', 'ok A'])
            self.assertEqual(code, 0 if passed else 1)
            self.assertEqual(json.loads(output.getvalue()), {'passed': passed})

    def test_transport_accepts_current_page_size_but_keeps_a_fixed_body_limit(self):
        # The source-built public sign-in page measured 90,219 bytes. Exercise
        # real HTTP body decoding so a successful page cannot hit the old 64 KiB cap.
        for size in (90219, 128 * 1024, 128 * 1024 + 1):
            with self.subTest(size=size), \
                 patch.object(checker.ssl, 'create_default_context') as context, \
                 patch.object(checker.socket, 'create_connection'):
                stream = context.return_value.wrap_socket.return_value.__enter__.return_value
                stream.makefile.return_value = io.BytesIO(
                    f'HTTP/1.1 200 OK\r\nContent-Length: {size}\r\n\r\n'.encode() + b'x' * size)
                if size > 128 * 1024:
                    with self.assertRaises(ValueError):
                        checker.request('8.8.8.8', self.platform, '/sign-in')
                else:
                    status, _, body = checker.request('8.8.8.8', self.platform, '/sign-in')
                    self.assertEqual(status, 200)
                    self.assertEqual(len(body), size)


if __name__ == '__main__':
    unittest.main()
