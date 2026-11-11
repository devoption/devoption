"""Exercise the library runner API on an explicitly disposable Linux systemd host.

Installs temporary root-owned helpers/config; refuses an existing installation.
No account, credential, registry, public listener or package publication is used.
"""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

from check_named_web_app import CONFIG, BROKER, WORKER, run


def consume_archive(archive, root, original, toolchain):
    expected = {str(path.relative_to(original)): path.read_bytes()
                for path in original.rglob('*') if path.is_file()}
    destination = root / 'imported'
    destination.mkdir(mode=0o700)
    found = {}
    with tarfile.open(archive, mode='r:') as package:
        for member in package:
            prefix = 'reusable-fixture-0.1.0/'
            if (not member.isfile() or not member.name.startswith(prefix)
                    or member.size > 512 * 1024 or len(found) >= 128):
                raise ValueError('Unsupported library source archive.')
            relative = member.name[len(prefix):]
            if relative not in expected or relative in found:
                raise ValueError('Unexpected library source archive member.')
            found[relative] = package.extractfile(member).read()
    if found != expected:
        raise ValueError('Library source archive differs from selected committed bytes.')
    for relative, content in found.items():
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content)
    consumer = root / 'consumer'
    (consumer / 'src').mkdir(parents=True)
    (consumer / 'Cargo.toml').write_text(
        '[package]\nname="library-consumer"\nversion="0.1.0"\nedition="2024"\nbuild=false\n'
        '[dependencies]\nreusable-fixture={path="../imported", version="=0.1.0"}\n')
    (consumer / 'src/main.rs').write_text('fn main() { assert_eq!(reusable_fixture::answer(), 42); println!("consumer: 42"); }\n')
    environment = {'PATH': str(toolchain / 'bin') + ':/usr/bin:/bin',
                   'HOME': str(root), 'CARGO_HOME': str(root / 'consumer-cargo'),
                   'CARGO_TARGET_DIR': str(root / 'consumer-target'), 'CARGO_NET_OFFLINE': 'true'}
    cargo = toolchain / 'bin/cargo'
    run(cargo, 'generate-lockfile', '--offline', cwd=consumer, env=environment)
    output = run(cargo, 'run', '--release', '--locked', '--offline', cwd=consumer, env=environment)
    if output.strip() != b'consumer: 42':
        raise ValueError('Separate native consumer behavior failed.')
    run(cargo, 'build', '--release', '--locked', '--offline', '--target', 'wasm32-wasip2',
        cwd=consumer, env=environment)
    print('Verified source archive consumed by a separate native/WASIp2 project; '
          'path wiring is a disposable Cargo fixture, not platform dependency admission.', flush=True)


def check(repository, binary):
    if sys.platform != 'linux' or os.geteuid() == 0:
        raise ValueError('Run as an unprivileged user on a disposable Linux systemd host.')
    if any(path.exists() or path.is_symlink() for path in (CONFIG, BROKER, WORKER)):
        raise ValueError('Existing build installation found; no files changed.')
    if run('systemctl', 'show', 'devoption-build.service', '-p', 'LoadState', '--value').strip() != b'not-found':
        raise ValueError('Existing build unit found; no files changed.')
    toolchain = Path(run('rustc', '--print', 'sysroot').decode().strip()).resolve(strict=True)
    fixture = repository / 'crates/devoption-builds/tests/fixtures/reusable-library'
    with tempfile.TemporaryDirectory(prefix='devoption-library-') as directory:
        root = Path(directory).resolve()
        for name in ('jobs', 'data', 'repositories', 'history', 'vendor', 'archives'):
            (root / name).mkdir(mode=0o700)
        source = root / 'repositories/library'
        shutil.copytree(fixture, source)
        git = ['git', '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false',
               '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid']
        git_env = os.environ | {'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_CONFIG_NOSYSTEM': '1'}
        run(*git, 'init', '-q', cwd=source, env=git_env)
        run(*git, 'add', '--all', cwd=source, env=git_env)
        run(*git, 'commit', '-qm', 'Trusted library fixture', cwd=source, env=git_env)
        config = run(sys.executable, repository / 'scripts/prepare_build_config.py',
                     '--devoption', binary, '--toolchain', toolchain, '--vendor', root / 'vendor',
                     '--archives', root / 'archives', '--lockfile', source / 'Cargo.lock',
                     '--work', root / 'jobs', '--owner-data', root / 'data',
                     '--repositories', root / 'repositories', '--history', root / 'history')
        config_path = root / 'build.toml'
        config_path.write_bytes(config)
        installed = []
        config_directory_existed = CONFIG.parent.exists()
        try:
            run('sudo', '-n', 'install', '-d', '-m755', CONFIG.parent, BROKER.parent)
            for original, destination, mode in [(config_path, CONFIG, '644'),
                    (repository / 'scripts/build_broker.py', BROKER, '755'),
                    (repository / 'scripts/build_worker.py', WORKER, '755')]:
                run('sudo', '-n', 'install', '-m' + mode, original, destination)
                installed.append(destination)
            environment = os.environ | {'DEVOPTION_LIBRARY_ACCEPTANCE_ROOT': str(root)}
            result = subprocess.run(['cargo', 'test', '-p', 'devoption-builds', '--test',
                                     'library_execution', '--release', '--locked', '--offline',
                                     '--', '--ignored', '--nocapture'], cwd=repository,
                                    env=environment, timeout=180)
            result.check_returncode()
            from check_library_http import check as check_http
            check_http(binary, root, fixture, repository)
            archive = root / 'verified-source.tar'
            consume_archive(archive, root, fixture, toolchain)
            print(f'Library source package: {archive.stat().st_size} bytes, '
                  f'SHA-256 {hashlib.sha256(archive.read_bytes()).hexdigest()}', flush=True)
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
    parser.add_argument('--binary', type=Path, required=True)
    args = parser.parse_args()
    check(args.repository.resolve(strict=True), args.binary.resolve(strict=True))
