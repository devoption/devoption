"""Disposable HTTPS -> proxy -> real WASIp2 app-session acceptance.

Requires explicitly supplied trusted fixture/runtime/proxy and a disposable
platform backend. Starts only temporary loopback processes; installs no trust.
"""
import argparse
from email.message import Message
import http.client
from http.cookiejar import CookieJar
import os
from pathlib import Path
import socket
import ssl
import subprocess
import tempfile
import time
from types import SimpleNamespace
from urllib.request import Request

from configure_app_hostname import render


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def check(caddy, runtime, component, backend):
    with tempfile.TemporaryDirectory(prefix='devoption-app-auth-') as temporary:
        root = Path(temporary).resolve()
        children = []
        with (root/'process.log').open('w+') as log:
            try:
                ports = [free_port(), free_port()]
                for port in ports:
                    runtime_directory = root/f'worker-{port}'
                    runtime_directory.mkdir()
                    children.append(subprocess.Popen(
                        [str(runtime), 'app-worker', str(component), str(port), 'serve'],
                        env={'PATH': os.defpath, 'LANG': 'C.UTF-8',
                             'RUNTIME_DIRECTORY': str(runtime_directory)},
                        stdin=subprocess.DEVNULL, stdout=log, stderr=log))
                port = free_port()
                template = render(dict(platform_hostname='platform.example.test',
                    app_hostname='one.example.test', project='a'*32,
                    platform_port=int(backend.split(':')[1])), ports[0])
                other = render(dict(platform_hostname='platform.example.test',
                    app_hostname='two.example.test', project='b'*32,
                    platform_port=int(backend.split(':')[1])), ports[1])
                template += other[other.index('\ntwo.example.test {'):]
                config = template.replace('admin off', 'admin off\n    auto_https disable_redirects\n    skip_install_trust')
                for host in ('platform', 'one', 'two'):
                    config = config.replace(f'{host}.example.test {{',
                        f'{host}.example.test:{port} {{\n    bind 127.0.0.1\n    tls internal')
                (root/'Caddyfile').write_text(config)
                children.append(subprocess.Popen([str(caddy), 'run', '--config', str(root/'Caddyfile'),
                    '--adapter', 'caddyfile'], stdout=log, stderr=log, env={
                        'PATH': os.defpath, 'HOME': str(root),
                        'XDG_DATA_HOME': str(root/'data'), 'XDG_CONFIG_HOME': str(root/'config')}))
                deadline = time.monotonic() + 120
                while True:
                    if any(child.poll() is not None for child in children):
                        raise RuntimeError('fixture process exited')
                    try:
                        context = ssl.create_default_context(cafile=str(root/'data/caddy/pki/authorities/local/root.crt'))
                        with context.wrap_socket(socket.create_connection(('127.0.0.1', port), 1),
                                server_hostname='one.example.test'):
                            break
                    except OSError:
                        if time.monotonic() >= deadline:
                            raise RuntimeError('fixture TLS readiness timed out')
                        time.sleep(.1)

                jar = CookieJar()

                def request(host, path, method='GET', headers=None, browser=False, body=''):
                    authority = f'{host}.example.test' + ('' if host == 'platform' else f':{port}')
                    url = f'https://{host}.example.test:{port}{path}'
                    selected = Request(url, method=method)
                    if browser:
                        jar.add_cookie_header(selected)
                    fields = {'Host': authority, 'Connection': 'close', 'Content-Length': str(len(body.encode()))}
                    fields.update(dict(selected.header_items()))
                    fields.update(headers or {})
                    with context.wrap_socket(socket.create_connection(('127.0.0.1', port), 5),
                            server_hostname=f'{host}.example.test') as stream:
                        stream.sendall((f'{method} {path} HTTP/1.1\r\n'
                            + ''.join(f'{k}: {v}\r\n' for k, v in fields.items()) + '\r\n' + body).encode())
                        response = http.client.HTTPResponse(stream)
                        response.begin()
                        body = response.read(65537)
                        message = Message()
                        for key, value in response.getheaders():
                            message[key] = value
                        if browser:
                            jar.extract_cookies(SimpleNamespace(info=lambda: message), selected)
                        return response.status, message, body

                for host in ('one', 'two'):
                    # Runtime readiness is checked separately from TLS readiness.
                    while request(host, '/healthz')[0] != 200:
                        if any(child.poll() is not None for child in children):
                            raise RuntimeError('fixture process exited before worker readiness')
                        if time.monotonic() >= deadline:
                            raise RuntimeError('fixture worker readiness timed out')
                        time.sleep(.1)
                    assert request(host, '/account/session', browser=True)[0] == 401
                one_origin = f'https://one.example.test:{port}'
                app_headers = {'Origin': one_origin, 'X-App-CSRF-Token': 'fixture-csrf'}
                assert request('one', '/account/login', 'POST', app_headers | {'Origin': 'https://evil.test'}, True)[0] == 401
                status, fields, _ = request('one', '/account/login', 'POST', app_headers, True)
                assert status == 200
                cookie = fields['Set-Cookie']
                assert cookie.startswith('__Host-devoption-app-session=fixture_one;')
                assert 'Domain=' not in cookie and all(flag in cookie for flag in ('Secure', 'HttpOnly', 'Path=/', 'SameSite=Strict'))
                observed = request('one', '/account/session', browser=True)
                assert observed[2] == b'member one', (observed, list(jar))
                assert request('two', '/account/session', browser=True)[0] == 401
                assert request('platform', '/projects', browser=True)[0] == 303
                assert request('two', '/account/session', headers={'Cookie': cookie.split(';')[0]})[0] == 401
                for extra in ({}, {'Origin': 'null'}, {'Origin': 'https://evil.test'},
                              {'Origin': f'https://two.example.test:{port}'}):
                    assert request('one', '/account/change', 'POST', extra, True)[0] == 403
                assert request('one', '/account/change', 'POST', {'Origin': one_origin}, True)[0] == 401
                assert request('one', '/account/change', 'POST', app_headers, True)[2] == b'changed'
                # Control credentials injected into the app request cannot reach the guest.
                assert request('one', '/account/session', headers={
                    'Cookie': '__Host-devoption=platform_secret; private=hidden; ' + cookie.split(';')[0],
                    'Authorization': 'Bearer platform_secret', 'Proxy-Authorization': 'Basic hidden',
                    'X-CSRF-Token': 'platform_secret', 'X-DevOption-Probe': 'hidden'})[2] == b'member one'
                for path in ('/unsafe-cookie', '/domain-cookie'):
                    assert request('one', path)[1].get_all('Set-Cookie') is None
                # A second app's independent session does not replace the first.
                assert request('two', '/account/login', 'POST', {
                    'Origin': f'https://two.example.test:{port}', 'X-App-CSRF-Token': 'fixture-csrf'}, True)[0] == 200
                assert request('two', '/account/session', browser=True)[2] == b'member two'
                assert request('one', '/account/session', browser=True)[2] == b'member one'
                assert request('one', '/account/logout', 'POST', app_headers, True)[0] == 200
                assert request('one', '/account/session', browser=True)[0] == 401
                assert request('two', '/account/session', browser=True)[2] == b'member two'
                assert request('platform', '/projects', browser=True)[0] == 303
                for path in ('/auth/session', '/auth/sign-in', '/sign-in', '/setup'):
                    assert request('one', path, browser=True)[0] == 404
                # The platform's real owner login remains a separate session.
                platform_origin = 'https://platform.example.test'
                status, fields, _ = request('platform', '/auth/challenge', 'POST',
                    {'Origin': platform_origin}, True)
                assert status == 204
                status, _, _ = request('platform', '/auth/sign-in', 'POST', {
                    'Origin': platform_origin, 'X-CSRF-Token': fields['X-Csrf-Token'],
                    'Content-Type': 'application/x-www-form-urlencoded'}, True,
                    'password=existing+owner+fixture+only')
                assert status == 204
                assert request('platform', '/projects', browser=True)[0] == 200
                assert request('one', '/account/session', browser=True)[0] == 401
                assert request('two', '/account/session', browser=True)[2] == b'member two'
                print('Real WASIp2 app authentication: sign-in/session/sign-out, two host-only browser sessions, '
                      'audience/control isolation, credential filtering, unsafe Origin/CSRF refusal and cookie scope passed.')
            except Exception:
                log.flush(); log.seek(max(0, log.tell() - 16384))
                print(log.read(16384))
                for worker_log in root.glob('worker-*/worker.log'):
                    print(worker_log.read_text()[-4096:])
                raise
            finally:
                for child in reversed(children):
                    if child.poll() is None:
                        child.terminate()
                    try:
                        child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        child.kill(); child.wait(timeout=5)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('caddy', 'runtime', 'component'):
        parser.add_argument('--'+name, required=True, type=Path)
    parser.add_argument('--platform-backend', required=True)
    args = parser.parse_args()
    if not args.platform_backend.startswith('127.0.0.1:'):
        parser.error('use a disposable loopback platform backend')
    check(args.caddy.resolve(strict=True), args.runtime.resolve(strict=True),
          args.component.resolve(strict=True), args.platform_backend)
