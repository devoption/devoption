#!/usr/bin/python3
"""Fixed compiler worker inside the installed systemd build boundary.

stdout carries only the bounded component; stderr carries bounded compiler logs.
Never invoke this directly as an alternative to the broker's isolation.
"""
import os
from pathlib import Path
import selectors
import socket
import stat
import subprocess
import sys
import time

LIMIT = 1048576
ARTIFACT_LIMIT = 4194304


class WorkerFailure(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def read_component_candidate(path):
    if path.resolve(strict=True) != path:
        raise WorkerFailure(126, 'compiler artifact path rejected')
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as source:
        before = os.fstat(source.fileno())
        if not stat.S_ISREG(before.st_mode) or not 8 <= before.st_size <= ARTIFACT_LIMIT:
            raise WorkerFailure(126, 'compiler artifact rejected')
        data = source.read(ARTIFACT_LIMIT + 1)
        after = os.fstat(source.fileno())
        if (len(data) != before.st_size or data[:8] != b'\0asm\x0d\0\x01\0'
                or (before.st_size, before.st_mtime_ns, before.st_ctime_ns)
                != (after.st_size, after.st_mtime_ns, after.st_ctime_ns)):
            raise WorkerFailure(126, 'compiler component rejected')
    return data


def main():
    if os.geteuid() == 0:
        raise RuntimeError("worker requires an unprivileged identity")
    status = Path('/proc/self/status').read_text()
    if 'NoNewPrivs:\t1' not in status or 'CapEff:\t0000000000000000' not in status:
        raise RuntimeError('worker privilege restriction is unavailable')
    group = next(line[3:] for line in Path('/proc/self/cgroup').read_text().splitlines() if line.startswith('0::'))
    cgroup = Path('/sys/fs/cgroup') / group.lstrip('/')
    for name, expected in [('memory.max', '536870912'), ('memory.swap.max', '0'), ('pids.max', '64')]:
        if (cgroup / name).read_text().strip() != expected:
            raise RuntimeError('worker cgroup limit is unavailable')
    quota, period = (cgroup / 'cpu.max').read_text().split()
    if quota == 'max' or int(quota) > int(period):
        raise RuntimeError('worker CPU limit is unavailable')
    for path in ('/source', '/toolchain', '/vendor'):
        if not os.statvfs(path).f_flag & os.ST_RDONLY:
            raise RuntimeError('worker input mount is writable')
    for path, limit in (('/job', 117440512), ('/job/target', 134217728)):
        info = os.statvfs(path)
        if info.f_blocks * info.f_frsize > limit:
            raise RuntimeError('worker scratch limit is unavailable')
    if not os.statvfs('/job').f_flag & os.ST_NOEXEC or os.statvfs('/job/target').f_flag & os.ST_NOEXEC:
        raise RuntimeError('worker executable scratch policy is unavailable')
    for family in (socket.AF_INET, socket.AF_INET6, socket.AF_UNIX):
        try:
            connection = socket.socket(family)
        except OSError:
            continue
        connection.close()
        raise RuntimeError("worker network restriction is unavailable")
    root = Path('/job')
    for name in ('home', 'cargo', 'tmp', 'target'):
        (root / name).mkdir(mode=0o700, exist_ok=True)
    (root / 'cargo/config.toml').write_text(
        '[source.crates-io]\nreplace-with="prepared"\n'
        '[source.prepared]\ndirectory="/vendor"\n')
    environment = {
        'PATH': '/toolchain/bin:/usr/bin', 'RUSTC': '/toolchain/bin/rustc',
        'HOME': '/job/home', 'CARGO_HOME': '/job/cargo',
        'CARGO_TARGET_DIR': '/job/target', 'TMPDIR': '/job/tmp',
        'LANG': 'C.UTF-8', 'CARGO_NET_OFFLINE': 'true',
        'CARGO_INCREMENTAL': '0', 'CARGO_BUILD_JOBS': '1',
        'RUSTC_WRAPPER': '', 'RUSTC_WORKSPACE_WRAPPER': '',
    }
    command = ['/toolchain/bin/cargo', 'build', '--release', '--locked', '--offline',
               '--target', 'wasm32-wasip2', '--package', 'devoption-web-fixture', '--lib']
    child = subprocess.Popen(command, cwd='/source', env=environment,
                             stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT)
    selector = selectors.DefaultSelector()
    selector.register(child.stdout, selectors.EVENT_READ)
    retained = 0
    deadline = time.monotonic() + 55
    try:
        while selector.get_map():
            if time.monotonic() >= deadline:
                raise WorkerFailure(124, 'compiler deadline reached')
            for key, _ in selector.select(0.1):
                chunk = os.read(key.fileobj.fileno(), 8192)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                available = max(0, LIMIT - retained)
                sys.stderr.buffer.write(chunk[:available])
                sys.stderr.buffer.flush()
                retained += len(chunk)
                if retained >= LIMIT:
                    raise WorkerFailure(125, 'compiler output limit reached')
        if child.wait(timeout=max(0.1, deadline - time.monotonic())) != 0:
            return 1
        artifact = root / 'target/wasm32-wasip2/release/devoption_web_fixture.wasm'
        data = read_component_candidate(artifact)
        # The supervisor retains an independent copy only after complete cgroup stop.
        sys.stdout.buffer.write(data)
        sys.stdout.buffer.flush()
        return 0
    finally:
        if child.poll() is None:
            child.kill()
            child.wait()
        selector.close()


if __name__ == '__main__':
    try:
        sys.exit(main())
    except WorkerFailure as failure:
        print(str(failure), file=sys.stderr)
        sys.exit(failure.code)
    except subprocess.TimeoutExpired:
        print('compiler deadline reached', file=sys.stderr)
        sys.exit(124)
    except Exception:
        print('Build worker failed; inspect compiler output and service result.', file=sys.stderr)
        sys.exit(1)
