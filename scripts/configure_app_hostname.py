#!/usr/bin/env python3
"""Generate reviewable hostname configuration; never installs files or changes DNS."""
import argparse
import json
from pathlib import Path, PurePosixPath
import re
import tomllib
from deployment_broker import hostname, validate_ingress


def validate_policy(policy):
    if not isinstance(policy, dict) or set(policy) != {'client_ca_file', 'allowed_clients_file'}:
        raise ValueError('expected client CA and permitted leaf certificate file paths')
    for value in policy.values():
        if (not isinstance(value, str) or len(value) > 4096
                or not re.fullmatch(r'/[A-Za-z0-9_./-]+', value)
                or str(PurePosixPath(value)) != value
                or any(part in ('', '.', '..') for part in value.split('/')[1:])
                or value == '/' or value.endswith('/')):
            raise ValueError('certificate paths must be canonical absolute ASCII file paths')
    if policy['client_ca_file'] == policy['allowed_clients_file']:
        raise ValueError('use separate CA and permitted leaf certificate files')


def load_policy(path):
    with path.open('rb') as source:
        raw = source.read(8193)
    if len(raw) > 8192:
        raise ValueError('access policy exceeds size limit')
    policy = tomllib.loads(raw.decode('utf-8'))
    validate_policy(policy)
    return policy


def render_platform(host, port, policy=None):
    hostname(host)
    if type(port) is not int or not 1024 <= port <= 65535:
        raise ValueError('invalid private platform port')
    access = ''
    if policy is not None:
        validate_policy(policy)
        access = f'''    tls {{
        client_auth {{
            mode require_and_verify
            trust_pool file {policy['client_ca_file']}
            verifier leaf file {policy['allowed_clients_file']}
        }}
    }}
'''
    return f'''{host} {{
{access}    request_body {{
        max_size 64KB
    }}
    reverse_proxy 127.0.0.1:{port} {{
        header_up -Forwarded
        header_up -X-Forwarded-*
        header_up -X-Real-IP
        header_up -X-Client-*
        header_up -X-SSL-*
        header_up -SSL-Client-*
        transport http {{
            dial_timeout 3s
            response_header_timeout 95s
        }}
    }}
}}
'''


def generate_platform(host, port, policy, output):
    validate_policy(policy)
    site = render_platform(host, port, policy)
    output.mkdir(mode=0o700)
    (output/'Caddyfile').write_text('''{
    admin off
    servers {
        strict_sni_host on
    }
}

''' + site)


def render(binding, app_port, platform_access=None):
    validate_ingress(binding, app_port)
    platform, app = binding['platform_hostname'], binding['app_hostname']
    platform_site = render_platform(platform, binding['platform_port'], platform_access)
    return f'''{{
    admin off
    servers {{
        strict_sni_host on
    }}
}}

{platform_site}
{app} {{
    request_body {{
        max_size 64KB
    }}
    @control path /auth /auth/* /sign-in /setup /setup/*
    respond @control 404
    reverse_proxy 127.0.0.1:{app_port} {{
        header_up -Cookie
        header_up -Authorization
        header_up -Proxy-Authorization
        header_up -X-Csrf-Token
        header_up -X-DevOption-*
        header_up -Forwarded
        header_up -X-Forwarded-*
        header_up -X-Real-IP
        transport http {{
            dial_timeout 3s
            response_header_timeout 10s
        }}
    }}
}}
'''


def generate(source, output, binding, platform_access=None):
    raw = source.read_text()
    config = tomllib.loads(raw)
    required = {'uid', 'build_work', 'state', 'worker', 'port', 'owner_data', 'history', 'repositories'}
    if set(config) - {'ingress'} != required:
        raise ValueError('expected the existing deployment configuration')
    if type(config['port']) is not int or not 1024 <= config['port'] <= 65535:
        raise ValueError('invalid private app port')
    proxy = render(binding, config['port'], platform_access)
    # Serialize only the fixed schema. Existing binding is replaced in the staged
    # copy; installation remains a separate operator-reviewed action.
    text = ''.join(f'{key} = {json.dumps(config[key])}\n' for key in sorted(required))
    text += '\n[ingress]\n' + ''.join(f'{key} = {json.dumps(value)}\n' for key, value in binding.items())
    output.mkdir(mode=0o700)
    (output/'deployment.toml').write_text(text)
    (output/'Caddyfile').write_text(proxy)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--deployment-config', type=Path)
    parser.add_argument('--platform-hostname', required=True)
    parser.add_argument('--app-hostname')
    parser.add_argument('--project')
    parser.add_argument('--platform-port', type=int, default=3000)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--platform-only', action='store_true')
    access = parser.add_mutually_exclusive_group(required=True)
    access.add_argument('--platform-access-policy', type=Path)
    access.add_argument('--public-platform', action='store_true')
    args = parser.parse_args()
    if args.platform_only:
        if not args.platform_access_policy or any((args.deployment_config, args.app_hostname, args.project)):
            parser.error('--platform-only requires an access policy and no app/deployment arguments')
    elif not all((args.deployment_config, args.app_hostname, args.project)):
        parser.error('app routing requires --deployment-config, --app-hostname and --project')
    try:
        policy = load_policy(args.platform_access_policy) if args.platform_access_policy else None
        if args.platform_only:
            generate_platform(args.platform_hostname, args.platform_port, policy, args.output)
        else:
            generate(args.deployment_config, args.output, dict(platform_hostname=args.platform_hostname,
                     app_hostname=args.app_hostname, project=args.project, platform_port=args.platform_port), policy)
    except (ValueError, OSError) as error:
        parser.exit(1, f'Hostname configuration not generated: {error}\n')
    if not args.platform_access_policy:
        print('Administrative route has no client-certificate gate; supply --platform-access-policy to retain protected access.')
    print('Review the staged configuration and validate certificate inputs with the pinned proxy. Nothing was installed; no credentials or DNS were changed.')
