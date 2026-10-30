"""Real unified-installer acceptance in a fresh private QEMU guest.

Host mode prepares only public test inputs. Guest mode is destructive and requires
the explicit marker installed in this runner's fresh VM. No production state,
host credentials, SSH keys, forwarded ports or host trust changes are used.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time

from check_upgrade_reboot import IMAGE_SHA256, image

ROOT = Path(__file__).resolve().parents[1]
GUEST = Path('/srv/devoption-install-proof')
CASES = ['plan-is-read-only', 'existing-install-refused', 'apply-and-enroll',
         'https-sign-in', 'managed-repository', 'approved-build-and-deploy',
         'retry-preserves-data-and-processes', 'changed-file-preserved',
         'changed-input-refused', 'restart-recovers-app', 'stopped-intent-persists']


def run(*arguments, timeout=240, failure=False):
    result = subprocess.run([str(x) for x in arguments], capture_output=True, timeout=timeout)
    if (result.returncode == 0) == failure:
        raise ValueError('Acceptance command returned unexpected status: ' + str(arguments[0]) + '\n' +
                         result.stderr.decode(errors='replace')[-4096:])
    return result.stdout


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare(output, binary, runtime, caddy_archive):
    if sys.platform != 'linux' or os.geteuid() == 0:
        raise ValueError('Prepare as an unprivileged user with the pinned Linux toolchain.')
    # mkdir without exist_ok prevents replacing prior inputs or accepting stale evidence.
    output.mkdir(mode=0o755)
    source = ROOT / 'examples/static-site'
    vendor = output / 'vendor'
    run('cargo', 'vendor', '--locked', '--manifest-path', source / 'Cargo.toml', vendor)
    import tomllib
    lock = tomllib.loads((source / 'Cargo.lock').read_text())
    cache = Path(os.environ.get('CARGO_HOME', str(Path.home() / '.cargo'))) / 'registry/cache'
    archives = output / 'archives'; archives.mkdir()
    for package in lock['package']:
        if 'source' not in package:
            continue
        filename = package['name'] + '-' + package['version'] + '.crate'
        matches = list(cache.glob('*/' + filename))
        if len(matches) != 1 or digest(matches[0]) != package['checksum']:
            raise ValueError('Expected one checksum-matching original crate archive: ' + filename)
        shutil.copyfile(matches[0], archives / filename)
    sysroot = Path(run('rustc', '--print', 'sysroot').decode().strip()).resolve(strict=True)
    toolchain = output / 'toolchain'; (toolchain / 'bin').mkdir(parents=True)
    for name in ('rustc', 'cargo'):
        shutil.copy2(sysroot / 'bin' / name, toolchain / 'bin' / name)
    shutil.copytree(sysroot / 'lib', toolchain / 'lib', symlinks=False,
                    ignore=shutil.ignore_patterns('src'))
    for original, name in ((binary, 'devoption'), (runtime, 'devoption-runtime'),
                           (caddy_archive, 'caddy.tar.gz'), (source / 'Cargo.lock', 'Cargo.lock')):
        shutil.copyfile(original, output / name)
    (output / 'devoption').chmod(0o755)
    (output / 'devoption-runtime').chmod(0o755)
    repository = output / 'repository'; repository.mkdir()
    shutil.copyfile(ROOT / 'Cargo.toml', repository / 'Cargo.toml')
    for directory in ('scripts', 'examples/systemd', 'examples/caddy', 'examples/static-site'):
        target = repository / directory; target.mkdir(parents=True)
        for path in (ROOT / directory).rglob('*'):
            if path.is_file() and (directory != 'scripts' or path.suffix == '.py'):
                destination = target / path.relative_to(ROOT / directory)
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, destination)
    from prepare_build_config import verify_vendor
    verify_vendor(output / 'Cargo.lock', vendor, archives)
    metadata = {'source_commit': run('git', '-C', ROOT, 'rev-parse', 'HEAD').decode().strip(),
                'binary_sha256': digest(binary), 'runtime_sha256': digest(runtime),
                'lock_sha256': digest(output / 'Cargo.lock')}
    for key in ('toolchain', 'vendor'):
        metadata[key + '_sha256'] = run(binary, 'build-input-digest', output / key).decode().strip()
    (output / 'metadata.json').write_text(json.dumps(metadata))
    print('Prepared reviewed binaries, bounded toolchain, authenticated vendor tree and public fixture sources.', flush=True)


def require_guest():
    if (os.geteuid() != 0 or os.environ.get('DEVOPTION_DISPOSABLE_APP_INSTALL') != '1' or
            not Path('/run/devoption-app-install-guest').is_file()):
        raise ValueError('Requires the fresh disposable VM and explicit guest marker.')


def persistent_snapshot():
    # Include identity, permissions and inode as well as bytes. No read counters
    # or filesystem access times are compared; observation may change those.
    result = {}
    for root in ('/var/lib/devoption', '/var/lib/devoption-app', '/etc/devoption'):
        for path in sorted(Path(root).rglob('*')):
            info = path.lstat()
            if path.is_symlink():
                raise ValueError('Unexpected symlink in disposable installation state.')
            result[str(path)] = [info.st_ino, info.st_uid, info.st_gid, info.st_mode,
                                 digest(path) if path.is_file() else None]
    return result


def guest():
    require_guest()
    from check_bundle_installer import enroll_command
    from check_bundle_upgrade import HOSTNAME, PASSWORD, request, session
    from install_app import AppHost
    from install_bundle import STATE, PLATFORM, PROXY
    host = AppHost()
    metadata = json.loads((GUEST / 'metadata.json').read_text())
    installer = GUEST / 'repository/scripts/install_app.py'
    selected = GUEST / 'installation.toml'
    config = dict(schema_version=1, hostname=HOSTNAME, **metadata,
                  binary=str(GUEST / 'devoption'), runtime=str(GUEST / 'devoption-runtime'),
                  caddy_archive=str(GUEST / 'caddy.tar.gz'), toolchain=str(GUEST / 'toolchain'),
                  vendor=str(GUEST / 'vendor'), lockfile=str(GUEST / 'Cargo.lock'), archives=str(GUEST / 'archives'))
    selected.write_text(''.join(f'{key} = {json.dumps(value)}\n' for key, value in config.items()))
    commands = lambda mode: ['/usr/bin/python3', str(installer), mode, '--config', str(selected)]
    invoke = lambda mode: json.loads(run(*commands(mode), timeout=300))
    completed = []
    def passed(case):
        completed.append(case)
        print('Passed: ' + case, flush=True)

    assert host.account('devoption') is None and host.account('caddy') is None
    for path in (STATE, PLATFORM, PROXY, '/opt/devoption', '/etc/devoption', '/etc/caddy'):
        assert not Path(path).exists()
    plan = invoke('plan')
    assert not plan['resumed'] and plan['next_action'] == 'apply'
    assert host.account('devoption') is None and host.account('caddy') is None
    assert not Path(STATE).exists() and not Path(PLATFORM).exists()
    passed('plan-is-read-only')
    foreign = Path('/etc/devoption'); foreign.mkdir()
    sentinel = foreign / 'preserve-existing'
    sentinel.write_bytes(b'disposable preexisting installation sentinel')
    run(*commands('apply'), failure=True)
    assert sentinel.read_bytes() == b'disposable preexisting installation sentinel'
    assert host.account('devoption') is None and not Path(STATE).exists()
    sentinel.unlink(); foreign.rmdir()  # Only this test's sentinel, before any installation.
    passed('existing-install-refused')
    assert invoke('apply')['missing_files'] == []
    enroll_command(commands('enroll'), password=PASSWORD, timeout=300)
    owner = Path(PLATFORM + '/data/owner.bin').read_bytes()
    assert owner and invoke('status')['owner_enrolled']
    passed('apply-and-enroll')

    # Fixture-only proxy override: production Caddyfile bytes remain untouched.
    # Test CA is private to this guest; no real DNS, ACME or host trust is used.
    proxy = GUEST / 'fixture.Caddyfile'
    proxy.write_text('''{
    admin off
    auto_https disable_redirects
    skip_install_trust
    servers {
        strict_sni_host on
    }
}
''' + HOSTNAME + ''' {
    bind 127.0.0.1
    tls internal
    reverse_proxy 127.0.0.1:3000 {
        header_up -Forwarded
        header_up -X-Forwarded-*
        header_up -X-Real-IP
    }
}
''')
    dropins = Path('/etc/systemd/system/caddy-devoption.service.d'); dropins.mkdir()
    from release_bundle import CADDY_VERSION
    (dropins / 'fixture.conf').write_text(f'''[Service]
ExecStart=
ExecStart=/opt/devoption/proxy/caddy-{CADDY_VERSION} run --config {proxy} --adapter caddyfile
IPAddressDeny=any
IPAddressAllow=localhost
''')
    host.reload()
    invoke('start')
    deadline = time.monotonic() + 60
    while True:
        try:
            if request('/sign-in')[0] == 200:
                break
        except OSError:
            pass
        if time.monotonic() > deadline:
            raise ValueError('Private test-CA HTTPS did not become ready.')
        time.sleep(.2)
    token, cookie = session()
    passed('https-sign-in')
    def mutate(path, body='', expected=204):
        nonlocal token
        code, _, token, _ = request('/auth/session', 'POST', cookie=cookie)
        assert code == 204
        code, content, _, _ = request(path, 'POST', body, token, cookie, timeout=95)
        if code != expected:
            raise ValueError('Fixture mutation failed: ' + path + ' status ' + str(code) + ' ' + content[:256].decode(errors='replace'))
    mutate('/projects', 'Unified installer fixture')
    project, = set(re.findall(rb'href="/projects/([0-9a-f]{32})"', request('/projects', cookie=cookie)[1]))
    project = project.decode()
    mutate(f'/projects/{project}/repositories/create', 'app')
    repositories = Path(PLATFORM + '/repositories')
    checkouts = list(repositories.glob('*/.git'))
    assert len(checkouts) == 1
    checkout = checkouts[0].parent
    source = GUEST / 'repository/examples/static-site'
    for path in source.rglob('*'):
        if path.is_file():
            target = checkout / path.relative_to(source)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, target)
    uid, gid = host.account('devoption')
    for path in checkout.rglob('*'):
        os.chown(path, uid, gid, follow_symlinks=False)
    host.run('runuser', '-u', 'devoption', '--', 'git', '-C', str(checkout), 'add', '.')
    host.run('runuser', '-u', 'devoption', '--', 'git', '-C', str(checkout), '-c', 'user.name=Fixture',
             '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture app')
    passed('managed-repository')
    page = request(f'/projects/{project}/builds', cookie=cookie)[1]
    repository = re.search(rb'/builds/([0-9a-f]{32})', page)[1].decode()
    base = f'/projects/{project}/builds/{repository}'
    mutate(base + '/prepare')
    page = request(base + '/run', cookie=cookie)[1]
    approval = re.search(rb'data-build-action="approve" data-token="([0-9a-f]+)"', page)[1].decode()
    mutate(base + '/approve', approval, 202)
    deadline = time.monotonic() + 150
    while True:
        page = request(f'/projects/{project}/releases', cookie=cookie)[1]
        jobs = re.findall(rb'data-deploy-action="promote" data-value="([0-9a-f]{32})"', page)
        if jobs:
            break
        if time.monotonic() > deadline:
            history = request(f'/projects/{project}/builds', cookie=cookie)[1]
            for attempt in set(re.findall(rb'/projects/[0-9a-f]{32}/builds/attempts/[0-9a-f]{32}', history)):
                details = request(attempt.decode(), cookie=cookie)[1]
                import html
                print(html.unescape(re.sub('<[^>]+>', ' ', details.decode(errors='replace')))[-8192:], flush=True)
            raise ValueError('Installed offline build did not succeed; saved attempt output is above.')
        time.sleep(.25)
    mutate(f'/projects/{project}/releases/promote', jobs[0].decode())
    status = lambda: json.loads(host.run('runuser', '-u', 'devoption', '--', '/usr/bin/sudo', '-n',
                                       '/usr/local/libexec/devoption-deployment-broker', 'status').stdout)
    release, = status()['releases']
    mutate(f'/projects/{project}/releases/prepare', release['release'] + '\nok A')
    page = request(f'/projects/{project}/releases', cookie=cookie)[1]
    approval = re.search(rb'data-deploy-action="approve" data-value="([0-9a-f]+)"', page)[1].decode()
    mutate(f'/projects/{project}/releases/approve', approval)
    assert status()['deployment']['observed'] == 'healthy'
    passed('approved-build-and-deploy')
    processes = {unit: host.run('systemctl', 'show', unit, '-p', 'InvocationID', '--value').stdout
                 for unit in ('devoption-public.service', 'caddy-devoption.service', 'devoption-app.service')}
    before = persistent_snapshot()
    assert invoke('apply')['missing_files'] == []
    assert invoke('enroll')['owner_enrolled']
    assert persistent_snapshot() == before and Path(PLATFORM + '/data/owner.bin').read_bytes() == owner
    for unit, identity in processes.items():
        assert identity.strip() and host.run('systemctl', 'show', unit, '-p', 'InvocationID', '--value').stdout == identity
    passed('retry-preserves-data-and-processes')
    edited = Path('/etc/devoption/build.toml')
    original = edited.read_bytes()
    edited.write_bytes(original + b'\n# disposable operator edit\n')
    changed = persistent_snapshot()
    run(*commands('apply'), failure=True)
    assert persistent_snapshot() == changed
    edited.write_bytes(original)  # Restore only the test's known byte edit.
    passed('changed-file-preserved')
    input_file = GUEST / 'devoption-runtime'
    original = input_file.read_bytes()
    input_file.write_bytes(b'changed disposable input')
    before = persistent_snapshot()
    run(*commands('apply'), failure=True)
    assert persistent_snapshot() == before
    input_file.write_bytes(original)
    passed('changed-input-refused')
    host.run('systemctl', 'restart', 'devoption-public.service')
    host.healthy(HOSTNAME)
    token, cookie = session()
    assert b'Unified installer fixture' in request('/projects', cookie=cookie)[1]
    assert status()['deployment']['observed'] == 'healthy'
    assert Path(PLATFORM + '/data/owner.bin').read_bytes() == owner
    passed('restart-recovers-app')
    mutate(f'/projects/{project}/releases/stop')
    host.run('systemctl', 'restart', 'devoption-public.service')
    host.healthy(HOSTNAME)
    assert status()['deployment']['intent'] == 'stopped'
    assert status()['deployment']['observed'] == 'stopped'
    passed('stopped-intent-persists')
    report = dict(success=True, cases=completed, source_commit=metadata['source_commit'],
                  binary_sha256=metadata['binary_sha256'], runtime_sha256=metadata['runtime_sha256'],
                  image_sha256=IMAGE_SHA256, production_cli=True, private_test_ca=True,
                  public_dns_acme_verified=False)
    validate_report(report)
    Path('/output/report.json').write_text(json.dumps(report))


def validate_report(report):
    if (not isinstance(report, dict) or report.get('success') is not True or report.get('cases') != CASES or
            report.get('production_cli') is not True or report.get('private_test_ca') is not True or
            report.get('public_dns_acme_verified') is not False or report.get('image_sha256') != IMAGE_SHA256):
        raise ValueError('Incomplete unified-installer acceptance evidence.')
    for key, count in (('source_commit', 40), ('binary_sha256', 64), ('runtime_sha256', 64)):
        if not isinstance(report.get(key), str) or not re.fullmatch('[0-9a-f]{' + str(count) + '}', report[key]):
            raise ValueError('Missing exact source/binary identity in acceptance evidence.')
    return report


def host(inputs, report, base_image=None, timeout=900):
    if report.exists() or report.is_symlink():
        raise ValueError('Use a fresh report path; never replace prior evidence.')
    if not 300 <= timeout <= 1800:
        raise ValueError('Use a VM timeout between 300 and 1800 seconds.')
    with tempfile.TemporaryDirectory(prefix='devoption-app-install-vm-') as temporary:
        root = Path(temporary); results = root / 'results'; results.mkdir()
        if base_image:
            if digest(base_image) != IMAGE_SHA256:
                raise ValueError('Cached Ubuntu base image differs from the reviewed pin.')
            base = base_image
        else:
            base = root / 'base.img'; image(base)
        run('qemu-img', 'create', '-f', 'qcow2', '-F', 'qcow2', '-b', base, root / 'disk.qcow2', '8G')
        seed = root / 'seed'; seed.mkdir()
        (seed / 'meta-data').write_text('instance-id: devoption-app-install\nlocal-hostname: app-install-fixture\n')
        (seed / 'user-data').write_text('''#cloud-config
users: []
disable_root: true
ssh_deletekeys: false
ssh_genkeytypes: []
write_files:
  - path: /etc/systemd/system/devoption-install-acceptance.service
    content: |
      [Unit]
      After=network-online.target
      Wants=network-online.target
      [Service]
      Type=oneshot
      TimeoutStartSec=1200
      Environment=DEVOPTION_DISPOSABLE_APP_INSTALL=1
      ExecStart=/usr/bin/python3 /srv/devoption-install-proof/repository/scripts/check_app_installation.py --guest
      StandardOutput=journal+console
      StandardError=journal+console
runcmd:
  - [mkdir, -p, /inputs, /output, /srv/devoption-install-proof]
  - [mount, -t, 9p, -o, 'trans=virtio,version=9p2000.L', inputs, /inputs]
  - [cp, -r, /inputs/., /srv/devoption-install-proof/]
  - [mount, -t, 9p, -o, 'trans=virtio,version=9p2000.L', output, /output]
  - [sh, -c, 'apt-get update && apt-get install -y --no-install-recommends git sudo build-essential']
  - [touch, /run/devoption-app-install-guest]
  - [systemctl, daemon-reload]
  - [systemctl, start, --no-block, devoption-install-acceptance.service]
''')
        if shutil.which('xorriso'):
            run('xorriso', '-as', 'mkisofs', '-quiet', '-output', root / 'seed.iso', '-volid', 'cidata', '-joliet', '-rock', seed)
        elif platform.system() == 'Darwin':
            run('hdiutil', 'makehybrid', '-iso', '-joliet', '-default-volume-name', 'cidata', '-o', root / 'seed.iso', seed)
        else:
            raise ValueError('Install xorriso for the disposable seed image.')
        accelerator = 'tcg' if platform.system() == 'Darwin' else 'kvm:tcg'
        command = ['qemu-system-x86_64', '-machine', 'q35,accel=' + accelerator, '-cpu', 'max', '-smp', '2', '-m', '2048',
                   '-display', 'none', '-monitor', 'none', '-serial', 'file:' + str(root / 'serial.log'), '-no-reboot',
                   '-drive', 'if=virtio,format=qcow2,file=' + str(root / 'disk.qcow2'),
                   '-drive', 'if=virtio,format=raw,readonly=on,file=' + str(root / 'seed.iso'),
                   '-netdev', 'user,id=fixture,ipv6=off', '-device', 'virtio-net-pci,netdev=fixture',
                   '-virtfs', 'local,path=' + str(inputs) + ',mount_tag=inputs,security_model=none,readonly=on',
                   '-virtfs', 'local,path=' + str(results) + ',mount_tag=output,security_model=mapped-xattr']
        with (root / 'qemu.log').open('wb') as log:
            process = subprocess.Popen(command, stdout=log, stderr=log)
            try:
                process.wait(timeout=timeout)
                result = validate_report(json.loads((results / 'report.json').read_text()))
                expected = json.loads((inputs / 'metadata.json').read_text())
                if any(result[key] != expected[key] for key in ('source_commit', 'binary_sha256', 'runtime_sha256')):
                    raise ValueError('VM report differs from selected prepared inputs.')
                if process.returncode:
                    raise ValueError('QEMU failed despite its report.')
                with report.open('x') as stream:
                    json.dump(result, stream, indent=2)
                print('Fresh-VM unified install, HTTPS auth, approved build/deploy, preservation and restart acceptance passed.', flush=True)
            except BaseException:
                for name in ('serial.log', 'qemu.log'):
                    path = root / name
                    if path.exists():
                        print(path.read_text(errors='replace')[-12288:], file=sys.stderr)
                raise
            finally:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill(); process.wait(timeout=10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--prepare', type=Path)
    mode.add_argument('--inputs', type=Path)
    mode.add_argument('--guest', action='store_true')
    for name in ('binary', 'runtime', 'caddy-archive', 'report', 'base-image'):
        parser.add_argument('--' + name, type=Path)
    parser.add_argument('--timeout', type=int, default=900)
    args = parser.parse_args()
    if args.guest:
        require_guest()
        try:
            guest()
        except BaseException:
            import traceback
            traceback.print_exc()
            Path('/output/report.json').write_text(json.dumps({'success': False}))
            raise
        finally:
            run('systemctl', 'poweroff')
    elif args.prepare and all((args.binary, args.runtime, args.caddy_archive)):
        prepare(args.prepare.resolve(), args.binary.resolve(strict=True), args.runtime.resolve(strict=True), args.caddy_archive.resolve(strict=True))
    elif args.inputs and args.report:
        host(args.inputs.resolve(strict=True), args.report.resolve(), args.base_image.resolve(strict=True) if args.base_image else None, args.timeout)
    else:
        parser.error('Select --prepare DIR with binary/runtime/caddy-archive, or --inputs DIR --report NEW_FILE.')


if __name__ == '__main__':
    main()
