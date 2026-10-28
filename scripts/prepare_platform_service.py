#!/usr/bin/env python3
"""Stage a reviewable systemd service for installed build/deployment configuration."""
import argparse
from pathlib import Path
import re
import tomllib
from deployment_broker import hostname as validate_hostname, validate_ingress, deployment_slots


def safe_path(value):
    if not isinstance(value, str) or not re.fullmatch(r'/[A-Za-z0-9_./-]+', value):
        raise ValueError('paths must be absolute ASCII paths without systemd expansions')
    if any(part in ('', '.', '..') for part in value[1:].split('/')):
        raise ValueError('paths must be normalized')
    return value


def render(build, deployment, binary, account, hostname, port):
    if not re.fullmatch(r'[a-z_][a-z0-9_-]*', account) or account == 'root':
        raise ValueError('use the existing nonroot platform account')
    if type(build.get('uid')) is not int or build['uid'] <= 0:
        raise ValueError('build configuration must select a nonroot UID')
    paths = {key: safe_path(build[key]) for key in ('owner_data', 'repositories', 'history', 'work', 'toolchain', 'vendor')}
    roots = list(map(Path, paths.values()))
    if any(a == b or a in b.parents or b in a.parents for i, a in enumerate(roots) for b in roots[i+1:]):
        raise ValueError('build roots must be disjoint')
    binary = safe_path(binary)
    if any(Path(binary) == root or root in Path(binary).parents for root in roots[:4]):
        raise ValueError('binary must be outside writable storage')
    validate_hostname(hostname)
    if type(port) is not int or not 1024 <= port <= 65535:
        raise ValueError('use an unprivileged private platform port')
    if deployment is not None:
        slots = deployment_slots(deployment)
        for slot in slots:
            app_port = slot['port']
            if type(app_port) is not int or not 1024 <= app_port <= 65535 or app_port == port:
                raise ValueError('platform and app require distinct private ports')
            if 'ingress' in slot:
                binding = slot['ingress']
                validate_ingress(binding, app_port)
                if binding['platform_hostname'] != hostname or binding['platform_port'] != port:
                    raise ValueError('service origin/port disagree with ingress')
        state = Path(safe_path(deployment['state']))
        if Path(binary) == state or state in Path(binary).parents:
            raise ValueError('binary must be outside release storage')
        if any(state == root or state in root.parents or root in state.parents for root in roots):
            raise ValueError('release state must be separate from build roots')
        for key in ('uid', 'owner_data', 'repositories', 'history'):
            if deployment.get(key) != build[key]:
                raise ValueError('build and deployment configurations disagree: ' + key)
        if deployment.get('build_work') != paths['work']:
            raise ValueError('build and deployment work paths disagree')
    command = (f'{binary} serve --port {port} --data-dir {paths["owner_data"]}'
               f' --repository-root {paths["repositories"]} --build-history-dir {paths["history"]}'
               f' --public-origin https://{hostname} --build-execution enabled')
    if deployment is not None:
        command += ' --app-deployment enabled'
    writable = ' '.join(paths[key] for key in ('owner_data', 'repositories', 'history', 'work'))
    if deployment is not None:
        writable += ' ' + str(state)
    return f'''[Unit]
Description=DevOption platform with reviewed local app capabilities
After=network.target
StartLimitIntervalSec=60
StartLimitBurst=5

[Service]
Type=exec
User={account}
Group={account}
WorkingDirectory={paths['owner_data']}
ExecStartPre=/usr/bin/test -s {paths['owner_data']}/owner.bin
ExecStart={command}
Restart=on-failure
RestartSec=5s
UMask=0077
# Required only for the installed, narrowly authorized sudo brokers.
NoNewPrivileges=false
PrivateTmp=true
PrivateDevices=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths={writable}
RestrictSUIDSGID=true

[Install]
WantedBy=multi-user.target
'''


def generate(build_path, deployment_path, output, **options):
    build = tomllib.loads(build_path.read_text())
    deployment = tomllib.loads(deployment_path.read_text()) if deployment_path else None
    unit = render(build, deployment, **options)
    output.mkdir(mode=0o700)
    (output / 'devoption-public.service').write_text(unit)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build-config', type=Path, required=True)
    parser.add_argument('--deployment-config', type=Path)
    parser.add_argument('--binary', default='/opt/devoption/devoption')
    parser.add_argument('--account', default='devoption')
    parser.add_argument('--hostname', required=True)
    parser.add_argument('--port', type=int, default=3000)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        generate(args.build_config, args.deployment_config, args.output,
                 binary=args.binary, account=args.account, hostname=args.hostname, port=args.port)
    except (ValueError, KeyError, OSError) as error:
        parser.exit(1, f'Service not generated: {error}\n')
    print('Review the staged service. Verify account UID/group, installed brokers and sudo rules before installation. Nothing was installed or started.')
