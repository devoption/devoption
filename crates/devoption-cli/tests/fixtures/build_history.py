"""Disposable startup proof using historical claims, never real build results."""
import fcntl
import hashlib
from pathlib import Path
import selectors
import subprocess
import sys

binary, root = sys.argv[1], Path(sys.argv[2])
project, repository, unrelated = (bytes.fromhex(value) for value in sys.argv[3:6])
owner = root / "owner"
history = root / "history"
history.mkdir(mode=0o700)


def signed(data):
    return data + hashlib.sha256(data).digest()


def summary(states, project_id=project, repository_id=repository):
    records = b""
    for index, state in enumerate(states):
        records += bytes([index + 1]) * 16 + project_id + repository_id
        records += b"a" * 40 + b"s" * 32 + bytes([state]) + bytes(40)
    return signed(b"DVHISTRY\x01" + bytes([len(states)]) + records)


def private_write(path, data):
    path.write_bytes(data)
    path.chmod(0o600)


record = history / "build-history.bin"
private_write(record, summary([0, 1, 8]))
log = history / ("build-output-" + "01" * 16 + ".bin")
log_bytes = signed(b"DVOUTPUT\x01" + b"\x01" * 16 + (1024).to_bytes(4, "big")
                   + (5).to_bytes(4, "big") + bytes(4) + b"\x00" + b"saved")
private_write(log, log_bytes)


def args(data=owner, selected=history):
    result = [binary, "serve", "--port", "0", "--data-dir", str(data)]
    if selected is not None:
        result += ["--build-history-dir", str(selected)]
    return result


def start(selected=history):
    process = subprocess.Popen(args(selected=selected), stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE)
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            assert selector.select(15), "service did not announce startup"
            line = process.stdout.readline()
            assert line.startswith(b"Listening on http://127.0.0.1:"), process.stderr.read() if process.poll() is not None else line
        return process
    except BaseException:
        process.kill()
        process.wait(timeout=5)
        raise


def stop(process):
    process.kill()
    process.wait(timeout=5)
    process.stdout.close()
    process.stderr.close()


def rejected(command):
    result = subprocess.run(command, capture_output=True, timeout=15)
    assert result.returncode == 1, result.stderr
    assert b"Listening" not in result.stdout


# Omitted selection does not touch even unfinished saved history.
process = start(selected=None)
try:
    assert record.read_bytes() == summary([0, 1, 8])
    assert not (history / "build-history.lock").exists()
finally:
    stop(process)

for _ in range(2):
    process = start()
    try:
        assert record.read_bytes() == summary([9, 9, 8])
        assert log.read_bytes() == log_bytes
        # Catalog edits must remain available after startup validation finishes.
        with (owner / ".projects.lock").open("rb") as catalog_lock:
            fcntl.flock(catalog_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        with (history / "build-history.lock").open("rb") as lock:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                pass
            else:
                raise AssertionError("server did not retain history writer")
        rejected(args())
    finally:
        stop(process)
    with (history / "build-history.lock").open("rb") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)

# Every record is checked before recovery, including terminal foreign history.
for invalid in [summary([0], project_id=bytes(16)),
                summary([1], repository_id=bytes(16)),
                summary([0], repository_id=unrelated),
                summary([8], project_id=bytes(16))]:
    private_write(record, invalid)
    rejected(args())
    assert record.read_bytes() == invalid
    assert log.read_bytes() == log_bytes

# A valid unfinished record must not be rewritten before later records validate.
valid = summary([0])
foreign = summary([8], project_id=bytes(16))
second = b"\x02" * 16 + foreign[26:-32]
mixed = signed(b"DVHISTRY\x01\x02" + valid[10:-32] + second)
private_write(record, mixed)
rejected(args())
assert record.read_bytes() == mixed

private_write(record, summary([0]))
with (owner / ".projects.lock").open("rb") as lock:
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    rejected(args())
assert record.read_bytes() == summary([0])
catalog = owner / "projects.bin"
original_catalog = catalog.read_bytes()
private_write(catalog, b"corrupt catalog")
rejected(args())
assert record.read_bytes() == summary([0])
private_write(catalog, original_catalog)
# Missing catalog cannot match nonempty history.
catalog.rename(owner / "saved-catalog")
rejected(args())
assert record.read_bytes() == summary([0])
# Empty history has no catalog identities to validate.
private_write(record, summary([]))
process = start()
try:
    assert record.read_bytes() == summary([])
finally:
    stop(process)
(owner / "saved-catalog").rename(catalog)

# Failures never silently reset history or announce a listener.
private_write(record, b"corrupt")
rejected(args())
assert record.read_bytes() == b"corrupt"
private_write(record, summary([0]))
private_write(history / ".build-history.pending", b"preserve")
rejected(args())
assert record.read_bytes() == summary([0])
assert (history / ".build-history.pending").read_bytes() == b"preserve"

empty_owner = root / "empty-owner"
empty_owner.mkdir(mode=0o700)
unused = root / "unused-history"
unused.mkdir(mode=0o700)
rejected(args(data=empty_owner, selected=unused))
assert list(unused.iterdir()) == []
unsafe = root / "unsafe-history"
unsafe.mkdir(mode=0o755)
unsafe.chmod(0o755)
rejected(args(selected=unsafe))
assert list(unsafe.iterdir()) == []
alias = root / "history-alias"
alias.symlink_to(unused, target_is_directory=True)
rejected(args(selected=alias))
assert list(unused.iterdir()) == []
