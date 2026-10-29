"""Plan/apply a prepared, source-built app-capable installation on a fresh host.

This is not a signed bundle installer, upgrader or existing-install conversion.
The administrator independently reviews this checkout and the input SHA-256s.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tarfile
import tomllib

from deployment_broker import protected
from install_bundle import Installer, MIN_FREE, PLATFORM, PROXY, STATE, installation_lock, proxy_binary
from installer_host import Host
from prepare_build_config import verify_vendor
from prepare_platform_service import render, safe_path
from release_bundle import CADDY_SHA512, CADDY_VERSION, MAX_BINARY, canonical
from verify_bundle_provenance import snapshot

REPOSITORY = Path(__file__).resolve().parent.parent
HELPERS = {
    'build_broker.py': 'devoption-build-broker',
    'build_worker.py': 'devoption-build-worker',
    'deployment_broker.py': 'devoption-deployment-broker',
}
APP_STATE = '/var/lib/devoption-app'
SUDOERS = '/etc/sudoers.d/devoption-app'
SUDO_RULE = (b'devoption ALL=(root) NOPASSWD: /usr/local/libexec/devoption-build-broker stop, '
             b'/usr/local/libexec/devoption-build-broker run *\n'
             b'devoption ALL=(root) NOPASSWD: /usr/local/libexec/devoption-deployment-broker *\n')


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def input_tree(root, administrator=0):
    """Same byte/path/mode identity as build-input-digest; never execute inputs.

    Require administrator ownership too: installed configuration continues to
    reference these prepared trees, which the installer does not copy or chmod.
    """
    root = Path(safe_path(str(root)))
    protected(root, administrator)
    if root.resolve(strict=True) != root or not root.is_dir():
        raise ValueError('Prepared inputs must be canonical directories.')
    if administrator == 0 and any(not p.stat().st_mode & 0o001 for p in (root, *root.parents)):
        raise ValueError('Prepared input ancestors must be traversable by the service account.')
    files, total = [], 0
    def collect(directory, depth):
        if depth > 32:
            raise ValueError('Prepared inputs exceed depth limit.')
        for path in directory.iterdir():
            info = path.lstat()
            if info.st_uid != administrator or info.st_mode & 0o022:
                raise ValueError('Prepared inputs must be administrator-controlled.')
            if stat.S_ISDIR(info.st_mode):
                if info.st_mode & 0o005 != 0o005:
                    raise ValueError('Prepared input directories must be readable by the service account.')
                collect(path, depth + 1)
            elif stat.S_ISREG(info.st_mode):
                if not info.st_mode & 0o004:
                    raise ValueError('Prepared files must be readable by the service account.')
                if len(files) >= 16384:
                    raise ValueError('Prepared inputs exceed file limit.')
                files.append(path)
            else:
                raise ValueError('Prepared inputs cannot contain links or special files.')
    collect(root, 0)
    manifest = bytearray()
    for path in sorted(files):
        info = path.lstat()
        total += info.st_size
        if total > 2 * 1024**3 or info.st_size > 256 * 1024**2:
            raise ValueError('Prepared inputs exceed byte limit.')
        # snapshot deliberately rejects empty files; the Rust tree format allows them.
        if info.st_size:
            content = snapshot(path, 256 * 1024**2)
        else:
            content = b''
        relative = path.relative_to(root).as_posix().encode('utf-8')
        manifest += len(relative).to_bytes(8, 'big') + relative
        manifest += hashlib.sha256(content).digest() + (info.st_mode & 0o777).to_bytes(4, 'big')
    return sha256(manifest)


def load_config(path):
    config = tomllib.loads(snapshot(path, 16384).decode('utf-8'))
    paths = {'binary', 'runtime', 'caddy_archive', 'toolchain', 'vendor', 'lockfile', 'archives'}
    digests = {'binary_sha256', 'runtime_sha256', 'toolchain_sha256', 'vendor_sha256', 'lock_sha256'}
    if set(config) != paths | digests | {'schema_version', 'hostname', 'source_commit'}:
        raise ValueError('Unexpected or missing installation configuration fields.')
    if type(config['schema_version']) is not int or config['schema_version'] != 1:
        raise ValueError('Unsupported installation configuration schema.')
    from deployment_broker import hostname
    hostname(config['hostname'])
    for key in digests | {'source_commit'}:
        length = 40 if key == 'source_commit' else 64
        if not isinstance(config[key], str) or not re.fullmatch('[0-9a-f]{' + str(length) + '}', config[key]):
            raise ValueError('Supply independently reviewed lowercase input hashes: ' + key)
    for key in paths:
        value = config[key]
        safe_path(value)
        if Path(value).resolve(strict=True) != Path(value):
            raise ValueError('Installation inputs must use canonical paths: ' + key)
        # Inputs are retained between commands, outside all installation-owned roots.
        for reserved in ('/opt/devoption', PLATFORM, PROXY, APP_STATE, STATE, '/etc/devoption', '/etc/caddy'):
            root, selected = Path(reserved), Path(value)
            if selected == root or root in selected.parents or selected in root.parents:
                raise ValueError('Prepared inputs must be separate from installed storage: ' + key)
    roots = [Path(config[key]) for key in ('toolchain', 'vendor', 'archives')]
    if any(a == b or a in b.parents or b in a.parents for i, a in enumerate(roots) for b in roots[i+1:]):
        raise ValueError('Prepared toolchain, vendor and archive roots must be disjoint.')
    return config


class AppHost(Host):
    def app_preflight(self, resumed):
        for tool in ('visudo', 'systemd-run'):
            self.run(tool, '--version')
        if not resumed:
            for unit in ('devoption-build.service', 'devoption-app.service', 'devoption-app-validate.service'):
                result = self.run('systemctl', 'show', unit, '-p', 'LoadState', '--value')
                if result.stdout.strip() != b'not-found':
                    raise ValueError('Existing build/app unit conflicts with fresh installation: ' + unit)
            self.free_port(15000)

    def validate_app(self, binary, toolchain, vendor):
        # All executable probes run only after digest checks and as the nonroot account.
        version = tomllib.loads((REPOSITORY / 'Cargo.toml').read_text())['workspace']['package']['version']
        result = self.run('runuser', '-u', 'devoption', '--', binary, '--version')
        if result.stdout != f'devoption {version}\n'.encode() or result.stderr:
            raise ValueError('Installed platform version differs from the reviewed checkout.')
        for name in ('rustc', 'cargo'):
            result = self.run('runuser', '-u', 'devoption', '--', str(Path(toolchain) / 'bin' / name), '--version')
            if not result.stdout.startswith(f'{name} 1.99.0 '.encode()) or result.stderr:
                raise ValueError('Prepared Rust/Cargo 1.99.0 is required.')
        for root in (toolchain, vendor):
            result = self.run('runuser', '-u', 'devoption', '--', binary, 'build-input-digest', root)
            if result.stdout.decode().strip() != input_tree(Path(root)):
                raise ValueError('Installed input identity or service-user readability differs.')
        self.run('visudo', '-c', '-f', SUDOERS)
        self.run('visudo', '-c')


class AppInstaller(Installer):
    """Reuse the no-replacement installer filesystem engine, not bundle authenticity.

    The distinct receipt binds the source configuration and all staged bytes.
    Its identity cannot match a bundle receipt, so neither mode adopts the other.
    """
    def __init__(self, root, host, config, repository=REPOSITORY):
        self.root, self.host, self.config = root, host, config
        self.repository = repository
        self.hostname = config['hostname']
        self.version = tomllib.loads((repository / 'Cargo.toml').read_text())['workspace']['package']['version']
        self.release = '/opt/devoption/source/' + config['source_commit']
        self.binary = self.release + '/bin/devoption'
        self.runtime = self.release + '/bin/devoption-runtime'
        self.caddy = '/opt/devoption/proxy/caddy-' + CADDY_VERSION
        binary = snapshot(Path(config['binary']), MAX_BINARY)
        runtime = snapshot(Path(config['runtime']), MAX_BINARY)
        for key, content in (('binary_sha256', binary), ('runtime_sha256', runtime)):
            if sha256(content) != config[key]:
                raise ValueError('Prepared executable differs from the reviewed SHA-256: ' + key)
        self.verify_inputs()
        proxy_manifest = {'external_requirements': {'proxy': {'version': CADDY_VERSION, 'sha512': CADDY_SHA512}}}
        caddy = proxy_binary(Path(config['caddy_archive']), proxy_manifest)
        proxy_unit = snapshot(repository / 'examples/systemd/caddy-devoption.service', 16384)
        proxy_unit = proxy_unit.replace(b'/usr/local/bin/caddy', self.caddy.encode())
        proxy_config = snapshot(repository / 'examples/caddy/Caddyfile', 16384)
        proxy_config = proxy_config.replace(b'platform.example.com', self.hostname.encode())
        self.output = {self.binary: (binary, 0o755), self.runtime: (runtime, 0o755),
                       self.caddy: (caddy, 0o755), '/etc/caddy/Caddyfile': (proxy_config, 0o644),
                       '/etc/systemd/system/caddy-devoption.service': (proxy_unit, 0o644),
                       SUDOERS: (SUDO_RULE, 0o440)}
        for source, name in HELPERS.items():
            self.output['/usr/local/libexec/' + name] = (snapshot(repository / 'scripts' / source, 131072), 0o755)
        self.identity = {'schema_version': 1, 'kind': 'prepared-source-app-install', 'config': config,
                         'version': self.version, 'proxy_sha512': CADDY_SHA512,
                         'files': {name: sha256(data) for name, (data, _) in self.output.items()},
                         'service_generator_sha256': sha256(snapshot(repository / 'scripts/prepare_platform_service.py', 65536))}
        self.identity['installer_sha256'] = {
            name: sha256(snapshot(repository / 'scripts' / name, 131072))
            for name in ('install_app.py', 'install_bundle.py', 'installer_host.py', 'prepare_build_config.py')}
        self.dirs = {'/opt/devoption': (0o755, 'root'), '/etc/devoption': (0o755, 'root'),
                     '/etc/caddy': (0o755, 'root'), '/usr/local/libexec': (0o755, 'root'),
                     '/etc/sudoers.d': (0o750, 'root'), APP_STATE: (0o700, 'root'),
                     PLATFORM: (0o700, 'devoption'), PROXY: (0o700, 'caddy'),
                     PROXY + '/data': (0o700, 'caddy'), PROXY + '/config': (0o700, 'caddy')}
        for name in ('data', 'repositories', 'history', 'jobs'):
            self.dirs[PLATFORM + '/' + name] = (0o700, 'devoption')
        for name in self.output:
            parent = Path(name).parent
            while str(parent).startswith('/opt/devoption/'):
                self.dirs[str(parent)] = (0o755, 'root')
                parent = parent.parent
        # Shared standard directories keep their existing safe modes; never chmod them.
        for name in ('/usr/local/libexec', '/etc/sudoers.d'):
            path = self.path(name)
            if path.exists() and not path.is_symlink():
                self.dirs[name] = (stat.S_IMODE(path.stat().st_mode), 'root')
        self.prepare_output()

    def verify_inputs(self):
        for key in ('toolchain', 'vendor'):
            if input_tree(Path(self.config[key]), self.host.root_uid) != self.config[key + '_sha256']:
                raise ValueError('Prepared tree differs from reviewed identity: ' + key)
        lock = Path(self.config['lockfile'])
        if sha256(snapshot(lock, 2 * 1024**2)) != self.config['lock_sha256']:
            raise ValueError('Dependency lockfile differs from reviewed identity.')
        verify_vendor(lock, Path(self.config['vendor']), Path(self.config['archives']))
        if not list((Path(self.config['toolchain']) / 'lib/rustlib/wasm32-wasip2/lib').glob('*.rlib')):
            raise ValueError('Prepared toolchain requires the wasm32-wasip2 standard library.')
        for name in ('rustc', 'cargo'):
            tool = Path(self.config['toolchain']) / 'bin' / name
            if not tool.is_file() or tool.stat().st_mode & 0o005 != 0o005:
                raise ValueError('Prepared toolchain requires readable executable bin/' + name)

    def prepare_output(self):
        account = self.host.account('devoption')
        # A fresh plan uses a placeholder; the actual allocated UID is rendered
        # after account creation, before writing any generated configuration.
        uid = account[0] if account else 1
        build = {'uid': uid, 'work': PLATFORM + '/jobs', 'owner_data': PLATFORM + '/data',
                 'repositories': PLATFORM + '/repositories', 'history': PLATFORM + '/history',
                 **{key: self.config[key] for key in ('toolchain', 'vendor', 'toolchain_sha256', 'vendor_sha256', 'lock_sha256')}}
        deployment = {'uid': uid, 'build_work': build['work'], 'state': APP_STATE, 'worker': self.runtime,
                      'port': 15000, **{key: build[key] for key in ('owner_data', 'repositories', 'history')}}
        unit = render(build, deployment, self.binary, 'devoption', self.hostname, 3000)
        unit = unit.replace('--repository-root ', '--managed-repository-root ')
        def encode(table):
            return ''.join(f'{key} = {json.dumps(value)}\n' for key, value in table.items()).encode()
        self.output['/etc/devoption/build.toml'] = (encode(build), 0o644)
        self.output['/etc/devoption/deployment.toml'] = (encode(deployment), 0o644)
        self.output['/etc/systemd/system/devoption-public.service'] = (unit.encode(), 0o644)

    def preflight(self):
        self.verify_inputs()
        resumed = self.receipt()
        if not resumed:
            for name in ('/etc/devoption', APP_STATE, SUDOERS, *('/usr/local/libexec/' + n for n in HELPERS.values())):
                if self.path(name).exists() or self.path(name).is_symlink():
                    raise ValueError('Existing app installation must be preserved; no conversion: ' + name)
        self.host.app_preflight(resumed)
        plan = super().preflight()
        # Configuration is an owned inventory too, unlike persistent app/user data.
        directory = self.path('/etc/devoption')
        if directory.exists():
            if any('/etc/devoption/' + p.name not in self.output for p in directory.iterdir()):
                raise ValueError('Unexpected installed configuration; preserve it and inspect.')
        plan.update(installation_kind=self.identity['kind'], app_capable=True,
                    app_ingress_configured=False, platform_url='https://' + self.hostname,
                    next_action=('apply' if not resumed or plan['missing_files'] else
                                 'enroll' if not plan['owner_enrolled'] else 'start'),
                    prepared_inputs_copied=False, apply_starts_services=False,
                    accounts_to_create=[name for name in ('devoption', 'caddy') if self.host.account(name) is None],
                    storage_roots={key: str(value) for key, value in {
                        'owner': PLATFORM + '/data', 'repositories': PLATFORM + '/repositories',
                        'history': PLATFORM + '/history', 'jobs': PLATFORM + '/jobs',
                        'releases': APP_STATE}.items()})
        return plan

    def runtime_probe(self):
        self.verify_inputs()
        self.host.runtime_probe(self.runtime, self.version)
        self.host.validate_app(self.binary, self.config['toolchain'], self.config['vendor'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('plan', 'apply', 'enroll', 'start', 'status'))
    parser.add_argument('--config', required=True, type=Path)
    args = parser.parse_args()
    try:
        config = load_config(args.config)
        host = AppHost()
        host.preflight(Path('/'), MIN_FREE)
        installer = AppInstaller(Path('/'), host, config)
        if args.mode in ('plan', 'status'):
            result = installer.preflight()
        else:
            with installation_lock('/run/lock/devoption-installer.lock'):
                if args.mode == 'apply':
                    result = installer.install()
                elif args.mode == 'enroll':
                    installer.enroll()
                    result = installer.preflight()
                else:
                    installer.start()
                    result = installer.preflight()
        print(json.dumps(result, indent=2))
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError, tarfile.TarError) as error:
        parser.exit(1, f'App installation stopped: {error}\n')


if __name__ == '__main__':
    main()
