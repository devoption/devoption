#!/usr/bin/env python3
"""Generate reviewable hostname configuration; never installs files or changes DNS."""
import argparse
import json
from pathlib import Path
import tomllib
from deployment_broker import validate_ingress


def render(binding, app_port):
    validate_ingress(binding, app_port)
    platform, app = binding['platform_hostname'], binding['app_hostname']
    upstream = binding['platform_port']
    return f'''{{
    admin off
    servers {{
        strict_sni_host on
    }}
}}

{platform} {{
    request_body {{
        max_size 64KB
    }}
    reverse_proxy 127.0.0.1:{upstream} {{
        header_up -Forwarded
        header_up -X-Forwarded-*
        header_up -X-Real-IP
        transport http {{
            dial_timeout 3s
            response_header_timeout 95s
        }}
    }}
}}

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


def generate(source, output, binding):
    raw = source.read_text()
    config = tomllib.loads(raw)
    required = {'uid', 'build_work', 'state', 'worker', 'port', 'owner_data', 'history', 'repositories'}
    if set(config) - {'ingress'} != required:
        raise ValueError('expected the existing deployment configuration')
    if type(config['port']) is not int or not 1024 <= config['port'] <= 65535:
        raise ValueError('invalid private app port')
    proxy = render(binding, config['port'])
    # Serialize only the fixed schema. Existing binding is replaced in the staged
    # copy; installation remains a separate operator-reviewed action.
    text = ''.join(f'{key} = {json.dumps(config[key])}\n' for key in sorted(required))
    text += '\n[ingress]\n' + ''.join(f'{key} = {json.dumps(value)}\n' for key, value in binding.items())
    output.mkdir(mode=0o700)
    (output/'deployment.toml').write_text(text)
    (output/'Caddyfile').write_text(proxy)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--deployment-config', type=Path, required=True)
    parser.add_argument('--platform-hostname', required=True)
    parser.add_argument('--app-hostname', required=True)
    parser.add_argument('--project', required=True)
    parser.add_argument('--platform-port', type=int, default=3000)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        generate(args.deployment_config, args.output, dict(platform_hostname=args.platform_hostname,
                 app_hostname=args.app_hostname, project=args.project, platform_port=args.platform_port))
    except (ValueError, OSError) as error:
        parser.exit(1, f'Hostname configuration not generated: {error}\n')
    print('Review deployment.toml and Caddyfile in the new output directory. Nothing was installed; DNS is unchanged.')
