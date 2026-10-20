"""Reboot the base upgrade fixture in an isolated, credential-free QEMU guest.

Host mode exports only copied fixture inputs and a private result directory.
Guest mode is destructive and is only installed by this runner in a fresh VM.
Optional signed-packet mode uses the production verifier and upgrade CLI; it
does not change their policy or authorize signing/publication.
"""
import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
from unittest.mock import patch

IMAGE_URL = ('https://cloud-images.ubuntu.com/minimal/releases/noble/release-20261017/'
             'ubuntu-24.04-minimal-cloudimg-amd64.img')
IMAGE_SHA256 = 'b3296484e362ba2e713489a5de6225c719d9500962bde25d15f2f21e512c7658'
STAGES = ['upgraded', 'interrupted', 'recovered', 'complete']
STATE = Path('/var/lib/devoption-reboot-fixture.json')
REPORT = Path('/output/report.json')


def write(path, value):
    temporary = path.with_suffix('.tmp')
    with temporary.open('w') as stream:
        os.fchmod(stream.fileno(), 0o600)
        json.dump(value, stream, sort_keys=True)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)
    descriptor = os.open(path.parent, os.O_DIRECTORY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def validate_report(value):
    import re
    if (not isinstance(value, dict) or value.get('success') is not True or
            value.get('stages') != STAGES or not isinstance(value.get('boot_ids'), list) or
            len(value['boot_ids']) != 4 or
            any(not isinstance(item, str) or not re.fullmatch(
                '[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}', item) for item in value['boot_ids']) or
            len(set(value['boot_ids'])) != 4):
        raise ValueError('Incomplete reboot acceptance: require all stages and four distinct kernel boot IDs.')
    return value


def boot_checkpoint(value, stage):
    boot_id = Path('/proc/sys/kernel/random/boot_id').read_text().strip()
    if boot_id in value['boot_ids']:
        raise ValueError('The VM kernel did not reboot between acceptance stages.')
    value['boot_ids'].append(boot_id)
    value['stages'].append(stage)
    write(STATE, value)
    os.sync()


def require_guest():
    if os.geteuid() != 0 or os.environ.get('DEVOPTION_DISPOSABLE_UPGRADE_FIXTURE') != '1':
        raise ValueError('Guest requires root and explicit disposable fixture opt-in.')


def guest(directory, signed=False):
    require_guest()
    from check_bundle_upgrade import FixtureHost, fixture_installers, enroll, request, session, run
    from install_bundle import PLATFORM
    from upgrade_bundle import Upgrade, UNIT
    if signed:
        from functools import partial
        request, session = partial(request, timeout=30), partial(session, timeout=30)
        import signed_upgrade_fixture as packets
        old, new = packets.installers(directory)
        host = old.host
    else:
        host = FixtureHost()
        old, new = fixture_installers(directory, host)
    host.preflight(Path('/'), 1024**3)
    host.candidate_version = new.version
    repository = Path(PLATFORM + '/repositories/reboot-checkout')
    sentinel = Path(PLATFORM + '/repositories/reboot-sentinel')

    def persistent_state(value):
        token, cookie = session()
        assert b'Reboot fixture project' in request('/projects', cookie=cookie)[1]
        assert request('/account/profile', cookie=cookie)[1] == b'Reboot fixture owner'
        assert sentinel.read_bytes() == b'before upgrade'
        head = host.run('runuser', '-u', 'devoption', '--', 'git', '-C', str(repository), 'rev-parse', 'HEAD').stdout
        assert head.decode().strip() == value['repository_head']
        return cookie

    def running(version):
        for unit in ('devoption-public.service', 'caddy-devoption.service'):
            assert host.active(unit)
            assert host.run('systemctl', 'is-enabled', unit).stdout.strip() == b'enabled'
        host.healthy(old.hostname)
        host.public_health(old.hostname)
        assert run(old.binary if version == old.version else new.binary, '--version') == f'devoption {version}\n'.encode()
        assert json.loads(Path('/var/lib/devoption-installer/installation.json').read_bytes())['reservation']['candidate']['platform_version'] == version
        # Assert the actual running unit selects this release, not merely that
        # an executable with the expected version remains on disk.
        command = host.run('systemctl', 'show', 'devoption-public.service', '--property=ExecStart', '--value').stdout
        assert (old.binary if version == old.version else new.binary).encode() in command

    value = json.loads(STATE.read_bytes()) if STATE.exists() else {'stages': [], 'boot_ids': []}
    if not value['stages']:
        for path in ('/var/lib/devoption', '/var/lib/caddy', '/var/lib/devoption-installer',
                     '/opt/devoption', '/etc/caddy', UNIT, '/etc/systemd/system/caddy-devoption.service'):
            assert not Path(path).exists() and not Path(path).is_symlink()
        assert host.account('devoption') is None and host.account('caddy') is None
        if signed:
            corrupt = directory/'corrupt.zip'
            raw = bytearray((directory/'candidate.zip').read_bytes()); raw[-1] ^= 1
            corrupt.write_bytes(raw)
            packets.command(directory, 'plan', candidate_packet=corrupt, expect_failure=True)
            corrupt.unlink()
            assert not Path('/var/lib/devoption-installer').exists()
            packets.install(directory)
        else:
            old.install()
        enroll(Path(old.binary), timeout=150 if signed else 30); old.start()
        if signed:
            packets.trust_guest_ca()
        host.public_health(old.hostname); host.enable()
        token, cookie = session()
        assert request('/projects', 'POST', 'Reboot fixture project', token, cookie)[0] == 204
        assert request('/account/profile', 'POST', 'Reboot fixture owner', token, cookie)[0] == 204
        sentinel.write_bytes(b'before upgrade'); os.chown(sentinel, *host.account('devoption'))
        host.run('runuser', '-u', 'devoption', '--', 'git', 'init', '--quiet', str(repository))
        host.run('runuser', '-u', 'devoption', '--', 'git', '-C', str(repository), '-c',
                 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit',
                 '--quiet', '--allow-empty', '-m', 'reboot fixture')
        value['repository_head'] = host.run('runuser', '-u', 'devoption', '--', 'git', '-C', str(repository), 'rev-parse', 'HEAD').stdout.decode().strip()
        value['cookie'] = persistent_state(value)
        if signed:
            assert packets.command(directory, 'plan')['phase'] == 'not-started'
        assert (packets.command(directory, 'apply') if signed else Upgrade(old, new).apply())['phase'] == 'healthy'
        value['cookie'] = persistent_state(value)
        boot_checkpoint(value, 'upgraded')
    elif value['stages'] == STAGES[:1]:
        running(new.version)
        assert (packets.command(directory, 'status') if signed else Upgrade(old, new).preflight())['phase'] == 'healthy'
        assert request('/auth/session', 'POST', cookie=value['cookie'])[0] == 401
        persistent_state(value)
        # A write after the backup must disappear from the recovered live tree.
        sentinel.write_bytes(b'post-upgrade data')
        assert (packets.command(directory, 'recover') if signed else Upgrade(old, new).recover(confirm=True))['phase'] == 'restored'
        persistent_state(value)
        transaction = Upgrade(old, new, attempt=2)
        replace = transaction.replace
        def interrupt(path, *args, **kwargs):
            replace(path, *args, **kwargs)
            if path == Path(UNIT):
                raise SystemExit('controlled interruption before receipt commit')
        with patch.object(transaction, 'replace', side_effect=interrupt):
            try:
                transaction.apply()
            except SystemExit:
                pass
            else:
                raise AssertionError('Interruption did not occur.')
        assert transaction.record()['phase'] == 'backed-up'
        boot_checkpoint(value, 'interrupted')
    elif value['stages'] == STAGES[:2]:
        for unit in ('devoption-public.service', 'caddy-devoption.service'):
            assert not host.active(unit)
            assert subprocess.run(['systemctl', 'is-enabled', unit], capture_output=True, timeout=10).stdout.strip() == b'disabled'
        for port in (80, 443, 3000):
            host.free_port(port)
        transaction = Upgrade(old, new, attempt=2)
        assert transaction.record()['phase'] == 'backed-up'
        assert (packets.command(directory, 'recover', attempt=2) if signed else transaction.recover(confirm=True))['phase'] == 'restored'
        running(old.version)
        value['cookie'] = persistent_state(value)
        boot_checkpoint(value, 'recovered')
    elif value['stages'] == STAGES[:3]:
        running(old.version)
        assert request('/auth/session', 'POST', cookie=value['cookie'])[0] == 401
        persistent_state(value)
        transaction = Upgrade(old, new, attempt=2)
        assert (packets.command(directory, 'status', attempt=2) if signed else transaction.preflight())['phase'] == 'restored'
        # A completed retry must not restore the obsolete snapshot over new data.
        sentinel.write_bytes(b'write after recovery reboot')
        if signed:
            packets.command(directory, 'recover', attempt=2)
        else:
            transaction.recover(confirm=True)
        assert sentinel.read_bytes() == b'write after recovery reboot'
        boot_checkpoint(value, 'complete')
        result = {'success': True, 'stages': value['stages'], 'boot_ids': value['boot_ids'],
                  'previous': old.version, 'candidate': new.version}
        if signed:
            result.update(authenticated_packets=True, previous_identity=old.record, candidate_identity=new.record,
                          corrupt_packet_rejected=True, production_cli=True)
        validate_report(result)
        write(REPORT, result)
        host.run('systemctl', 'poweroff')
        return
    else:
        raise ValueError('Unknown reboot acceptance state; refuse to resume.')
    host.run('systemctl', 'reboot')


def image(path):
    with urllib.request.urlopen(IMAGE_URL, timeout=60) as response, path.open('xb') as stream:
        digest = hashlib.sha256()
        size = 0
        while chunk := response.read(1024**2):
            size += len(chunk)
            if size > 400 * 1024**2:
                raise ValueError('VM image exceeds the download bound.')
            stream.write(chunk); digest.update(chunk)
    if digest.hexdigest() != IMAGE_SHA256:
        raise ValueError('Ubuntu fixture image SHA-256 differs from the reviewed pin.')


def host(directory, output, signed=False, gh=None, boot_timeout=600):
    # No host root, SSH key, forwarded port, production installation or credential
    # is needed. QEMU receives only these private copied inputs/output, not $HOME.
    if output.exists():
        raise ValueError('Use a fresh report path; stale evidence cannot pass.')
    if not 600 <= boot_timeout <= 3600:
        raise ValueError('Use a bounded boot timeout between 600 and 3600 seconds.')
    if signed:
        import signed_upgrade_fixture as packets
        old, new = packets.installers(directory, gh)
    with tempfile.TemporaryDirectory(prefix='devoption-upgrade-reboot-') as temporary:
        root = Path(temporary)
        inputs, results = root/'inputs', root/'results'
        inputs.mkdir(); results.mkdir()
        scripts = inputs/'scripts'; scripts.mkdir()
        for path in Path(__file__).resolve().parent.glob('*.py'):
            shutil.copyfile(path, scripts/path.name)
        fixtures = inputs/'fixtures'; fixtures.mkdir()
        if signed:
            for name in ('previous.json', 'candidate.json', 'previous.zip', 'candidate.zip', 'gh.tar.gz'):
                shutil.copyfile(directory/name, fixtures/name)
        else:
            for name in ('previous', 'candidate'):
                shutil.copytree(directory/(name+'-bundle'), fixtures/(name+'-bundle'))
        shutil.copyfile(directory/'caddy.tar.gz', fixtures/'caddy.tar.gz')
        base = root/'base.img'; image(base)
        subprocess.run(['qemu-img', 'create', '-f', 'qcow2', '-F', 'qcow2', '-b', str(base), str(root/'disk.qcow2'), '8G'], check=True)
        seed = root/'seed'; seed.mkdir()
        (seed/'meta-data').write_text('instance-id: devoption-upgrade-reboot\nlocal-hostname: upgrade-fixture\n')
        user_data = '''#cloud-config
users: []
disable_root: true
ssh_deletekeys: false
ssh_genkeytypes: []
write_files:
  - path: /etc/systemd/system/devoption-upgrade-reboot-fixture.service
    content: |
      [Unit]
      Wants=network-online.target
      After=network-online.target devoption-public.service caddy-devoption.service
      RequiresMountsFor=/output
      [Service]
      Type=oneshot
      Environment=DEVOPTION_DISPOSABLE_UPGRADE_FIXTURE=1
      ExecStart=/usr/bin/python3 /opt/upgrade-fixture/scripts/check_upgrade_reboot.py --guest /opt/upgrade-fixture/fixtures
      StandardOutput=journal+console
      StandardError=journal+console
      [Install]
      WantedBy=multi-user.target
runcmd:
  - [mkdir, -p, /inputs, /output]
  - [mount, -t, 9p, -o, 'trans=virtio,version=9p2000.L', inputs, /inputs]
  - [cp, -r, /inputs, /opt/upgrade-fixture]
  - [sh, -c, 'echo "output /output 9p trans=virtio,version=9p2000.L 0 0" >> /etc/fstab']
  - [mount, /output]
  - [sh, -c, 'apt-get update && apt-get install -y --no-install-recommends git sudo']
  - [systemctl, daemon-reload]
  - [systemctl, enable, --now, --no-block, devoption-upgrade-reboot-fixture.service]
'''
        if signed:
            user_data = user_data.replace('--guest /opt/upgrade-fixture/fixtures', '--guest /opt/upgrade-fixture/fixtures --signed-packets')
            user_data = user_data.replace('      Type=oneshot\n', '      Type=oneshot\n      TimeoutStartSec=900\n')
            user_data = user_data.replace('git sudo', 'git sudo ca-certificates')
            user_data = user_data.replace('runcmd:\n', '''runcmd:
  - [sh, -c, 'echo "127.0.0.1 platform.upgrade.localhost" >> /etc/hosts']
''')
        (seed/'user-data').write_text(user_data)
        if shutil.which('xorriso'):
            subprocess.run(['xorriso', '-as', 'mkisofs', '-quiet', '-output', str(root/'seed.iso'),
                        '-volid', 'cidata', '-joliet', '-rock', str(seed)], check=True)
        elif platform.system() == 'Darwin':
            subprocess.run(['hdiutil', 'makehybrid', '-iso', '-joliet', '-default-volume-name', 'cidata', '-o', str(root/'seed.iso'), str(seed)], check=True, capture_output=True)
        else:
            raise ValueError('Install xorriso for the disposable VM seed.')
        accelerator = 'tcg' if platform.system() == 'Darwin' else 'kvm:tcg'
        command = ['qemu-system-x86_64', '-machine', 'q35,accel='+accelerator, '-cpu', 'max',
                   '-smp', '2', '-m', '2048', '-display', 'none', '-monitor', 'none',
                   '-serial', 'file:'+str(root/'serial.log'), '-no-reboot',
                   '-drive', 'if=virtio,format=qcow2,file='+str(root/'disk.qcow2'),
                   '-drive', 'if=virtio,format=raw,readonly=on,file='+str(root/'seed.iso'),
                   '-netdev', 'user,id=fixture,ipv6=off', '-device', 'virtio-net-pci,netdev=fixture',
                   '-virtfs', 'local,path='+str(inputs)+',mount_tag=inputs,security_model=none,readonly=on',
                   '-virtfs', 'local,path='+str(results)+',mount_tag=output,security_model=mapped-xattr']
        # Each power cycle starts a new QEMU process against the same private
        # disk. -no-reboot makes guest reboot exit, proving a real kernel boot.
        deadline = time.monotonic()+boot_timeout
        with (root/'qemu.log').open('wb') as log:
            for boot in range(4):
                print(f'Starting disposable upgrade VM boot {boot+1}/4', flush=True)
                child = subprocess.Popen(command, stdout=log, stderr=log)
                try:
                    child.wait(timeout=max(1, deadline-time.monotonic()))
                    if child.returncode != 0:
                        raise ValueError('Disposable QEMU process failed.')
                except BaseException:
                    for name in ('serial.log', 'qemu.log'):
                        path = root/name
                        if path.exists():
                            print(path.read_bytes()[-8192:].decode(errors='replace'), file=sys.stderr)
                    raise
                finally:
                    if child.poll() is None:
                        child.terminate()
                        try:
                            child.wait(timeout=10)
                        except subprocess.TimeoutExpired:
                            child.kill(); child.wait(timeout=10)
                if (results/'report.json').exists():
                    try:
                        result = validate_report(json.loads((results/'report.json').read_bytes()))
                    except ValueError:
                        print((root/'serial.log').read_bytes()[-8192:].decode(errors='replace'), file=sys.stderr)
                        raise
                    if boot != 3:
                        raise ValueError('Unexpected early reboot result.')
                    if signed:
                        packets.validate_report(result, old.record, new.record)
                    output.write_text(json.dumps(result, indent=2)+'\n')
                    print('Actual VM reboot acceptance passed: upgraded, interrupted, recovered; four distinct kernel boots.')
                    return
            raise ValueError('VM exited without complete reboot evidence.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--guest', type=Path)
    parser.add_argument('--fixtures', type=Path)
    parser.add_argument('--report', type=Path)
    parser.add_argument('--signed-packets', action='store_true')
    parser.add_argument('--gh', type=Path, help='Trusted host verifier for signed inputs; required in host signed mode')
    parser.add_argument('--boot-timeout', type=int, default=600)
    args = parser.parse_args()
    if args.guest and not args.fixtures and not args.report:
        # Reject accidental host invocation before installing the guest-only
        # failure handler: it powers off the authorized disposable VM.
        require_guest()
        try:
            guest(args.guest, args.signed_packets)
        except BaseException:
            import traceback
            traceback.print_exc()
            write(REPORT, {'success': False})
            subprocess.run(['systemctl', 'poweroff'], check=False, timeout=30)
            raise
    elif args.fixtures and args.report and not args.guest:
        if args.signed_packets and args.gh is None:
            parser.error('Signed host mode requires --gh for independent packet verification before boot.')
        host(args.fixtures.resolve(strict=True), args.report.resolve(), args.signed_packets, args.gh, args.boot_timeout)
    else:
        parser.error('Select --fixtures DIR --report NEW_FILE, or explicit disposable --guest DIR.')
