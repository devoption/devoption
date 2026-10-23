"""Build and serve a named public app on an explicitly disposable Linux host.

Uses the installed build broker/worker and a locally compiled trusted runtime.
Creates temporary root-owned build helpers/configuration and systemd units. Never
run on an existing installation or workstation. No owner, secrets, DNS or public
listeners are created. Dependency acquisition precedes the offline sandbox build.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

from check_bundle_runtime import smoke
from prepare_build_config import verify_vendor
from release_plan import workspace

CONFIG = Path('/etc/devoption/build.toml')
BROKER = Path('/usr/local/libexec/devoption-build-broker')
WORKER = Path('/usr/local/libexec/devoption-build-worker')


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, capture_output=True,
                          timeout=120, **kwargs).stdout


def check(repository, runtime):
    if sys.platform != 'linux' or os.geteuid() == 0:
        raise ValueError('Run as an unprivileged user on a disposable Linux systemd host.')
    paths = (CONFIG, BROKER, WORKER)
    if any(path.exists() or path.is_symlink() for path in paths):
        raise ValueError('Existing build installation found; no files changed.')
    if run('systemctl', 'show', 'devoption-build.service', '-p', 'LoadState', '--value').strip() != b'not-found':
        raise ValueError('Existing build unit found; no files changed.')
    version, _ = workspace(repository)
    toolchain = Path(run('rustc', '--print', 'sysroot').decode().strip()).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='devoption-named-app-') as directory:
        root = Path(directory).resolve()
        for name in ('jobs', 'data', 'repositories', 'history'):
            (root/name).mkdir(mode=0o700)
        token = 'ab' * 16
        job = root/'jobs'/token
        job.mkdir(mode=0o700)
        source = job/'source'
        shutil.copytree(repository/'examples/static-site', source)
        # Acquisition is separate from the fixed offline worker; verify original
        # archives against Cargo.lock before admitting the vendor tree.
        cargo_home = root/'cargo-cache'
        environment = os.environ | {'CARGO_HOME': str(cargo_home)}
        run('cargo', 'vendor', '--locked', '--manifest-path', source/'Cargo.toml', root/'vendor',
            env=environment)
        archives = list((cargo_home/'registry/cache').glob('*'))
        if len(archives) != 1:
            raise ValueError('Expected one crates.io archive cache.')
        verify_vendor(source/'Cargo.lock', root/'vendor', archives[0])
        values = {'uid': os.geteuid(), 'work': str(root/'jobs'),
                  'owner_data': str(root/'data'), 'repositories': str(root/'repositories'),
                  'history': str(root/'history'), 'toolchain': str(toolchain), 'vendor': str(root/'vendor')}
        config = root/'build.toml'
        config.write_text(''.join(f'{key} = {json.dumps(value)}\n' for key, value in values.items()))
        installed = []
        config_directory_existed = CONFIG.parent.exists()
        try:
            run('sudo', '-n', 'install', '-d', '-m755', CONFIG.parent, BROKER.parent)
            for original, destination, mode in [(config, CONFIG, '644'),
                    (repository/'scripts/build_broker.py', BROKER, '755'),
                    (repository/'scripts/build_worker.py', WORKER, '755')]:
                run('sudo', '-n', 'install', '-m'+mode, original, destination)
                installed.append(destination)
            assets = [(url, kind, (source/'assets'/filename).read_bytes()) for url, kind, filename in
                      [('/assets/site.css', 'text/css; charset=utf-8', 'site.css'),
                       ('/assets/site.js', 'text/javascript; charset=utf-8', 'site.js')]]
            saved = []
            for revision in ('A', 'B'):
                if revision == 'B':
                    path = source/'assets/index.html'
                    path.write_text(path.read_text().replace('revision A', 'revision B'))
                    path = source/'src/lib.rs'
                    path.write_text(path.read_text().replace('"ok A"', '"ok B"'))
                before = {str(path.relative_to(source)): path.read_bytes()
                          for path in source.rglob('*') if path.is_file()}
                component = run('sudo', '-n', BROKER, 'run', token)
                after = {str(path.relative_to(source)): path.read_bytes()
                         for path in source.rglob('*') if path.is_file()}
                if before != after or not component.startswith(b'\0asm\x0d\0\x01\0'):
                    raise ValueError('Build changed source or returned an invalid component.')
                artifact = root/f'{revision}.wasm'
                artifact.write_bytes(component)
                saved.append(artifact)
                smoke(runtime, artifact, version, revision=revision, assets=assets)
                print(f'Named app {revision}: {len(component)} bytes, SHA-256 {hashlib.sha256(component).hexdigest()}', flush=True)
            if saved[0].read_bytes() == saved[1].read_bytes():
                raise ValueError('Asset/source update did not change the component.')
            smoke(runtime, saved[0], version, assets=assets)
            print('Named app offline sandbox builds, exact assets, A/B/A runtime and source preservation passed.')
        finally:
            if BROKER in installed:
                run('sudo', '-n', BROKER, 'stop')
            for path in reversed(installed):
                run('sudo', '-n', 'rm', '--', path)
            if not config_directory_existed and CONFIG.parent.exists():
                run('sudo', '-n', 'rmdir', CONFIG.parent)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--disposable-system-host', action='store_true', required=True)
    parser.add_argument('--repository', type=Path, default=Path.cwd())
    parser.add_argument('--runtime', type=Path, required=True)
    args = parser.parse_args()
    check(args.repository.resolve(strict=True), args.runtime.resolve(strict=True))
