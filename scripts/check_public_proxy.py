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


def check(caddy, backend):
    host = "platform.example.test"
    origin = "https://" + host
    with tempfile.TemporaryDirectory(prefix="devoption-proxy-") as directory:
        root = Path(directory)
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
        template = render(dict(platform_hostname=host, app_hostname=app_host, project='a'*32, platform_port=int(backend.split(':')[1])), app.server_port)
        config = template.replace("admin off", "admin off\n\tauto_https disable_redirects\n\tskip_install_trust")
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
                        with context.wrap_socket(socket.create_connection(("127.0.0.1", port), 1), server_hostname=host):
                            break
                    except (OSError, ssl.SSLError):
                        if time.monotonic() >= deadline:
                            raise RuntimeError("local Caddy TLS readiness timed out")
                        time.sleep(0.1)

                def request(method, path, headers=None, body="", target=host):
                    fields = {"Host": target, "Connection": "close", "Content-Length": str(len(body.encode()))}
                    fields.update(headers or {})
                    with context.wrap_socket(socket.create_connection(("127.0.0.1", port), 5), server_hostname=target) as stream:
                        payload = f"{method} {path} HTTP/1.1\r\n" + "".join(f"{k}: {v}\r\n" for k, v in fields.items()) + "\r\n" + body
                        stream.sendall(payload.encode())
                        response = http.client.HTTPResponse(stream)
                        response.begin()
                        data = response.read(1024 * 1024)
                        return response.status, dict(response.getheaders()), data

                def auth_headers(fields):
                    return {"Origin": origin, "Cookie": fields["Set-Cookie"].split(";", 1)[0], "X-CSRF-Token": fields["X-Csrf-Token"]}

                assert request("GET", "/", target=app_host)[2] == b'app A'
                assert request("GET", "/", {"Cookie":"__Host-devoption=hidden", "Authorization":"Bearer hidden", "X-DevOption-Test":"hidden", "X-Csrf-Token":"hidden"}, target=app_host)[0] == 200
                assert not any(k.lower() in ('cookie','authorization','x-csrf-token') or k.lower().startswith('x-devoption-') for k in App.seen_headers)
                for path in ['/auth/session', '/auth/sign-in', '/setup', '/sign-in']:
                    assert request("GET", path, target=app_host)[0] == 404
                assert request("GET", "/", {"Host":host}, target=app_host)[0] == 421
                try: request("GET", "/", target='unknown.example.test')
                except ssl.SSLError: pass
                else: raise AssertionError('unknown TLS hostname accepted')
                App.revision='B'
                assert request("GET", "/", target=app_host)[2] == b'app B'
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
                assert request("POST", "/projects", auth, "TLS proxy fixture")[0] == 204
                assert b"TLS proxy fixture" in request("GET", "/projects", auth)[2]
                # Caddy strips caller-supplied AND generated forwarding metadata.
                assert request("POST", "/auth/session", auth | {"Forwarded": "host=evil.test;proto=http", "X-Forwarded-Host": "evil.test", "X-Forwarded-Proto": "http", "X-Real-IP": "192.0.2.1"})[0] == 204
                assert request("GET", "/projects", auth | {"Host": "other.example.test"})[0] != 200
                assert request("POST", "/auth/sign-out", auth)[0] == 204
                assert request("GET", "/projects", auth)[0] == 303
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
    check(str(Path(sys.argv[1]).resolve(strict=True)), sys.argv[2])
