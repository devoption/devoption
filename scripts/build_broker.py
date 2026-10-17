#!/usr/bin/python3
"""Root-installed, fixed systemd adapter; never accepts a command, unit or path.

Install with root ownership and the documented narrowly scoped sudo rule.
Configuration and worker must also be root-owned, non-writable by other users.
"""
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tomllib

CONFIG = Path('/etc/devoption/build.toml')
WORKER = Path('/usr/local/libexec/devoption-build-worker')
UNIT = 'devoption-build.service'


def protected_file(path):
    for parent in path.parents:
        meta = parent.lstat()
        if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != 0 or meta.st_mode & 0o022:
            raise RuntimeError('unsafe installed build parent')
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_mode & 0o022:
        raise RuntimeError('unsafe installed build configuration')


def main():
    if os.geteuid() != 0:
        raise RuntimeError('installed broker requires root')
    protected_file(CONFIG)
    protected_file(WORKER)
    config = tomllib.loads(CONFIG.read_text())
    uid = config['uid']
    if type(uid) is not int or uid <= 0 or os.environ.get('SUDO_UID') != str(uid):
        raise RuntimeError('unexpected build caller')
    environment = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LANG': 'C.UTF-8'}
    if sys.argv[1:] == ['stop']:
        result = subprocess.run(['/usr/bin/systemctl', 'stop', UNIT], env=environment,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=8)
        state = subprocess.run(['/usr/bin/systemctl', 'show', UNIT, '-p', 'ActiveState', '--value'],
                               env=environment, stdout=subprocess.PIPE, check=True, timeout=1).stdout.strip()
        # A missing collected unit is also inactive; a still-running unit fails closed.
        if state not in (b'inactive', b'failed'):
            return 1
        group = subprocess.run(['/usr/bin/systemctl', 'show', UNIT, '-p', 'ControlGroup', '--value'],
                               env=environment, stdout=subprocess.PIPE, check=True, timeout=1).stdout.decode().strip()
        if group:
            events = Path('/sys/fs/cgroup') / group.lstrip('/') / 'cgroup.events'
            if events.exists() and 'populated 0' not in events.read_text():
                return 1
        return 0
    if len(sys.argv) != 3 or sys.argv[1] != 'run' or not re.fullmatch('[0-9a-f]{32}', sys.argv[2]):
        raise RuntimeError('invalid build request')
    source = Path(config['work']) / sys.argv[2] / 'source'
    if source.resolve(strict=True) != source or not source.is_dir():
        raise RuntimeError('invalid build source')
    properties = [
        f'User={uid}', 'PrivateNetwork=yes', 'RestrictAddressFamilies=',
        'MemoryMax=536870912', 'MemorySwapMax=0', 'TasksMax=64', 'CPUQuota=100%',
        'RuntimeMaxSec=60', 'TimeoutStopSec=5', 'KillMode=control-group',
        'NoNewPrivileges=yes', 'CapabilityBoundingSet=', 'ProtectSystem=strict',
        'ProtectHome=tmpfs', 'PrivateDevices=yes', 'PrivateTmp=yes',
        'ProtectKernelTunables=yes', 'ProtectKernelModules=yes', 'ProtectControlGroups=yes',
        'RestrictSUIDSGID=yes', 'LockPersonality=yes', 'UMask=0077',
        'TemporaryFileSystem=/job:rw,size=117440512,mode=1777,nosuid,nodev,noexec '
        '/job/target:rw,size=134217728,mode=1777,nosuid,nodev',
        f'InaccessiblePaths=-{config["owner_data"]} -{config["repositories"]} -{config["history"]} -{config["work"]} -/run/user',
        f'BindReadOnlyPaths={config["toolchain"]}:/toolchain {source}:/source '
        f'{config["vendor"]}:/vendor {WORKER}:/worker.py',
    ]
    # Config paths are operator-selected, with no systemd list/specifier syntax.
    for key in ('work', 'owner_data', 'repositories', 'history', 'toolchain', 'vendor'):
        path = config[key]
        if not isinstance(path, str) or not re.fullmatch(r'/[A-Za-z0-9_./-]+', path):
            raise RuntimeError('unsupported build configuration path')
        if str(Path(path).resolve(strict=True)) != path:
            raise RuntimeError('build configuration path must be canonical')
    args = ['/usr/bin/systemd-run', '--quiet', '--wait', '--pipe', '--collect', f'--unit={UNIT}']
    for prop in properties:
        args.extend(['--property', prop])
    args.extend(['/usr/bin/python3', '/worker.py'])
    return subprocess.run(args, env=environment, stdin=subprocess.DEVNULL).returncode


if __name__ == '__main__':
    try:
        sys.exit(main())
    except Exception:
        print('Installed build broker refused the request.', file=sys.stderr)
        sys.exit(1)
