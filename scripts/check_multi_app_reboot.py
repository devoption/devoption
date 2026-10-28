"""Three real kernel boots of the disposable two-app lifecycle fixture.

Receives only the explicit fixture export, uses the existing reviewed Ubuntu
image pin, and destroys the VM, copied test CA and state after a bounded run.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

from check_upgrade_reboot import image


def run(*args):
    result = subprocess.run([str(x) for x in args], capture_output=True, timeout=95)
    if result.returncode:
        raise ValueError('Fixture command failed: '+result.stderr.decode(errors='replace')[-4096:])
    return result.stdout


def guest(directory):
    if os.geteuid() != 0 or os.environ.get('DEVOPTION_DISPOSABLE_MULTI_APP') != '1':
        raise ValueError('Explicit disposable guest opt-in required')
    from check_multi_app import STATE, CONFIG, BROKER, PROXY_UNIT
    metadata = json.loads((directory/'metadata.json').read_text())
    checkpoint = Path('/var/lib/devoption-multi-reboot.json')
    value = json.loads(checkpoint.read_text()) if checkpoint.exists() else {'boots': [], 'phases': []}
    boot = Path('/proc/sys/kernel/random/boot_id').read_text().strip()
    if boot in value['boots']: raise ValueError('Actual kernel reboot required')
    if not value['boots']:
        if any(path.exists() for path in (STATE, CONFIG, BROKER)):
            raise ValueError('Guest installation is not empty')
        run('tar', '-C', '/', '-xf', directory/'snapshot.tar')
        root = Path(metadata['root']); root.mkdir(mode=0o700)
        for name in ('data', 'repositories', 'history', 'jobs'):
            (root/name).mkdir(mode=0o700)
        for value_path in metadata['policy'].values():
            shutil.copyfile(directory/Path(value_path).name, value_path)
        (root/'Caddyfile').write_text(metadata['proxy'])
        unit = Path('/etc/systemd/system')/PROXY_UNIT
        unit.write_text(f'''[Unit]
After=network.target
[Service]
Environment=XDG_DATA_HOME={STATE}/caddy
Environment=XDG_CONFIG_HOME={STATE}/caddy-config
ExecStart={directory}/caddy run --config {root}/Caddyfile --adapter caddyfile
[Install]
WantedBy=multi-user.target
''')
        run('systemctl', 'daemon-reload'); run('systemctl', 'enable', '--now', PROXY_UNIT)

    def broker(project, *args):
        return json.loads(run('env', 'SUDO_UID='+str(metadata['uid']), BROKER, 'project', project, *args))

    for project in metadata['projects']: broker(project, 'reconcile')
    first, second = metadata['projects']
    for index, project in enumerate((first, second)):
        expected = 'stopped' if len(value['boots']) == 2 and index == 0 else 'running'
        deadline = time.monotonic()+30
        while True:
            status = broker(project, 'status')
            assert status['deployment']['intent'] == expected
            if expected == 'stopped':
                assert status['deployment']['observed'] == 'stopped'
                break
            if status['deployment']['observed'] == 'healthy' and status['ingress']['status'].startswith('Local HTTPS route verified'):
                break
            if time.monotonic() > deadline: raise ValueError('Independent reboot health failed')
            time.sleep(.2)
    phase = ('both-running', 'both-recovered', 'stopped-and-running')[len(value['boots'])]
    if len(value['boots']) == 1:
        sibling = broker(second, 'status')['deployment']
        broker(first, 'stop')
        assert broker(second, 'status')['deployment'] == sibling
    value['boots'].append(boot); value['phases'].append(phase)
    checkpoint.write_text(json.dumps(value)); os.sync()
    if len(value['boots']) == 3:
        value['success'] = True
        Path('/output/report.json').write_text(json.dumps(value))
        run('systemctl', 'poweroff')
    else:
        run('systemctl', 'reboot')


def host(directory, output):
    if output.exists(): raise ValueError('Use a fresh report path')
    with tempfile.TemporaryDirectory(prefix='devoption-multi-reboot-') as temporary:
        root = Path(temporary); inputs = root/'inputs'; inputs.mkdir(); results = root/'results'; results.mkdir()
        shutil.copytree(directory, inputs/'fixture')
        scripts = inputs/'scripts'; scripts.mkdir()
        for path in Path(__file__).parent.glob('*.py'): shutil.copyfile(path, scripts/path.name)
        base = root/'base.img'; image(base)
        run('qemu-img', 'create', '-f', 'qcow2', '-F', 'qcow2', '-b', base, root/'disk.qcow2', '4G')
        seed = root/'seed'; seed.mkdir()
        (seed/'meta-data').write_text('instance-id: devoption-multi-reboot\nlocal-hostname: multi-app-fixture\n')
        (seed/'user-data').write_text('''#cloud-config
users: []
disable_root: true
ssh_deletekeys: false
ssh_genkeytypes: []
write_files:
  - path: /etc/systemd/system/devoption-multi-reboot.service
    content: |
      [Unit]
      After=network.target devoption-multi-proof-proxy.service
      RequiresMountsFor=/output
      [Service]
      Type=oneshot
      Environment=DEVOPTION_DISPOSABLE_MULTI_APP=1
      ExecStart=/usr/bin/python3 /opt/multi/scripts/check_multi_app_reboot.py --guest /opt/multi/fixture
      StandardOutput=journal+console
      StandardError=journal+console
      [Install]
      WantedBy=multi-user.target
runcmd:
  - [mkdir, -p, /inputs, /output]
  - [mount, -t, 9p, -o, 'trans=virtio,version=9p2000.L', inputs, /inputs]
  - [cp, -r, /inputs, /opt/multi]
  - [chmod, '755', /opt/multi/fixture/caddy]
  - [sh, -c, 'echo "output /output 9p trans=virtio,version=9p2000.L 0 0" >> /etc/fstab']
  - [mount, /output]
  - [systemctl, daemon-reload]
  - [systemctl, enable, --now, --no-block, devoption-multi-reboot.service]
''')
        run('xorriso', '-as', 'mkisofs', '-quiet', '-output', root/'seed.iso', '-volid', 'cidata', '-joliet', '-rock', seed)
        command = ['qemu-system-x86_64', '-machine', 'q35,accel=kvm:tcg', '-cpu', 'max', '-smp', '2', '-m', '2048',
                   '-display', 'none', '-monitor', 'none', '-serial', 'file:'+str(root/'serial.log'), '-no-reboot',
                   '-drive', 'if=virtio,format=qcow2,file='+str(root/'disk.qcow2'),
                   '-drive', 'if=virtio,format=raw,readonly=on,file='+str(root/'seed.iso'),
                   '-netdev', 'user,id=fixture,ipv6=off', '-device', 'virtio-net-pci,netdev=fixture',
                   '-virtfs', 'local,path='+str(inputs)+',mount_tag=inputs,security_model=none,readonly=on',
                   '-virtfs', 'local,path='+str(results)+',mount_tag=output,security_model=mapped-xattr']
        deadline = time.monotonic()+600
        for boot in range(3):
            print(f'Starting disposable multi-app VM boot {boot+1}/3', flush=True)
            with (root/'qemu.log').open('ab') as log:
                child = subprocess.Popen(command, stdout=log, stderr=log)
                try:
                    child.wait(timeout=max(1, deadline-time.monotonic()))
                    if child.returncode: raise ValueError('QEMU process failed')
                except BaseException:
                    print((root/'serial.log').read_text(errors='replace')[-8192:])
                    raise
                finally:
                    if child.poll() is None:
                        child.terminate()
                        try: child.wait(timeout=10)
                        except subprocess.TimeoutExpired: child.kill(); child.wait(timeout=10)
            report = results/'report.json'
            if report.exists():
                result = json.loads(report.read_text())
                if (boot != 2 or result.get('success') is not True or len(set(result.get('boots', []))) != 3
                        or result.get('phases') != ['both-running', 'both-recovered', 'stopped-and-running']):
                    print((root/'serial.log').read_text(errors='replace')[-8192:])
                    raise ValueError('Incomplete multi-app reboot evidence')
                output.write_text(json.dumps(result, indent=2)+'\n')
                print('Independent running/stopped app recovery passed across three kernel boots.')
                return
        raise ValueError('Missing multi-app reboot report')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--guest', type=Path)
    parser.add_argument('--fixture', type=Path)
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    if args.guest and not args.fixture and not args.report:
        if os.geteuid() != 0 or os.environ.get('DEVOPTION_DISPOSABLE_MULTI_APP') != '1':
            parser.error('Guest requires explicit disposable opt-in')
        try: guest(args.guest)
        except BaseException:
            import traceback
            traceback.print_exc()
            Path('/output/report.json').write_text('{"success":false}')
            subprocess.run(['systemctl', 'poweroff'], timeout=30, check=False)
            raise
    elif args.fixture and args.report and not args.guest:
        host(args.fixture.resolve(strict=True), args.report.resolve())
    else: parser.error('Select --fixture DIR --report NEW_FILE or explicit --guest DIR')
