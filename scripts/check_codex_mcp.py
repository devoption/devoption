#!/usr/bin/env python3
"""Opt-in real Codex OAuth/tool proof, using only disposable local state.

Requires installed Codex, Cargo and OpenSSL; never installs them. The fixed
43123 callback is registered by the existing Rust fixture. No model turn runs.
"""
from __future__ import annotations

import argparse
import contextlib
import http.client
import http.server
import json
import os
from pathlib import Path
import queue
import re
import select
import shutil
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import urllib.parse
import urllib.request

ORIGIN = "https://platform.example.test"
RESOURCE = ORIGIN + "/mcp"
CALLBACK = "http://127.0.0.1:43123/callback"
COOKIE = "__Host-devoption_session=" + "ab" * 32
CSRF = "cd" * 32
TOOLS = {"list_projects", "list_repositories", "list_builds", "get_project_status", "get_diagnostics"}
REPOSITORY = Path(__file__).resolve().parent.parent


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def wait_until(predicate, seconds=60):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        value = predicate()
        if value:
            return value
        time.sleep(0.1)
    raise TimeoutError("Disposable fixture operation timed out")


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)
    for stream in (process.stdin, process.stdout):
        if stream:
            stream.close()


def shutdown_fixture(process, stop_file):
    stop_file.touch()
    try:
        process.wait(timeout=15)
    except subprocess.TimeoutExpired:
        stop(process)


def reader(stream):
    messages = queue.Queue()
    def consume():
        for line in stream:
            messages.put(line)
    threading.Thread(target=consume, daemon=True).start()
    return messages


def certificate(root):
    ca, ca_key = root / "ca.pem", root / "ca-key.pem"
    cert, key, csr = root / "leaf.pem", root / "leaf-key.pem", root / "leaf.csr"
    extensions = root / "leaf.ext"
    def openssl(*arguments):
        subprocess.run(["openssl", *map(str, arguments)], check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
    openssl("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
            "-subj", "/CN=Disposable DevOption MCP CA", "-addext", "basicConstraints=critical,CA:TRUE",
            "-addext", "keyUsage=critical,keyCertSign,cRLSign", "-keyout", ca_key, "-out", ca)
    openssl("req", "-new", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=platform.example.test",
            "-keyout", key, "-out", csr)
    extensions.write_text("subjectAltName=DNS:platform.example.test\nbasicConstraints=critical,CA:FALSE\n"
                          "keyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n"
                          "subjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n")
    openssl("x509", "-req", "-in", csr, "-CA", ca, "-CAkey", ca_key, "-CAcreateserial",
            "-days", "1", "-extfile", extensions, "-out", cert)
    for path in (ca_key, key):
        path.chmod(0o600)
    return ca, cert, key


@contextlib.contextmanager
def gateway(address, cert, key):
    """Local TLS termination and a CONNECT proxy limited to one synthetic host.

    Preserve paths, Host, Origin, cookies, OAuth bodies and issuer/resource URLs.
    No protocol/security metadata is rewritten; no upstream internet is allowed.
    """
    observed, blocked = [], []
    target = urllib.parse.urlsplit(address)
    require(target.scheme == "http" and target.hostname == "127.0.0.1" and target.port,
            "Fixture must report a loopback HTTP address")
    class Forward(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass
        def forward(self):
            size = int(self.headers.get("Content-Length", "0"))
            require(0 <= size <= 16384, "Fixture request too large")
            body = self.rfile.read(size)
            connection = http.client.HTTPConnection(target.hostname, target.port, timeout=15)
            try:
                connection.request(self.command, self.path, body, dict(self.headers))
                response = connection.getresponse()
                data = response.read()
                operation = None
                if self.path == "/mcp" and body:
                    operation = json.loads(body).get("method")
                observed.append((self.path.split("?")[0], response.status, operation))
                self.send_response(response.status)
                for name, value in response.getheaders():
                    if name.lower() not in {"connection", "transfer-encoding", "content-length"}:
                        self.send_header(name, value)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
            finally:
                connection.close()
        do_GET = do_POST = do_DELETE = forward
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Forward)
    tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    tls.load_cert_chain(cert, key)
    server.socket = tls.wrap_socket(server.socket, server_side=True)
    class Connect(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass
        def do_CONNECT(self):
            if self.path != "platform.example.test:443":
                blocked.append(self.path)
                self.send_error(403)
                return
            with socket.create_connection(server.server_address, timeout=10) as upstream:
                self.send_response(200)
                self.end_headers()
                try:
                    while True:
                        ready, _, _ = select.select([upstream, self.connection], [], [], 30)
                        if not ready:
                            return
                        for source in ready:
                            data = source.recv(65536)
                            if not data:
                                return
                            (self.connection if source is upstream else upstream).sendall(data)
                except (ConnectionError, TimeoutError):
                    return
    proxy = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Connect)
    for instance in (server, proxy):
        threading.Thread(target=instance.serve_forever, daemon=True).start()
    try:
        yield f"http://127.0.0.1:{proxy.server_port}", observed, blocked
    finally:
        for instance in (proxy, server):
            instance.shutdown()
            instance.server_close()


class AppServer:
    def __init__(self, codex, env, root, cleanup):
        log = cleanup.enter_context((root / "app-server.log").open("w"))
        self.process = subprocess.Popen([codex, "app-server", "--stdio", "--strict-config"],
            cwd=root, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, text=True)
        cleanup.callback(stop, self.process)
        self.messages = reader(self.process.stdout)
        self.next_id = 0
        self.rpc("initialize", {"clientInfo": {"name": "devoption_fixture", "version": "1"},
                                "capabilities": {"experimentalApi": True}})
        self.send({"method": "initialized"})
    def send(self, message):
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()
    def rpc(self, method, params):
        self.next_id += 1
        identity = self.next_id
        self.send({"id": identity, "method": method, "params": params})
        end = time.monotonic() + 60
        while time.monotonic() < end:
            require(self.process.poll() is None, "Codex app-server exited unexpectedly")
            try:
                message = json.loads(self.messages.get(timeout=0.2))
            except queue.Empty:
                continue
            if message.get("id") == identity:
                return message
        raise TimeoutError(f"Codex app-server timed out: {method}")
    def notification(self, method):
        end = time.monotonic() + 30
        while time.monotonic() < end:
            try:
                message = json.loads(self.messages.get(timeout=0.2))
            except queue.Empty:
                continue
            if message.get("method") == method:
                return message.get("params", {})
        raise TimeoutError(f"Codex notification timed out: {method}")

    def call(self, thread, tool, arguments):
        return self.rpc("mcpServer/tool/call", {"threadId": thread, "server": "devoption",
                                               "tool": tool, "arguments": arguments})


def check(codex):
    version = subprocess.check_output([codex, "--version"], text=True, timeout=10).strip()
    with tempfile.TemporaryDirectory(prefix="devoption-codex-mcp-") as directory, contextlib.ExitStack() as cleanup:
        root = Path(directory)
        root.chmod(0o700)
        # Confirm the registered fixed callback is available, without changing the registration.
        with socket.socket() as callback:
            callback.bind(("127.0.0.1", 43123))
        fixture_info, stop_file = root / "fixture.json", root / "stop"
        fixture_log = cleanup.enter_context((root / "fixture.log").open("w"))
        fixture_env = dict(os.environ, DEVOPTION_MCP_FIXTURE_INFO=str(fixture_info),
                           DEVOPTION_MCP_FIXTURE_STOP=str(stop_file))
        fixture = subprocess.Popen(["cargo", "test", "-p", "devoption-server", "--lib", "--locked",
            "--offline", "manual_mcp_browser_fixture", "--", "--ignored", "--nocapture"],
            cwd=REPOSITORY, env=fixture_env, stdout=fixture_log, stderr=subprocess.STDOUT)
        cleanup.callback(shutdown_fixture, fixture, stop_file)
        def ready():
            require(fixture.poll() is None, "Rust fixture exited; inspect local prerequisites/build")
            return fixture_info.exists()
        wait_until(ready, 120)
        info = json.loads(fixture_info.read_text())
        ca, cert, key = certificate(root)
        with gateway(info["address"], cert, key) as (proxy, observed, blocked):
            home = root / "codex"
            home.mkdir(mode=0o700)
            (home / "config.toml").write_text('''model_provider = "fixture"
model = "fixture"
mcp_oauth_credentials_store = "file"
[analytics]
enabled = false
[mcp_servers.devoption]
url = "https://platform.example.test/mcp"
scopes = ["mcp:read"]
[mcp_servers.devoption.oauth]
client_id = "fixture"
callback_url = "http://127.0.0.1:43123/callback"
callback_port = 43123
[model_providers.fixture]
name = "Disposable no-model fixture"
base_url = "https://platform.example.test/no-model"
wire_api = "responses"
requires_openai_auth = false
''')
            (home / "config.toml").chmod(0o600)
            # Isolate config/credentials. Forward no API key, account token, or user proxy setting.
            env = {key: value for key, value in os.environ.items()
                   if key in {"PATH", "TMPDIR", "LANG", "LC_ALL", "SYSTEMROOT"}}
            env.update(CODEX_HOME=str(home), CODEX_CA_CERTIFICATE=str(ca), SSL_CERT_FILE=str(ca),
                       HTTPS_PROXY=proxy, HTTP_PROXY=proxy, ALL_PROXY=proxy,
                       https_proxy=proxy, http_proxy=proxy, NO_PROXY="127.0.0.1,localhost")
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({"https": proxy}),
                urllib.request.HTTPSHandler(context=ssl.create_default_context(cafile=str(ca))))
            login_log = cleanup.enter_context((root / "login.log").open("w"))
            login = subprocess.Popen([codex, "mcp", "login", "devoption", "--no-browser"], cwd=root,
                env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=login_log, text=True)
            cleanup.callback(stop, login)
            lines = reader(login.stdout)
            def authorization():
                require(login.poll() is None, "Codex OAuth login exited before authorization")
                try:
                    line = lines.get_nowait()
                except queue.Empty:
                    return None
                match = re.search(r"https://platform\.example\.test/oauth/authorize\?\S+", line)
                return match.group(0) if match else None
            url = wait_until(authorization, 30)
            parameters = urllib.parse.parse_qs(urllib.parse.urlsplit(url).query)
            for name, expected in {"client_id": "fixture", "redirect_uri": CALLBACK,
                                   "scope": "mcp:read", "resource": RESOURCE,
                                   "response_type": "code", "code_challenge_method": "S256"}.items():
                require(parameters.get(name) == [expected], f"Codex authorization mismatch: {name}")
            require(parameters.get("state") and parameters.get("code_challenge"), "Missing state/PKCE")
            def browser(path, data=None):
                request = urllib.request.Request(path, data=data, headers={"Cookie": COOKIE,
                    "X-CSRF-Token": CSRF, "Origin": ORIGIN,
                    "Content-Type": "application/x-www-form-urlencoded"})
                with opener.open(request, timeout=15) as response:
                    return response.read()
            require(b"Allow read access" in browser(url), "Explicit consent page unavailable")
            redirect = json.loads(browser(url, b"decision=allow"))["redirect"]
            result = urllib.parse.parse_qs(urllib.parse.urlsplit(redirect).query)
            require(result.get("state") == parameters["state"] and result.get("iss") == [ORIGIN],
                    "Consent callback state/issuer mismatch")
            login.stdin.write(redirect + "\n")
            login.stdin.flush()
            require(login.wait(timeout=30) == 0, "Codex rejected OAuth callback/token exchange")
            app = AppServer(codex, env, root, cleanup)
            status = app.rpc("mcpServerStatus/list", {"serverName": "devoption", "detail": "toolsAndAuthOnly"})
            servers = status.get("result", {}).get("data", [])
            require(len(servers) == 1 and set(servers[0].get("tools", {})) == TOOLS,
                    "Codex did not discover exactly five read-only tools")
            started = app.rpc("thread/start", {"cwd": str(root), "ephemeral": True,
                "modelProvider": "fixture", "model": "fixture", "approvalPolicy": "never", "sandbox": "read-only"})
            thread = started.get("result", {}).get("thread", {}).get("id")
            require(thread, "Codex did not initialize a disposable client runtime")
            # Creating a client runtime is not a model turn; never call turn/start.
            app.rpc("mcpServerStatus/list", {"threadId": thread, "serverName": "devoption", "detail": "toolsAndAuthOnly"})
            projects = app.call(thread, "list_projects", {})
            def content(reply):
                result = reply.get("result", {})
                require(not result.get("isError", True), "Codex tool call failed")
                return json.loads(result["content"][0]["text"])
            catalog = content(projects)
            require(catalog["items"][0]["name"] == "Example project", "Wrong disposable project")
            project = catalog["items"][0]["id"]
            for tool in sorted(TOOLS - {"list_projects"}):
                content(app.call(thread, tool, {} if tool == "get_diagnostics" else {"project_id": project}))
            html = browser(ORIGIN + "/mcp/grants").decode()
            grants = re.findall(r'name="grant_id" value="([A-Za-z0-9_-]{43})"', html)
            require(len(grants) == 1, "Expected exactly one disposable grant")
            browser(ORIGIN + "/mcp/grants", urllib.parse.urlencode({"grant_id": grants[0]}).encode())
            after_revoke = len(observed)
            denied = app.call(thread, "get_diagnostics", {})
            require("error" in denied or denied.get("result", {}).get("isError") is True,
                    "Codex received tool data after revocation")
            require(any(path == "/mcp" and code == 401 for path, code, _ in observed[after_revoke:]),
                    "Expected authentication denial was not observed")
            # Exercise the app-server login path and its real native callback listener too.
            native_login = app.rpc("mcpServer/oauth/login", {"name": "devoption", "scopes": ["mcp:read"], "timeoutSecs": 30})
            native = native_login.get("result", {})
            native_url = native.get("authorizationUrl")
            require(native_url and native_url.startswith(ORIGIN + "/oauth/authorize?"), "Native OAuth URL unavailable")
            native_parameters = urllib.parse.parse_qs(urllib.parse.urlsplit(native_url).query)
            require(native_parameters.get("redirect_uri") == [CALLBACK], "Native callback mismatch")
            native_redirect = json.loads(browser(native_url, b"decision=allow"))["redirect"]
            callback_url = urllib.parse.urlsplit(native_redirect)
            require(callback_url.scheme == "http" and callback_url.netloc == "127.0.0.1:43123"
                    and callback_url.path == "/callback", "Unexpected callback destination")
            callback = http.client.HTTPConnection("127.0.0.1", 43123, timeout=15)
            try:
                callback.request("GET", callback_url.path + "?" + callback_url.query)
                reply = callback.getresponse()
                reply.read()
                require(reply.status == 200, "Codex callback listener rejected consent")
            finally:
                callback.close()
            completed = app.notification("mcpServer/oauthLogin/completed")
            require(completed.get("success") and completed.get("loginId") == native.get("loginId"),
                    "Native app-server OAuth did not complete")
            # A fresh client runtime loads the newly consented credential, without a model turn.
            renewed = app.rpc("thread/start", {"cwd": str(root), "ephemeral": True,
                "modelProvider": "fixture", "model": "fixture", "approvalPolicy": "never", "sandbox": "read-only"})
            renewed_thread = renewed.get("result", {}).get("thread", {}).get("id")
            require(renewed_thread, "Renewed client runtime unavailable")
            app.rpc("mcpServerStatus/list", {"threadId": renewed_thread, "serverName": "devoption", "detail": "toolsAndAuthOnly"})
            content(app.call(renewed_thread, "get_diagnostics", {}))
            new_grants = re.findall(r'name="grant_id" value="([A-Za-z0-9_-]{43})"', browser(ORIGIN + "/mcp/grants").decode())
            require(len(new_grants) == 1, "Expected one renewed grant")
            browser(ORIGIN + "/mcp/grants", urllib.parse.urlencode({"grant_id": new_grants[0]}).encode())
            require(not any(path.startswith("/no-model") for path, _, _ in observed),
                    "Unexpected model-provider request attempted")
            require({method for _, _, method in observed if method} >= {"initialize", "tools/list", "tools/call"},
                    "Missing native Codex MCP protocol coverage")
            stop(app.process)
            subprocess.run([codex, "mcp", "logout", "devoption"], cwd=root, env=env, check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15)
        stop_file.touch()
        require(fixture.wait(timeout=15) == 0, "Rust fixture did not shut down cleanly")
        return {"codex": version, "oauth": "passed", "native_callback": "passed", "tools": sorted(TOOLS), "revocation": "passed",
                "transport": "verified disposable TLS via restricted loopback CONNECT proxy",
                "model_turns": 0, "blocked_external_connect_attempts": len(blocked),
                "global_configuration_changes": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", default="codex", help="Installed Codex executable; no installation performed")
    args = parser.parse_args()
    executable = shutil.which(args.codex)
    require(executable, "Codex executable was not found")
    print(json.dumps(check(executable), indent=2))


if __name__ == "__main__":
    main()
