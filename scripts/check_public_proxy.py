#!/usr/bin/env python3
"""Disposable TLS/proxy check, invoked by the ignored Rust integration test.

Only localhost listeners and a temporary Caddy CA; no trust-store installation,
public DNS, ACME, service registration or real account data. Caddy must be supplied.
"""
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import threading
from configure_app_hostname import render
import os
from pathlib import Path
import socket
import ssl
import subprocess
import sys
import tempfile
import time


def fixture_clients(root):
    """Disposable identities only; never installs trust or exports device keys."""
    def openssl(*args):
        subprocess.run(['openssl', *map(str, args)], check=True, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, timeout=10)
    ca = root/'client-ca.pem'
    key = root/'client-ca.key'
    openssl('req', '-new', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256',
            '-nodes', '-subj', '/CN=Disposable Client CA', '-days', '2',
            '-addext', 'basicConstraints=critical,CA:TRUE',
            '-addext', 'keyUsage=critical,keyCertSign,cRLSign', '-keyout', key, '-out', ca)
    clients = {}
    for serial, name in enumerate(('allowed', 'spare', 'rogue', 'expired', 'server-only'), 1):
        certificate, private, csr = root/(name+'.pem'), root/(name+'.key'), root/(name+'.csr')
        openssl('req', '-new', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256',
                '-nodes', '-subj', '/CN=Disposable Device', '-keyout', private, '-out', csr)
        extensions = root/(name+'.ext')
        extensions.write_text('basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\n'
                              + 'extendedKeyUsage=' + ('serverAuth' if name == 'server-only' else 'clientAuth') + '\n')
        signer = ['-signkey', private] if name == 'rogue' else ['-CA', ca, '-CAkey', key]
        if name == 'expired':
            (root/'index').write_text('')
            (root/'serial').write_text('10\n')
            ca_config = root/'fixture-ca.conf'
            ca_config.write_text(f'[ca]\ndefault_ca=fixture\n[fixture]\ndatabase={root}/index\n'
                                 f'serial={root}/serial\nnew_certs_dir={root}\ndefault_md=sha256\n'
                                 'policy=fixture_policy\n[fixture_policy]\ncommonName=supplied\n')
            openssl('ca', '-batch', '-notext', '-config', ca_config, '-cert', ca, '-keyfile', key,
                    '-in', csr, '-startdate', '20000101000000Z', '-enddate', '20000102000000Z',
                    '-extfile', extensions, '-out', certificate)
        else:
            openssl('x509', '-req', '-in', csr, *signer, '-set_serial', str(serial),
                    '-days', '1', '-extfile', extensions, '-out', certificate)
        private.chmod(0o600)
        clients[name] = certificate, private
    key.chmod(0o600)
    allowed = root/'allowed-clients.pem'
    # The rogue/expired/server-only certs are deliberately allowlisted: the chain,
    # lifetime and client-auth usage checks must still reject them independently.
    allowed.write_bytes(b''.join(clients[name][0].read_bytes()
                                for name in ('allowed', 'rogue', 'expired', 'server-only')))
    return {'client_ca_file': str(ca), 'allowed_clients_file': str(allowed)}, clients


def check(caddy, backend, protected=False):
    host = "platform.example.test"
    origin = "https://" + host
    with tempfile.TemporaryDirectory(prefix="devoption-proxy-") as directory:
        root = Path(directory).resolve()
        policy, clients = fixture_clients(root) if protected else (None, {})
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        class App(BaseHTTPRequestHandler):
            revision = 'A'
            seen_headers = None
            def do_GET(self):
                App.seen_headers = dict(self.headers)
                body = ('app ' + App.revision).encode()
                self.send_response(200); self.send_header('Content-Length', str(len(body)))
                self.end_headers(); self.wfile.write(body)
            def log_message(self, *_): pass
        app = ThreadingHTTPServer(('127.0.0.1', 0), App)
        thread = threading.Thread(target=app.serve_forever, daemon=True); thread.start()
        app_host = 'app.example.test'
        template = render(dict(platform_hostname=host, app_hostname=app_host, project='a'*32, platform_port=int(backend.split(':')[1])), app.server_port, policy)
        config = template.replace("admin off", "admin off\n\tauto_https disable_redirects\n\tskip_install_trust")
        if protected:
            config = config.replace('    tls {', '    tls {\n        issuer internal', 1)
            config = config.replace(host + " {", f"{host}:{port} {{\n\tbind 127.0.0.1")
        else:
            config = config.replace(host + " {", f"{host}:{port} {{\n\tbind 127.0.0.1\n\ttls internal")
        config = config.replace(app_host + " {", f"{app_host}:{port} {{\n\tbind 127.0.0.1\n\ttls internal")
        (root / "Caddyfile").write_text(config)
        environment = {"PATH": os.defpath, "HOME": str(root),
                       "XDG_DATA_HOME": str(root / "data"), "XDG_CONFIG_HOME": str(root / "config")}
        with (root / "caddy.log").open("w+") as log:
            process = subprocess.Popen([caddy, "run", "--config", str(root / "Caddyfile"), "--adapter", "caddyfile"],
                                       env=environment, stdout=log, stderr=log)
            try:
                cert = root / "data/caddy/pki/authorities/local/root.crt"
                deadline = time.monotonic() + 15
                while True:
                    if process.poll() is not None:
                        log.seek(0)
                        raise RuntimeError("Caddy stopped: " + log.read(8192))
                    try:
                        context = ssl.create_default_context(cafile=str(cert))
                        anonymous = ssl.create_default_context(cafile=str(cert))
                        if protected:
                            context.load_cert_chain(*map(str, clients['allowed']))
                        with context.wrap_socket(socket.create_connection(("127.0.0.1", port), 1), server_hostname=host):
                            break
                    except (OSError, ssl.SSLError):
                        if time.monotonic() >= deadline:
                            raise RuntimeError("local Caddy TLS readiness timed out")
                        time.sleep(0.1)

                def request(method, path, headers=None, body="", target=host, tls_context=None):
                    fields = {"Host": target, "Connection": "close", "Content-Length": str(len(body.encode()))}
                    fields.update(headers or {})
                    with (tls_context or context).wrap_socket(socket.create_connection(("127.0.0.1", port), 5), server_hostname=target) as stream:
                        payload = f"{method} {path} HTTP/1.1\r\n" + "".join(f"{k}: {v}\r\n" for k, v in fields.items()) + "\r\n" + body
                        stream.sendall(payload.encode())
                        response = http.client.HTTPResponse(stream)
                        response.begin()
                        data = response.read(1024 * 1024)
                        return response.status, dict(response.getheaders()), data

                def auth_headers(fields):
                    return {"Origin": origin, "Cookie": fields["Set-Cookie"].split(";", 1)[0], "X-CSRF-Token": fields["X-Csrf-Token"]}

                assert request("GET", "/", target=app_host, tls_context=anonymous)[2] == b'app A'
                assert request("GET", "/", {"Cookie":"__Host-devoption=hidden", "Authorization":"Bearer hidden", "X-DevOption-Test":"hidden", "X-Csrf-Token":"hidden"}, target=app_host, tls_context=anonymous)[0] == 200
                assert not any(k.lower() in ('cookie','authorization','x-csrf-token') or k.lower().startswith('x-devoption-') for k in App.seen_headers)
                for path in ['/auth/session', '/auth/sign-in', '/setup', '/sign-in']:
                    assert request("GET", path, target=app_host, tls_context=anonymous)[0] == 404
                assert request("GET", "/", {"Host":host}, target=app_host, tls_context=anonymous)[0] == 421
                try: request("GET", "/", target='unknown.example.test')
                except ssl.SSLError: pass
                else: raise AssertionError('unknown TLS hostname accepted')
                App.revision='B'
                assert request("GET", "/", target=app_host, tls_context=anonymous)[2] == b'app B'
                if protected:
                    def client_context(name):
                        selected = ssl.create_default_context(cafile=str(cert))
                        selected.load_cert_chain(*map(str, clients[name]))
                        return selected

                    def denied(selected, extra_headers=None):
                        try:
                            request('GET', '/sign-in', {
                                'X-Client-Cert': 'forged', 'X-SSL-Client-Verify': 'SUCCESS',
                                'SSL-Client-Verify': 'SUCCESS', 'Cookie': '__Host-devoption=forged',
                            } | (extra_headers or {}), tls_context=selected)
                        except (ssl.SSLError, ConnectionResetError):
                            return
                        raise AssertionError('unauthorized client reached platform HTTP')

                    denied(anonymous)
                    for name in ('spare', 'rogue', 'expired', 'server-only'):
                        denied(client_context(name))
                    assert request('GET', '/sign-in')[0] == 200
                    assert request('GET', '/projects', {'X-SSL-Client-Verify': 'SUCCESS'})[0] == 303
                    # No client certificate on the app SNI may bypass the platform gate.
                    assert request('GET', '/sign-in', {'Host': host}, target=app_host,
                                   tls_context=anonymous)[0] == 421

                assert request("GET", "/projects")[0] == 303
                assert request("GET", "/sign-in")[0] == 200
                status, fields, _ = request("POST", "/auth/challenge", {"Origin": origin})
                assert status == 204
                auth = auth_headers(fields)
                auth["Content-Type"] = "application/x-www-form-urlencoded"
                status, fields, _ = request("POST", "/auth/sign-in", auth, "password=existing+owner+fixture+only")
                assert status == 204
                assert "Secure; HttpOnly; SameSite=Strict" in fields["Set-Cookie"]
                assert "Domain=" not in fields["Set-Cookie"]
                auth = auth_headers(fields)
                auth["Content-Type"] = "text/plain"
                assert request("POST", "/projects", auth | {"Origin": "https://app.example.test"}, "Rejected")[0] == 403
                project_name = 'Protected TLS proxy fixture' if protected else 'TLS proxy fixture'
                assert request("POST", "/projects", auth, project_name)[0] == 204
                assert project_name.encode() in request("GET", "/projects", auth)[2]
                # Caddy strips caller-supplied AND generated forwarding metadata.
                assert request("POST", "/auth/session", auth | {"Forwarded": "host=evil.test;proto=http", "X-Forwarded-Host": "evil.test", "X-Forwarded-Proto": "http", "X-Real-IP": "192.0.2.1"})[0] == 204
                assert request("GET", "/projects", auth | {"Host": "other.example.test"})[0] != 200
                assert request("POST", "/auth/sign-out", auth)[0] == 204
                assert request("GET", "/projects", auth)[0] == 303
                if protected:
                    status, fields, _ = request('POST', '/auth/challenge', {'Origin': origin})
                    assert status == 204
                    login = auth_headers(fields) | {'Content-Type': 'application/x-www-form-urlencoded'}
                    status, fields, _ = request('POST', '/auth/sign-in', login,
                                                'password=existing+owner+fixture+only')
                    assert status == 204
                    active_auth = auth_headers(fields)
                    assert request('GET', '/projects', active_auth)[0] == 200
                    # Missing certificate inputs must prevent configuration activation.
                    for value in policy.values():
                        path = Path(value)
                        moved = path.with_suffix('.saved')
                        path.rename(moved)
                        try:
                            result = subprocess.run([caddy, 'adapt', '--validate', '--config', str(root/'Caddyfile'),
                                                     '--adapter', 'caddyfile'], env=environment,
                                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
                            assert result.returncode != 0
                        finally:
                            moved.rename(path)
                    # Replacement + full restart drops old connections and TLS sessions.
                    # Retain an already issued platform cookie to prove it is insufficient.
                    old_context = context
                    allowed = Path(policy['allowed_clients_file'])
                    allowed.write_bytes(clients['spare'][0].read_bytes())
                    process.terminate(); process.wait(timeout=5)
                    process = subprocess.Popen([caddy, 'run', '--config', str(root/'Caddyfile'), '--adapter', 'caddyfile'],
                                               env=environment, stdout=log, stderr=log)
                    context = client_context('spare')
                    deadline = time.monotonic() + 15
                    while True:
                        try:
                            assert request('GET', '/sign-in')[0] == 200
                            break
                        except OSError:
                            if process.poll() is not None or time.monotonic() >= deadline:
                                raise RuntimeError('protected proxy restart failed')
                            time.sleep(.1)
                    denied(old_context, active_auth)
                    assert request('GET', '/projects', active_auth)[0] == 200
                    assert request('GET', '/', target=app_host, tls_context=anonymous)[0] == 200
                    assert request('GET', '/projects')[0] == 303
                    allowed.write_text('')
                    process.terminate(); process.wait(timeout=5)
                    # Empty allowlist may refuse activation or run with no accepted leafs;
                    # in either case it must never become an ungated administrative site.
                    result = subprocess.run([caddy, 'adapt', '--validate', '--config', str(root/'Caddyfile'),
                                             '--adapter', 'caddyfile'], env=environment,
                                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
                    if result.returncode == 0:
                        process = subprocess.Popen([caddy, 'run', '--config', str(root/'Caddyfile'), '--adapter', 'caddyfile'],
                                                   env=environment, stdout=log, stderr=log)
                        deadline = time.monotonic() + 15
                        while True:
                            try:
                                assert request('GET', '/', target=app_host, tls_context=anonymous)[0] == 200
                                break
                            except OSError:
                                if process.poll() is not None or time.monotonic() >= deadline:
                                    raise RuntimeError('empty-allowlist proxy readiness failed')
                                time.sleep(.1)
                        denied(context)
                        denied(anonymous)
                    print('Protected proxy: verified permitted client plus owner login, absent/untrusted/unlisted/expired/wrong-usage denial, forged-header and cross-host denial, missing/empty trust input refusal, revocation/restart and public-app isolation passed.')
            finally:
                app.shutdown(); app.server_close()
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
    print("Local TLS proxy: two hostname routes, app update, cookie stripping, blocked control paths, unknown-host rejection, authenticated project creation, host/origin checks, stripped forwarding metadata and logout passed; temporary CA removed.")


if __name__ == "__main__":
    if len(sys.argv) != 3 or not sys.argv[2].startswith("127.0.0.1:"):
        sys.exit("usage: check_public_proxy.py /absolute/path/to/caddy 127.0.0.1:PORT")
    for protected in (False, True):
        check(str(Path(sys.argv[1]).resolve(strict=True)), sys.argv[2], protected)
