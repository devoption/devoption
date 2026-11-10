#!/usr/bin/python3
"""Fixed compiler worker inside the installed systemd build boundary.

stdout carries only a bounded component or fixed library completion record; stderr carries bounded compiler logs.
Never invoke this directly as an alternative to the broker's isolation.
"""
import os
from pathlib import Path
import re
import selectors
import socket
import stat
import subprocess
import sys
import time
import tomllib
from urllib.parse import urlsplit

LIMIT = 1048576
ARTIFACT_LIMIT = 4194304


class WorkerFailure(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def target_identity(manifest, kind):
    """Select only a conventional library from the already reviewed source.

    The Rust snapshot validator owns full manifest/lock/source admission. Repeat
    the target shape checks here so this worker never selects another target or
    accepts an argument/path supplied outside the read-only source mount.
    """
    if len(manifest) > 65536:
        raise WorkerFailure(126, 'compiler manifest rejected')
    try:
        value = tomllib.loads(manifest.decode('utf-8'))
        package, library = value['package'], value['lib']
        name, target = package['name'], library['name']
        if (set(value) - {'package', 'lib', 'dependencies'}
                or set(package) != {'name', 'version', 'edition', 'build'}
                or set(library) != {'name', 'crate-type'}
                or not isinstance(name, str) or not isinstance(target, str)
                or not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_-]{0,127}', name)
                or not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]{0,127}', target)
                or target != name.replace('-', '_')
                or library['crate-type'] != [kind]
                or package['build'] is not False
                or package['edition'] not in ('2021', '2024')):
            raise ValueError('unsupported target')
    except (KeyError, TypeError, ValueError, UnicodeError):
        raise WorkerFailure(126, 'compiler manifest rejected') from None
    return name, target


def build_target(manifest):
    name, target = target_identity(manifest, 'cdylib')
    command = ['/toolchain/bin/cargo', 'build', '--release', '--locked', '--offline',
               '--target', 'wasm32-wasip2', '--package', name, '--lib']
    artifact = Path('wasm32-wasip2/release') / f'{target}.wasm'
    return command, artifact


LIBRARY_COMPLETION = b'devoption-library-build-v1\n'


def library_commands(manifest):
    name, _ = target_identity(manifest, 'rlib')
    def fixed(operation):
        return ['/toolchain/bin/cargo', operation, '--release', '--locked', '--offline',
                '--package', name, '--lib']
    return [fixed('build'), fixed('test'), fixed('build') + ['--target', 'wasm32-wasip2']]


def selected_commands(manifest):
    try:
        kind = tomllib.loads(manifest.decode('utf-8'))['lib']['crate-type']
    except (KeyError, TypeError, ValueError, UnicodeError):
        raise WorkerFailure(126, 'compiler manifest rejected') from None
    if kind == ['rlib']:
        return library_commands(manifest), None
    command, artifact = build_target(manifest)
    return [command], artifact


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


COMPILER_SECONDS = 55


def run_compilers(commands, environment, source_directory='/source'):
    """Internal fixed-command runner, with a shared wall time and output budget."""
    retained = 0
    deadline = time.monotonic() + COMPILER_SECONDS
    for command in commands:
        if time.monotonic() >= deadline:
            raise WorkerFailure(124, 'compiler deadline reached')
        child = subprocess.Popen(command, cwd=source_directory, env=environment,
                                 stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                 stderr=subprocess.STDOUT)
        selector = selectors.DefaultSelector()
        selector.register(child.stdout, selectors.EVENT_READ)
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
                return False
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()
            selector.close()
            child.stdout.close()
    return True


def dependency_config(data):
    """Accept only direct offline source replacements emitted by the controller."""
    try:
        if len(data) > 65536:
            raise ValueError('configuration limit')
        value = tomllib.loads(data.decode('utf-8'))
        if set(value) - {'source', 'registries'}:
            raise ValueError('unsupported configuration')
        sources, registries = value.get('source', {}), value.get('registries', {})
        if not isinstance(sources, dict) or not isinstance(registries, dict) or len(sources) > 66:
            raise ValueError('unsupported sources')
        directories = set()
        for name, source in sources.items():
            if not re.fullmatch(r'[a-zA-Z0-9_-]{1,128}', name) or not isinstance(source, dict):
                raise ValueError('unsupported source')
            if set(source) == {'directory'}:
                directory = source['directory']
                if not isinstance(directory, str) or not re.fullmatch(r'/vendor/[a-zA-Z0-9_-]+', directory):
                    raise ValueError('unsupported directory')
                if directory in directories:
                    raise ValueError('duplicate directory')
                directories.add(directory)
            else:
                if set(source) != ({'replace-with'} if name == 'crates-io' else {'registry', 'replace-with'}):
                    raise ValueError('unsupported replacement')
                replacement = sources.get(source['replace-with'])
                if not isinstance(replacement, dict) or set(replacement) != {'directory'}:
                    raise ValueError('replacement must terminate at a directory')
                if name != 'crates-io':
                    index = source['registry']
                    parsed = urlsplit(index.removeprefix('sparse+'))
                    if (parsed.scheme != 'https' or not parsed.hostname or parsed.username
                            or parsed.password or parsed.query or parsed.fragment
                            or registries.get(name) != {'index': index}):
                        raise ValueError('unsupported registry')
        if set(registries) != {n for n,s in sources.items() if 'registry' in s}:
            raise ValueError('unbound registry')
        return data.decode('utf-8')
    except (KeyError, TypeError, ValueError, UnicodeError):
        raise WorkerFailure(126, 'compiler dependency configuration rejected') from None


def selected_dependency_config(vendor=Path('/vendor')):
    path = vendor / 'cargo-config.toml'
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    except FileNotFoundError:
        return ('[source.crates-io]\nreplace-with="prepared"\n'
                '[source.prepared]\ndirectory="/vendor"\n')
    with os.fdopen(descriptor, 'rb') as source:
        metadata = os.fstat(source.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1 or metadata.st_size > 65536:
            raise WorkerFailure(126, 'compiler dependency configuration rejected')
        return dependency_config(source.read(65537))


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
    (root / 'cargo/config.toml').write_text(selected_dependency_config())
    environment = {
        'PATH': '/toolchain/bin:/usr/bin', 'RUSTC': '/toolchain/bin/rustc',
        'HOME': '/job/home', 'CARGO_HOME': '/job/cargo',
        'CARGO_TARGET_DIR': '/job/target', 'TMPDIR': '/job/tmp',
        'LANG': 'C.UTF-8', 'CARGO_NET_OFFLINE': 'true',
        'CARGO_INCREMENTAL': '0', 'CARGO_BUILD_JOBS': '1',
        'RUSTC_WRAPPER': '', 'RUSTC_WORKSPACE_WRAPPER': '',
    }
    with Path('/source/Cargo.toml').open('rb') as source:
        commands, artifact_path = selected_commands(source.read(65537))
    if not run_compilers(commands, environment):
        return 1
    data = (read_component_candidate(root / 'target' / artifact_path)
            if artifact_path is not None else LIBRARY_COMPLETION)
    # The supervisor retains output only after complete cgroup stop. For a
    # library it packages its own verified snapshot, never compiler scratch.
    sys.stdout.buffer.write(data)
    sys.stdout.buffer.flush()
    return 0


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
