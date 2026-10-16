"""Public synthetic historical claims for tests/screenshots, never build evidence."""
import hashlib
from pathlib import Path
import sys

root = Path(sys.argv[1])
project, repo, other_project, other_repo = map(bytes.fromhex, sys.argv[2:6])
def signed(data):
    return data + hashlib.sha256(data).digest()
def save(name, data):
    path = root / name
    path.write_bytes(signed(data))
    path.chmod(0o600)
records = b""
for n, state in [(1, 0), (2, 4), (3, 2), (4, 8)]:
    p, r = (other_project, other_repo) if n == 4 else (project, repo)
    artifact = b"a" * 32 + (123).to_bytes(8, "big") if state == 2 else bytes(40)
    records += bytes([n]) * 16 + p + r + b"a" * 40 + b"s" * 32 + bytes([state]) + artifact
save("build-history.bin", b"DVHISTRY\x01\x04" + records)
for n, stdout, stderr, truncated in [
    (1, b"   Compiling example-service v0.1.0\n   Checking source snapshot\n", b"Supervisor connection lost before completion.\n", False),
    (2, b"<script>fixture-only</script>\x1b[31m\n", "error: fixture rejected\u202e\n".encode(), True),
]:
    limit = len(stdout) + len(stderr) if truncated else 1024
    save("build-output-" + (bytes([n]) * 16).hex() + ".bin",
         b"DVOUTPUT\x01" + bytes([n]) * 16 + limit.to_bytes(4, "big")
         + len(stdout).to_bytes(4, "big") + len(stderr).to_bytes(4, "big")
         + bytes([truncated]) + stdout + stderr)
