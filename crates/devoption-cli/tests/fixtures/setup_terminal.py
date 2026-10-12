"""Public fixture passwords only. Real CLI on isolated controlling PTYs, no shell."""

import errno
import os
from pathlib import Path
import pty
import re
import select
import signal
import subprocess
import sys
import termios
import time
import urllib.parse
import urllib.request


BINARY, ROOT = sys.argv[1], Path(sys.argv[2])
PASSWORD = "  café fixture phrase 🔐  ".encode()
FIRST = b"New owner password (15+ characters): "
CONFIRM = b"Confirm owner password: "


class Setup:
    def __init__(self, path):
        self.fd = None
        self.path = path
        self.transcript = bytearray()
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            before = termios.tcgetattr(0)
            child = subprocess.Popen([BINARY, "setup", "--data-dir", str(path)])
            print(f"CHILD:{child.pid}", flush=True)
            code = child.wait()
            restored = termios.tcgetattr(0) == before
            print(f"RESULT:{code}:{int(restored)}", flush=True)
            os._exit(0)
        self.read_until(b"CHILD:")
        self.read_until(FIRST)
        self.child = int(re.search(rb"CHILD:(\d+)", self.transcript)[1])
        assert not termios.tcgetattr(self.fd)[3] & (termios.ECHO | termios.ECHONL)

    def read_until(self, marker):
        deadline = time.monotonic() + 10
        while marker not in self.transcript:
            if time.monotonic() > deadline:
                self.close()
                raise AssertionError("terminal fixture timed out")
            if not select.select([self.fd], [], [], 0.1)[0]:
                continue
            try:
                data = os.read(self.fd, 8192)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
                data = b""
            if not data:
                self.close()
                raise AssertionError("CLI ended before expected terminal marker")
            self.transcript.extend(data)

    def send(self, data):
        while data:
            data = data[os.write(self.fd, data):]

    def complete(self, first=PASSWORD, confirmation=PASSWORD, success=False):
        self.send(first + b"\n")
        self.read_until(CONFIRM)
        assert not termios.tcgetattr(self.fd)[3] & (termios.ECHO | termios.ECHONL)
        self.send(confirmation + b"\n")
        self.finish(success)

    def finish(self, success=False):
        self.read_until(b"RESULT:")
        self.read_until(b"RESULT:0:1" if success else b"RESULT:1:1")
        assert PASSWORD not in self.transcript
        assert b"$argon2" not in self.transcript
        assert str(self.path).encode() not in self.transcript
        assert (b"Owner created." in self.transcript) == success
        if not success:
            assert not (self.path / "owner.bin").exists()
        self.close()

    def close(self):
        if self.fd is not None:
            os.close(self.fd)
            self.fd = None
            # Closing the controlling PTY hangs up its children. Reap our exact
            # unreaped wrapper PID; never signal an entire process group.
            pid, _ = os.waitpid(self.pid, os.WNOHANG)
            if not pid:
                try:
                    os.kill(self.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                os.waitpid(self.pid, 0)

    def __del__(self):
        self.close()


def fixture(name):
    path = ROOT / name
    path.mkdir(mode=0o700)
    return path


# Byte-exact Unicode/whitespace, line clearing and Unicode backspace. Input is
# passed only through a hidden terminal, never child arguments/environment.
success = fixture("success")
Setup(success).complete(b"discard\x15" + PASSWORD + "🧪".encode() + b"\x7f", success=True)
record = (success / "owner.bin").read_bytes()
assert PASSWORD not in record
existing = subprocess.run([BINARY, "setup", "--data-dir", str(success)], capture_output=True)
assert existing.returncode == 1 and FIRST not in existing.stderr
assert (success / "owner.bin").read_bytes() == record

for name, first, confirmation in [
    ("empty", b"", b""),
    ("short", b"short fixture", b"short fixture"),
    ("mismatch", PASSWORD, b"different fixture phrase"),
]:
    Setup(fixture(name)).complete(first, confirmation)

for name, data in [
    ("invalid_utf8", b"\xff\n"),
    ("control", b"\x00\n"),
    ("interrupt", PASSWORD + b"\x03"),
    ("eof", b"\x04"),
    ("suspend", b"\x1a"),
]:
    session = Setup(fixture(name))
    session.send(data)
    session.finish()

session = Setup(fixture("overlong"))
session.send(b"x" * 1025)
time.sleep(0.2)
assert not termios.tcgetattr(session.fd)[3] & termios.ECHO
session.send(PASSWORD + b"\n")
session.finish()

session = Setup(fixture("confirmation_interrupt"))
session.send(PASSWORD + b"\n")
session.read_until(CONFIRM)
session.send(b"\x03")
session.finish()

for name, sig in [("sigint", signal.SIGINT), ("sigterm", signal.SIGTERM),
                  ("sighup", signal.SIGHUP), ("sigquit", signal.SIGQUIT),
                  ("sigtstp", signal.SIGTSTP)]:
    session = Setup(fixture(name))
    session.send(PASSWORD)
    os.kill(session.child, sig)
    session.finish()

# The account created through the CLI can authenticate after a fresh server
# startup. These HTTP credentials stay in memory and never enter diagnostic output.
server = subprocess.Popen(
    [BINARY, "serve", "--port", "0", "--data-dir", str(success)],
    stdout=subprocess.PIPE, stderr=subprocess.PIPE,
)
try:
    assert select.select([server.stdout], [], [], 10)[0], "server startup timed out"
    address = re.search(rb"http://127\.0\.0\.1:\d+", server.stdout.readline())[0].decode()
    request = urllib.request.Request(address + "/auth/challenge", data=b"", headers={"Origin": address})
    with urllib.request.urlopen(request, timeout=5) as response:
        assert response.status == 204
        cookie = response.headers["Set-Cookie"].split(";", 1)[0]
        csrf = response.headers["X-CSRF-Token"]
    request = urllib.request.Request(
        address + "/auth/sign-in",
        data=urllib.parse.urlencode({"password": PASSWORD.decode()}).encode(),
        headers={"Origin": address, "Cookie": cookie, "X-CSRF-Token": csrf,
                 "Content-Type": "application/x-www-form-urlencoded"},
    )
    with urllib.request.urlopen(request, timeout=5) as response:
        assert response.status == 204
        assert response.headers["Set-Cookie"].startswith("__Host-devoption_session=")
    assert (success / "owner.bin").read_bytes() == record
finally:
    server.kill()
    server.communicate(timeout=5)

print("CLI/HTTP fixtures passed: hidden input, restoration, bounds, cancellation, sign-in")
