#!/usr/bin/python3 -I
"""Fixed privileged adapter for the opt-in project-scoped Linux runtime.

No caller-selected commands, units, ports or filesystem paths. Installation owns
configuration, immutable release copies and durable intent. UI approvals remain
in the authenticated controller; this broker accepts only its configured UID.
"""
import fcntl
import hashlib
import http.client
import json
import os
from pathlib import Path
import re
import stat
import socket
import ssl
import ipaddress
import pwd
import subprocess
import sys
import time
import tomllib
import select
import tempfile
import urllib.parse

CONFIG = Path('/etc/devoption/deployment.toml')
BROKER = Path('/usr/local/libexec/devoption-deployment-broker')
UNIT = 'devoption-app.service'
VALIDATE_UNIT = 'devoption-app-validate.service'
POLICY = 'WasiHttpV1'
MAX_COMPONENT = 4 * 1024 * 1024
HEX = re.compile(r'[0-9a-f]{64}\Z')
ID = re.compile(r'[0-9a-f]{32}\Z')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def read_regular(path, limit, uid=None, dir_fd=None):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, dir_fd=dir_fd)
    with os.fdopen(fd, 'rb') as stream:
        before = os.fstat(stream.fileno())
        if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size > limit:
            raise ValueError('invalid file')
        if uid is not None and (before.st_uid != uid or before.st_mode & 0o022):
            raise ValueError('invalid owner or permissions')
        data = stream.read(limit + 1)
        after = os.fstat(stream.fileno())
        fields = ("st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_nlink", "st_size", "st_mtime_ns", "st_ctime_ns")
        if any(getattr(before, key) != getattr(after, key) for key in fields) or len(data) != before.st_size:
            raise ValueError('file changed')
        return data


def protected(path, uid=0):
    if not path.is_absolute() or path.resolve() != path:
        raise ValueError('noncanonical path')
    for part in [path, *path.parents]:
        info = part.lstat()
        if info.st_uid != uid or info.st_mode & 0o022 or stat.S_ISLNK(info.st_mode):
            raise ValueError('unprotected path')


def hostname(value):
    if not isinstance(value, str) or len(value) > 253 or '.' not in value:
        raise ValueError('use a lowercase ASCII DNS hostname')
    if not re.search(r'[a-z]', value.rsplit('.', 1)[-1]) or not all(re.fullmatch(r'[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?', label) for label in value.split('.')):
        raise ValueError('invalid DNS hostname')
    try: ipaddress.ip_address(value)
    except ValueError: return value
    raise ValueError('IP addresses are not hostnames')


def validate_ingress(binding, app_port):
    required = {'platform_hostname', 'app_hostname', 'project', 'platform_port'}
    if not isinstance(binding, dict) or set(binding) - {'ca_file'} != required:
        raise ValueError('unexpected hostname binding fields')
    platform = hostname(binding['platform_hostname'])
    app = hostname(binding['app_hostname'])
    if platform == app:
        raise ValueError('platform and app hostnames must differ')
    if not isinstance(binding['project'], str) or not ID.fullmatch(binding['project']):
        raise ValueError('hostname requires one exact project identity')
    port = binding['platform_port']
    if type(port) is not int or not 1024 <= port <= 65535 or port == app_port:
        raise ValueError('platform and app require distinct private ports')
    if 'ca_file' in binding and (not isinstance(binding['ca_file'], str) or not re.fullmatch(r'/[A-Za-z0-9_./-]+', binding['ca_file'])):
        raise ValueError('invalid explicit rehearsal CA path')


def ingress_status(config, record):
    binding = config.get('ingress')
    if not binding: return None
    result = {'project': binding['project'], 'url': 'https://' + binding['app_hostname']}
    def report(message):
        return {**result, 'status': message}
    if not record or record['project'] != binding['project'] or record['intent'] != 'running':
        return report('App stopped or not deployed. Review and deploy a saved release before checking HTTPS.')
    if not isinstance(record.get('invocation'), str) or not ID.fullmatch(record['invocation']):
        return report('HTTPS route does not match the selected app. Check the exact hostname, upstream and release health.')
    try:
        context = ssl.create_default_context(cafile=binding.get('ca_file'))
        # Probe only the local ingress. DNS never chooses a destination for this privileged process.
        with socket.create_connection(('127.0.0.1', 443), timeout=2) as raw:
            with context.wrap_socket(raw, server_hostname=binding['app_hostname']) as stream:
                stream.sendall(('GET /healthz HTTP/1.1\r\nHost: ' + binding['app_hostname'] + '\r\nConnection: close\r\n\r\n').encode())
                response = http.client.HTTPResponse(stream); response.begin()
                body = response.read(257)
                if response.status == 502 or response.status == 503:
                    return report('HTTPS proxy cannot reach the app. Check its private upstream port and worker status.')
                if response.status != 200 or body != record['health'].encode() or response.getheader('X-DevOption-Invocation') != record.get('invocation'):
                    return report('HTTPS route does not match the selected app. Check the exact hostname, upstream and release health.')
        label = 'Local HTTPS route verified with the explicit rehearsal CA.' if binding.get('ca_file') else 'Local HTTPS route and certificate verified.'
        return report(label + ' Public DNS and reachability are not checked here.')
    except ssl.SSLCertVerificationError:
        return report('Certificate verification failed. Check certificate hostname, expiry, Caddy issuance/renewal logs and trust chain; do not bypass TLS verification.')
    except (OSError, ssl.SSLError, http.client.HTTPException):
        return report('Local HTTPS ingress unavailable. Check Caddy on port 443, its TLS configuration and service logs.')


def deployment_slots(config):
    """Validate the operator schema without provisioning paths or reading grants."""
    common = {'uid', 'build_work', 'state', 'worker', 'owner_data', 'history', 'repositories'}
    if 'apps' not in config:
        if set(config) - {'ingress', 'records', 'https'} != common | {'port'}:
            raise ValueError('unexpected configuration')
        return [dict(config)]
    if set(config) != common | {'apps'} or not isinstance(config['apps'], list) or not 1 <= len(config['apps']) <= 8:
        raise ValueError('expected one to eight explicit app slots')
    slots, projects, ports, hosts = [], set(), set(), set()
    origin = None
    for app in config['apps']:
        if not isinstance(app, dict) or set(app) - {'records', 'https'} != {'project', 'port', 'ingress'}:
            raise ValueError('each app requires project, port and ingress')
        project, port = app['project'], app['port']
        if not isinstance(project, str) or not ID.fullmatch(project) or project in projects:
            raise ValueError('duplicate or invalid app project')
        if type(port) is not int or not 1024 <= port <= 65535 or port in ports:
            raise ValueError('duplicate or invalid app listener')
        validate_ingress(app['ingress'], port)
        binding = app['ingress']
        pair = (binding['platform_hostname'], binding['platform_port'])
        if binding['project'] != project or binding['app_hostname'] in hosts or (origin is not None and pair != origin):
            raise ValueError('ambiguous project or hostname binding')
        origin = pair
        projects.add(project); ports.add(port); hosts.add(binding['app_hostname'])
        slot = {key: config[key] for key in common}
        slot.update({key: value for key, value in app.items() if key != 'project'})
        slot['state'] = str(Path(config['state']) / project)
        slots.append(slot)
    if origin[1] in ports or origin[0] in hosts:
        raise ValueError('platform and apps require distinct listeners and names')
    return slots


def configuration():
    protected(CONFIG)
    protected(BROKER)
    raw = read_regular(CONFIG, 65536, 0)
    config = tomllib.loads(raw.decode())
    slots = deployment_slots(config)
    if 'apps' not in config:
        return slot_configuration(slots[0], raw)
    root = Path(config['state'])
    protected(root)
    if not root.is_dir() or root.stat().st_mode & 0o777 != 0o700:
        raise ValueError('slot root must be root private')
    # Refuse silently abandoning an existing single-app installation. Operators
    # must stop and explicitly migrate its records first; no automatic authority.
    if any((root / name).exists() for name in ('desired.json', 'last-action.json')) or any(HEX.fullmatch(p.name) for p in root.iterdir()):
        raise ValueError('legacy deployment state requires explicit migration')
    configured = []
    for app, slot in zip(config['apps'], slots):
        selected = slot_configuration(slot, canonical(slot))
        project = app['project']
        selected.update(project=project, unit='devoption-app-'+project+'.service',
                        validate_unit='devoption-app-'+project+'-validate.service',
                        runtime_directory='devoption-app-'+project)
        configured.append(selected)
    identities = [slot['records']['identity']['uid'] for slot in configured if slot.get('records')]
    if len(identities) != len(set(identities)):
        raise ValueError('apps require separate record service identities')
    return {'state': root, 'apps': configured}


def slot_configuration(config, raw):
    if type(config['uid']) is not int or config['uid'] <= 0 or int(os.environ.get('SUDO_UID', '-1')) != config['uid']:
        raise ValueError('caller not configured owner')
    if type(config['port']) is not int or not 1024 <= config['port'] <= 65535:
        raise ValueError('invalid private port')
    for key in ['build_work', 'state', 'worker', 'owner_data', 'history', 'repositories']:
        path = Path(config[key])
        if not re.fullmatch(r'/[A-Za-z0-9_./-]+', str(path)) or path.resolve() != path or not path.exists():
            raise ValueError('invalid path')
        config[key] = path
    protected(config['state'])
    protected(config['worker'])
    if config['state'].stat().st_mode & 0o777 != 0o700:
        raise ValueError('state must be root private')
    paths = [config[k] for k in ['state', 'build_work', 'owner_data', 'history', 'repositories']]
    if any(a == b or a in b.parents or b in a.parents for i, a in enumerate(paths) for b in paths[i+1:]):
        raise ValueError('deployment and platform storage must be disjoint')
    if config['state'] in config['worker'].parents:
        raise ValueError('worker must be outside release storage')
    trust = b''
    if 'ingress' in config:
        validate_ingress(config['ingress'], config['port'])
        if 'ca_file' in config['ingress']:
            protected(Path(config['ingress']['ca_file']))
            trust = read_regular(Path(config['ingress']['ca_file']), 65536, 0)
    record_bytes = b''
    if 'records' in config:
        binding = config['records']
        if not isinstance(binding, dict) or set(binding) != {'grant_file'}:
            raise ValueError('unexpected record binding')
        grant_path = Path(binding['grant_file'])
        if not re.fullmatch(r'/[A-Za-z0-9_./-]+', str(grant_path)):
            raise ValueError('invalid record grant path')
        protected(grant_path)
        record_bytes = read_regular(grant_path, 32768, 0)
        grant = json.loads(record_bytes)
        if not isinstance(grant, dict) or not ID.fullmatch(grant.get('app_id', '')):
            raise ValueError('invalid record app identity')
        if config.get('ingress') and grant['app_id'] != config['ingress']['project']:
            raise ValueError('record app must match hostname binding')
        identity = record_identity(grant, config['uid'])
        config['records'] = {'grant_file': grant_path, 'bytes': record_bytes,
                             'grant': grant, 'identity': identity}
        record_bytes += canonical(identity)
    if 'https' in config:
        validate_https_policy(config['https'])
        if config.get('ingress') and config['https']['project'] != config['ingress']['project']:
            raise ValueError('HTTPS project must match hostname binding')
    config['fingerprint'] = digest(raw + trust + record_bytes + read_regular(BROKER, 131072, 0) + read_regular(config['worker'], 256*1024*1024, 0))
    return config


def validate_https_policy(policy):
    if not isinstance(policy, dict) or set(policy) != {'project', 'environment', 'secret', 'revision', 'endpoint'}:
        raise ValueError('invalid HTTPS policy')
    if not isinstance(policy['project'], str) or not ID.fullmatch(policy['project']) or policy['environment'] not in ('development', 'preview', 'production'):
        raise ValueError('invalid HTTPS scope')
    if not isinstance(policy['secret'], str) or not re.fullmatch(r'[A-Z_][A-Z0-9_]{0,63}', policy['secret']) or type(policy['revision']) is not int or not 1 <= policy['revision'] <= 2**64-1:
        raise ValueError('invalid HTTPS secret binding')
    endpoint = policy['endpoint']
    if not isinstance(endpoint, str) or len(endpoint) > 512:
        raise ValueError('invalid HTTPS endpoint')
    # Canonical DNS host and a fixed literal path. No IPs, ports, userinfo, query,
    # fragment, encoded paths or path normalization differences between clients.
    if not re.fullmatch(r'https://[a-z0-9.-]+/[A-Za-z0-9/_-]*', endpoint):
        raise ValueError('invalid HTTPS endpoint')
    url = urllib.parse.urlsplit(endpoint)
    hostname(url.hostname)
    if len(url.path) > 256:
        raise ValueError('invalid HTTPS path')
    return policy


def https_policy(config, project):
    policy = config.get('https')
    return dict(policy) if policy and policy['project'] == project else None


def check_https_release(config, manifest):
    if manifest.get('https') and manifest['https'] != https_policy(config, manifest['project']):
        raise ValueError('HTTPS grant revoked or changed; promote and review a new release')


def read_delivery():
    deadline = time.monotonic()+2
    data = bytearray()
    while True:
        remaining = deadline-time.monotonic()
        if remaining <= 0 or not select.select([sys.stdin], [], [], remaining)[0]:
            raise ValueError('credential delivery unavailable')
        chunk = os.read(sys.stdin.fileno(), 32769-len(data))
        if not chunk:
            break
        data.extend(chunk)
        if len(data) > 32768:
            raise ValueError('credential delivery too large')
    if not data:
        return None
    try:
        value = json.loads(data)
    except (ValueError, UnicodeError):
        raise ValueError('invalid credential delivery') from None
    finally:
        data[:] = b'\0'*len(data)
    return value


def public_addresses(host):
    # A separate fixed resolver process gives libc DNS a hard deadline. No shell,
    # credentials, caller paths or environment are inherited by the resolver.
    source = "import json,socket,sys; a=sorted({v[4][0] for v in socket.getaddrinfo(sys.argv[1],443,socket.AF_INET,socket.SOCK_STREAM)}); print(json.dumps(a[:17]))"
    try:
        result = subprocess.run(['/usr/bin/python3', '-I', '-c', source, host], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env={'PATH':'/usr/bin:/bin'}, timeout=3, check=True)
        return json.loads(result.stdout)
    except (OSError, ValueError, subprocess.SubprocessError):
        raise ValueError('HTTPS endpoint resolution unavailable') from None


def resolve_https(config, manifest, delivery):
    policy = manifest.get('https')
    if not policy:
        if delivery is not None:
            raise ValueError('unexpected credential delivery')
        return None
    check_https_release(config, manifest)
    if not isinstance(delivery, dict) or set(delivery) != {'policy', 'value'} or delivery['policy'] != policy:
        raise ValueError('HTTPS credential binding changed')
    validate_https_policy(delivery['policy'])
    value = delivery['value']
    if not isinstance(value, str) or not re.fullmatch(r'[\x21-\x7e]{1,8192}', value):
        raise ValueError('HTTPS credential is not a bounded bearer token')
    # DNS is resolved once by the broker, before the running app is disturbed.
    # The worker pins only these addresses; the guest never receives DNS access.
    addresses = public_addresses(urllib.parse.urlsplit(policy['endpoint']).hostname)
    if not 1 <= len(addresses) <= 16 or any(not ipaddress.IPv4Address(ip).is_global or ipaddress.IPv4Address(ip).is_multicast or ipaddress.IPv4Address(ip) in ipaddress.IPv4Network('192.88.99.0/24') for ip in addresses):
        raise ValueError('HTTPS endpoint did not resolve exclusively to public IPv4 addresses')
    return {'policy': policy, 'addresses': addresses, 'value': value}


def record_identity(grant, owner_uid):
    user = grant.get('user')
    if not isinstance(user, str) or not re.fullmatch(r'[a-z_][a-z0-9_]{0,30}', user):
        raise ValueError('invalid record service user')
    account = pwd.getpwnam(user)
    if account.pw_uid < 100 or account.pw_uid == owner_uid or account.pw_shell not in ('/usr/sbin/nologin', '/sbin/nologin', '/bin/false'):
        raise ValueError('record worker requires a distinct non-login service identity')
    directory = Path(grant.get('socket_dir', ''))
    if not directory.is_absolute() or directory.resolve() != directory or len(str(directory)) > 80:
        raise ValueError('invalid record socket directory')
    groups = os.getgrouplist(user, account.pw_gid)
    if account.pw_gid == 0 or set(groups) != {account.pw_gid}:
        raise ValueError('record service identity must have only its dedicated primary group')
    for path in [directory, *directory.parents]:
        info = path.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid in (owner_uid, account.pw_uid) or info.st_mode & 0o002 or (info.st_gid in groups and info.st_mode & 0o020):
            raise ValueError('record socket path must not be writable by platform/app identities')
    return {'user': user, 'uid': account.pw_uid, 'gid': account.pw_gid}


def record_policy(config, project):
    binding = config.get('records')
    if not binding or binding['grant']['app_id'] != project:
        return None
    return {'version': 1, 'grant_sha256': digest(binding['bytes']), **binding['identity']}


def check_record_release(config, manifest):
    policy = manifest.get('records')
    if policy is None:
        return
    if policy != record_policy(config, manifest['project']):
        raise ValueError('record grant revoked or changed; promote and review a new release')


def atomic(path, value):
    temporary = path.with_suffix('.next')
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        with os.fdopen(fd, 'wb') as stream:
            stream.write(canonical(value)); stream.flush(); os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try: os.fsync(directory)
        finally: os.close(directory)
    finally:
        temporary.unlink(missing_ok=True)


def command(args, timeout=10, check=True):
    return subprocess.run(args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, timeout=timeout, check=check,
                          env={'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LANG': 'C'})


def unit_state(unit=UNIT):
    result = command(['/usr/bin/systemctl', 'show', unit, '--property=ActiveState,SubState,ControlGroup,InvocationID'], 3)
    return dict(line.split('=', 1) for line in result.stdout.decode().splitlines() if '=' in line)


def stop_unit(unit=UNIT):
    command(['/usr/bin/systemctl', 'stop', unit], 8, False)
    observed = unit_state(unit)
    if observed.get('ActiveState') not in ('inactive', 'failed') or observed.get('ControlGroup'):
        raise ValueError('owned unit stop is unconfirmed')
    command(['/usr/bin/systemctl', 'reset-failed', unit], 3, False)


def load_release(config, release):
    if not HEX.fullmatch(release):
        raise ValueError('invalid release')
    directory = config['state'] / release
    protected(directory)
    manifest_bytes = read_regular(directory / 'manifest.json', 16384, 0)
    if digest(manifest_bytes) != release:
        raise ValueError('release manifest changed')
    manifest = json.loads(manifest_bytes)
    if config.get('project') and manifest['project'] != config['project']:
        raise ValueError('release belongs to another project')
    component = read_regular(directory / 'component.wasm', MAX_COMPONENT, 0)
    if digest(component) != manifest['artifact_sha256'] or len(component) != manifest['artifact_bytes']:
        raise ValueError('release artifact changed')
    if 'records' in manifest:
        policy = manifest['records']
        if not isinstance(policy, dict) or set(policy) != {'version', 'grant_sha256', 'user', 'uid', 'gid'} or type(policy['version']) is not int or policy['version'] != 1 or not isinstance(policy.get('grant_sha256'), str) or not HEX.fullmatch(policy['grant_sha256']) or type(policy['uid']) is not int or policy['uid'] < 100 or type(policy['gid']) is not int or policy['gid'] <= 0 or not isinstance(policy['user'], str):
            raise ValueError('invalid saved record policy')
        grant = read_regular(directory/'records.json', 32768, 0)
        if digest(grant) != policy['grant_sha256']:
            raise ValueError('saved record grant changed')
    if 'https' in manifest:
        validate_https_policy(manifest['https'])
        if manifest['https']['project'] != manifest['project']:
            raise ValueError('saved HTTPS project changed')
    return manifest


def read_build(base, uid, job):
    # Build staging identifiers differ from job IDs. Search only bounded private
    # child directories, retaining descriptors through receipt/component reads.
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
    root = os.open('/', flags)
    try:
        for part in base.parts[1:]:
            child = os.open(part, flags, dir_fd=root)
            os.close(root); root = child
        info = os.fstat(root)
        if info.st_uid != uid or info.st_mode & 0o077:
            raise ValueError('invalid build root')
        entries = os.listdir(root)
        if len(entries) > 256: raise ValueError('build staging limit exceeded')
        found = None
        for entry in entries:
            if not ID.fullmatch(entry): continue
            directory = os.open(entry, flags, dir_fd=root)
            artifact = None
            try:
                info = os.fstat(directory)
                if info.st_uid != uid or info.st_mode & 0o077: raise ValueError('invalid job directory')
                try: artifact = os.open('artifact', flags, dir_fd=directory)
                except FileNotFoundError: continue
                info = os.fstat(artifact)
                if info.st_uid != uid or info.st_mode & 0o077: raise ValueError('invalid artifact directory')
                try: receipt = tomllib.loads(read_regular('receipt.toml', 16384, uid, artifact).decode())
                except FileNotFoundError: continue
                if receipt.get('job') != job: continue
                if found is not None: raise ValueError('duplicate build receipt')
                found = (receipt, read_regular('component.wasm', MAX_COMPONENT, uid, artifact))
            finally:
                if artifact is not None: os.close(artifact)
                os.close(directory)
        if found is None: raise ValueError('saved build artifact missing')
        return found
    finally: os.close(root)


def promote(config, job, expected, provenance):
    if not ID.fullmatch(job) or not HEX.fullmatch(expected):
        raise ValueError('invalid build identity')
    if config.get('project') and provenance[0] != config['project']:
        raise ValueError('build belongs to another project')
    base = config['build_work']
    receipt, component = read_build(base, config['uid'], job)
    if tuple(str(receipt.get(key)) for key in ['project', 'repository', 'commit', 'source_sha256', 'artifact_bytes']) != provenance:
        raise ValueError('build provenance changed')
    if receipt.get('version') != 2 or receipt.get('job') != job or receipt.get('artifact') != 'component.wasm':
        raise ValueError('invalid build receipt')
    if receipt.get('artifact_sha256') != expected or digest(component) != expected or len(component) != receipt.get('artifact_bytes'):
        raise ValueError('build artifact changed')
    if not component.startswith(b'\0asm\x0d\0\x01\0'):
        raise ValueError('not a component')
    for key in ['project', 'repository']:
        if not ID.fullmatch(receipt[key]): raise ValueError('invalid association')
    if set(receipt) != {'version', 'job', 'project', 'repository', 'commit', 'source_sha256', 'review_sha256', 'artifact_sha256', 'artifact_bytes', 'artifact', 'toolchain_sha256', 'dependency_sha256', 'configuration_sha256', 'build_policy', 'target', 'abi'}:
        raise ValueError('unsupported build receipt; rebuild with the current platform')
    if (receipt['build_policy'], receipt['target'], receipt['abi']) != ('WasmBuildV1', 'wasm32-wasip2', 'wasi:http/proxy@0.2.4'):
        raise ValueError('unsupported build contract')
    for key in ['source_sha256', 'review_sha256', 'toolchain_sha256', 'dependency_sha256', 'configuration_sha256']:
        if not HEX.fullmatch(receipt[key]): raise ValueError('invalid provenance')
    if not re.fullmatch(r'[0-9a-f]{40}', receipt['commit']): raise ValueError('invalid commit')
    manifest = {**receipt, 'policy': POLICY, 'runtime': 'wasmtime-49.0.3'}
    records = record_policy(config, receipt['project'])
    if records:
        manifest['records'] = records
    https = https_policy(config, receipt['project'])
    if https:
        manifest['https'] = https
    encoded = canonical(manifest)
    release = digest(encoded)
    destination = config['state'] / release
    if destination.exists():
        load_release(config, release)
        return release
    if sum(1 for p in config['state'].iterdir() if HEX.fullmatch(p.name)) >= 32:
        raise ValueError('local release capacity reached')
    temporary = config['state'] / (release + '.pending')
    temporary.mkdir(mode=0o700)
    try:
        files = [('component.wasm', component), ('manifest.json', encoded)]
        if records:
            files.append(('records.json', config['records']['bytes']))
        for name, data in files:
            with (temporary / name).open('xb') as stream:
                os.chmod(stream.name, 0o444 if name in ('component.wasm', 'records.json') else 0o400); stream.write(data); stream.flush(); os.fsync(stream.fileno())
        fd = os.open(temporary, os.O_RDONLY)
        try: os.fsync(fd)
        finally: os.close(fd)
        os.rename(temporary, destination)
        fd = os.open(config['state'], os.O_RDONLY)
        try: os.fsync(fd)
        finally: os.close(fd)
    finally:
        if temporary.exists():
            for file in temporary.iterdir(): file.unlink()
            temporary.rmdir()
    return release


def launch(config, release, validate=False, delivery=None):
    manifest = load_release(config, release)
    check_record_release(config, manifest)
    check_https_release(config, manifest)
    unit = config.get('validate_unit', VALIDATE_UNIT) if validate else config.get('unit', UNIT)
    stop_unit(unit)
    mounts = [f'{config["state"] / release / "component.wasm"}:/component.wasm']
    properties = [
        'DynamicUser=yes', 'NoNewPrivileges=yes', 'CapabilityBoundingSet=',
        'ProtectSystem=strict', 'ProtectHome=yes', 'PrivateTmp=yes', 'PrivateDevices=yes',
        'ProtectKernelTunables=yes', 'ProtectKernelModules=yes', 'ProtectControlGroups=yes',
        'RestrictSUIDSGID=yes', 'LockPersonality=yes', 'RestrictRealtime=yes',
        'CPUQuota=100%', 'MemoryMax=536870912', 'MemorySwapMax=0', 'TasksMax=64',
        'LimitCORE=0', 'TimeoutStopSec=5', 'KillMode=control-group', 'Restart=no', 'UMask=0077',
        'RestrictAddressFamilies=AF_INET', 'IPAddressDeny=any', 'IPAddressAllow=localhost',
        'SocketBindDeny=any', f'SocketBindAllow=ipv4:tcp:{config["port"]}',
        'InaccessiblePaths=' + ' '.join('-'+str(config[k]) for k in ['state', 'owner_data', 'history', 'repositories', 'build_work']),
        'RuntimeDirectory='+config.get('runtime_directory', 'devoption-app')+('-validate' if validate else ''),
        'RuntimeDirectoryPreserve=yes', 'StandardOutput=null', 'StandardError=null',
    ]
    if validate: properties += ['RuntimeMaxSec=30', 'PrivateNetwork=yes']
    if manifest.get('records'):
        identity = manifest['records']
        properties.remove('DynamicUser=yes')
        properties.remove('RestrictAddressFamilies=AF_INET')
        properties += [f'User={identity["uid"]}', f'Group={identity["gid"]}',
                       'RestrictAddressFamilies=AF_INET AF_UNIX']
        mounts.append(f'{config["state"] / release / "records.json"}:/records-grant.json')
        if not validate:
            mounts.append(config['records']['grant']['socket_dir'])
    if manifest.get('https') and not validate:
        if delivery is None:
            raise ValueError('HTTPS credential delivery missing')
        properties += ['IPAddressAllow='+ip+'/32' for ip in delivery['addresses']]
    properties.append('BindReadOnlyPaths='+' '.join(mounts))
    args = ['/usr/bin/systemd-run', '--quiet', '--collect', '--unit='+unit]
    if validate: args += ['--wait']
    if manifest.get('https'): args += ['--service-type=exec']
    for prop in properties: args += ['--property='+prop]
    credential = None
    try:
        if manifest.get('https'):
            if not validate:
                # Root-private volatile source; systemd copies into its read-only
                # per-unit credential mount. Type=exec waits for credential setup
                # before systemd-run returns and the source can be removed.
                # Never persist a value in releases.
                credential = tempfile.NamedTemporaryFile(prefix='devoption-https-', dir='/run', delete=False)
                os.fchmod(credential.fileno(), 0o600)
                credential.write(canonical(delivery)); credential.flush(); credential.close()
                args += ['--property=LoadCredential=https.json:'+credential.name]
            args += [str(config['worker']), 'capability-worker', '/component.wasm', str(config['port']), 'validate' if validate else 'serve', '/records-grant.json' if manifest.get('records') else '-', '-' if validate else f'/run/credentials/{unit}/https.json']
        else:
            args += [str(config['worker']), 'records-worker' if manifest.get('records') else 'app-worker', '/component.wasm', str(config['port']), 'validate' if validate else 'serve']
            if manifest.get('records'):
                args.append('/records-grant.json')
        command(args, 37 if validate else 5)
    finally:
        if credential is not None:
            Path(credential.name).unlink(missing_ok=True)


def health(config, expected, invocation):
    # A missing marker and a missing systemd identity must never match as None.
    if not isinstance(invocation, str) or not ID.fullmatch(invocation):
        return False
    connection = http.client.HTTPConnection('127.0.0.1', config['port'], timeout=1)
    try:
        connection.request('GET', '/healthz', headers={'Connection': 'close'})
        response = connection.getresponse()
        body = response.read(257)
        return response.status == 200 and body == expected.encode() and response.getheader('X-DevOption-Invocation') == invocation and unit_state(config.get('unit', UNIT)).get('InvocationID') == invocation
    except (OSError, http.client.HTTPException):
        return False
    finally: connection.close()


def desired(config):
    path = config['state'] / 'desired.json'
    return json.loads(read_regular(path, 16384, 0)) if path.exists() else None


def save(config, value):
    atomic(config['state'] / 'desired.json', value)


def start(config, release, expected, fingerprint, generation=None, delivery=None):
    if fingerprint != config['fingerprint']:
        raise ValueError('runtime configuration changed; review again')
    if not re.fullmatch(r'[\x20-\x7e]{1,128}', expected):
        raise ValueError('health body must be printable ASCII')
    if generation is not None and generation != digest(canonical(desired(config))):
        raise ValueError('deployment changed; review again')
    manifest = load_release(config, release)
    if config.get('project') and manifest['project'] != config['project']:
        raise ValueError('release belongs to another project')
    if config.get('ingress') and manifest['project'] != config['ingress']['project']:
        raise ValueError('hostname is bound to another project; operator reconfiguration required')
    resolved = resolve_https(config, manifest, delivery)
    # Validate candidate before disturbing a running release. Compilation is bounded
    # by the validation unit, but never instantiates application code here.
    try:
        launch(config, release, True)
    except Exception:
        atomic(config['state'] / 'last-action.json', {'project': manifest['project'], 'release': release, 'message': 'Candidate compatibility validation failed; current app was preserved.'})
        raise
    previous = desired(config)
    stop_unit(config.get('unit', UNIT))
    record = {'release': release, 'project': manifest['project'], 'health': expected,
              'fingerprint': fingerprint, 'intent': 'stopped', 'observed': 'starting',
              'previous': (previous.get('last_healthy') or previous.get('previous') or previous.get('release')) if previous else None,
              'last_healthy': previous.get('last_healthy') if previous else None}
    # Interrupted starts do not become permission to retry an unverified candidate.
    save(config, record)
    try:
        until = time.monotonic() + 30
        launch(config, release, delivery=resolved)
        while time.monotonic() < until:
            state = unit_state(config.get('unit', UNIT))
            if state.get('ActiveState') == 'active' and health(config, expected, state.get('InvocationID')):
                record.update(intent='running', observed='healthy', invocation=state.get('InvocationID'), last_healthy=release)
                save(config, record)
                atomic(config['state'] / 'last-action.json', {'project': manifest['project'], 'release': release, 'message': 'Deployment passed the exact health check.'})
                return
            if state.get('ActiveState') in ('failed', 'inactive'):
                break
            time.sleep(.2)
        raise ValueError('candidate health check failed')
    except Exception:
        stop_unit(config.get('unit', UNIT))
        record.update(intent='stopped', observed='failed')
        save(config, record)
        atomic(config['state'] / 'last-action.json', {'project': manifest['project'], 'release': release, 'message': 'Candidate startup or health failed; candidate was stopped. Review a saved release to recover.'})
        raise


def status(config):
    record = desired(config)
    state = unit_state(config.get('unit', UNIT))
    if record and record['intent'] == 'running':
        try: load_release(config, record['release'])
        except (ValueError, OSError): record['observed'] = 'integrity failed'
        else: record['observed'] = ('healthy' if record['fingerprint'] == config['fingerprint'] and state.get('InvocationID') == record.get('invocation') and state.get('ActiveState') == 'active' and health(config, record['health'], record.get('invocation')) else 'failed')
    releases = []
    for directory in sorted(config['state'].iterdir()):
        if HEX.fullmatch(directory.name):
            try: manifest = load_release(config, directory.name)
            except (ValueError, OSError): continue
            releases.append({'release': directory.name, **manifest})
    log = ''
    path = Path('/run') / config.get('runtime_directory', 'devoption-app') / 'worker.log'
    if path.exists():
        try: log = read_regular(path, 1024*1024).decode(errors='replace')[-65536:]
        except (ValueError, OSError): log = '[Log changed during inspection; refresh to read again.]'
    last_path = config['state'] / 'last-action.json'
    last = json.loads(read_regular(last_path, 16384, 0)) if last_path.exists() else None
    return {'project': config.get('project'), 'ingress': ingress_status(config, record), 'last_action': last, 'generation': digest(canonical(desired(config))), 'fingerprint': config['fingerprint'], 'port': config['port'], 'deployment': record, 'releases': releases, 'log': log}


def dispatch(config, args):
    if args[:1] == ['project']:
        if len(args) < 3 or not ID.fullmatch(args[1]):
            raise ValueError('invalid project operation')
        project, args = args[1], args[2:]
        if 'apps' in config:
            selected = next((slot for slot in config['apps'] if slot['project'] == project), None)
            if selected is None:
                if args == ['status']:
                    return {'unconfigured': True, 'releases': []}
                raise ValueError('project has no configured deployment slot')
            config = selected
        else:
            if args[:1] == ['start'] and load_release(config, args[1])['project'] != project:
                raise ValueError('release belongs to another project')
            if args[:1] == ['promote'] and (len(args) != 8 or args[3] != project):
                raise ValueError('build belongs to another project')
            if args[:1] in (['stop'], ['reconcile']) and desired(config) and desired(config)['project'] != project:
                raise ValueError('deployment belongs to another project')
    elif 'apps' in config:
        if args == ['status']:
            return {'apps': [status(slot) for slot in config['apps']]}
        raise ValueError('multi-app operations require an exact project')
    match args:
        case ['status']:
            return status(config)
        case ['promote', job, checksum, project, repository, commit, source, size]:
            return {'release': promote(config, job, checksum, (project, repository, commit, source, size))}
        case ['start', release, expected, fingerprint, generation]:
            delivery = read_delivery() if load_release(config, release).get('https') else None
            start(config, release, expected, fingerprint, generation, delivery)
        case ['stop']:
            record = desired(config)
            if record:
                record.update(intent='stopped', observed='stopping'); save(config, record)
            stop_unit(config.get('unit', UNIT))
            if record:
                record['observed'] = 'stopped'; save(config, record)
        case ['reconcile']:
            record = desired(config)
            stop_unit(config.get('unit', UNIT))
            stop_unit(config.get('validate_unit', VALIDATE_UNIT))
            if record and record['intent'] == 'running':
                try:
                    delivery = read_delivery() if load_release(config, record['release']).get('https') else None
                    start(config, record['release'], record['health'], record['fingerprint'], delivery=delivery)
                    resumed = desired(config)
                    resumed['previous'] = record.get('previous')
                    save(config, resumed)
                except Exception:
                    stop_unit(config.get('unit', UNIT))
                    record.update(intent='stopped', observed='recovery failed')
                    save(config, record)
        case _:
            raise ValueError('unsupported deployment operation')
    return {'ok': True}


def main():
    os.umask(0o077)
    if os.geteuid() != 0:
        raise ValueError('installed root broker required')
    config = configuration()
    with (config['state'] / 'broker.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        result = dispatch(config, sys.argv[1:])
    print(json.dumps(result))


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        print('Deployment operation failed: '+str(error), file=sys.stderr)
        sys.exit(1)
