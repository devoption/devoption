#!/usr/bin/python3 -I
"""Fixed privileged adapter for managed-node operations and saved-release export.

The separately installed, protected deployment broker still owns local runtime
behavior. Installing this adapter does not change existing broker fingerprints.
"""
import base64
import fcntl
import json
import os
from pathlib import Path
import re
import select
import stat
import subprocess
import sys
import tempfile
import time
import tomllib
import types


BROKER = Path('/usr/local/libexec/devoption-deployment-broker')
ADAPTER = Path('/usr/local/libexec/devoption-cluster-broker')
NODE_CONFIG = Path('/etc/devoption/cluster-node.toml')
MAX_COMPONENT = 4 * 1024 * 1024
MAX_PACKET = 6 * 1024 * 1024
HEX = re.compile(r'[0-9a-f]{64}\Z')
ID = re.compile(r'[0-9a-f]{32}\Z')


def protected(path):
    if not path.is_absolute() or path.resolve() != path:
        raise ValueError('noncanonical installed path')
    for part in (path, *path.parents):
        info = part.lstat()
        if info.st_uid != 0 or info.st_mode & 0o022 or stat.S_ISLNK(info.st_mode):
            raise ValueError('unprotected installation or state')


def local_broker():
    protected(ADAPTER)
    protected(BROKER)
    if not ADAPTER.is_file() or not BROKER.is_file():
        raise ValueError('expected installed regular broker files')
    # Compile protected source directly, without consulting a pre-existing cache.
    with BROKER.open('rb') as stream:
        code = stream.read(131073)
    if len(code) > 131072:
        raise ValueError('local broker exceeds the supported size')
    module = types.ModuleType('_devoption_local_broker')
    exec(compile(code, str(BROKER), 'exec'), module.__dict__)
    return module


def validate_receipt(runtime, receipt, component):
    fields = {'version', 'job', 'project', 'repository', 'commit', 'source_sha256',
              'review_sha256', 'artifact_sha256', 'artifact_bytes', 'artifact',
              'toolchain_sha256', 'dependency_sha256', 'configuration_sha256',
              'build_policy', 'target', 'abi'}
    if not isinstance(receipt, dict) or receipt.keys() != fields:
        raise ValueError('unsupported build receipt')
    if (type(receipt['version']) is not int or receipt['version'] != 2
            or receipt['artifact'] != 'component.wasm'
            or (receipt['build_policy'], receipt['target'], receipt['abi'])
            != ('WasmBuildV1', 'wasm32-wasip2', 'wasi:http/proxy@0.2.4')):
        raise ValueError('unsupported build contract')
    for key in ('job', 'project', 'repository'):
        require_hex(receipt[key], ID)
    for key in ('source_sha256', 'review_sha256', 'toolchain_sha256', 'dependency_sha256',
                'configuration_sha256', 'artifact_sha256'):
        require_hex(receipt[key], HEX)
    if not isinstance(receipt['commit'], str) or not re.fullmatch(r'[0-9a-f]{40}', receipt['commit']):
        raise ValueError('invalid commit')
    if (type(receipt['artifact_bytes']) is not int or len(component) != receipt['artifact_bytes']
            or len(component) > MAX_COMPONENT or not component.startswith(b'\0asm\x0d\0\x01\0')
            or runtime.digest(component) != receipt['artifact_sha256']):
        raise ValueError('invalid component framing or checksum')


def sync_directory(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def save_release(runtime, config, receipt, component, origin):
    manifest = {**receipt, 'policy': runtime.POLICY, 'runtime': 'wasmtime-49.0.4'}
    manifest['cluster_origin'] = origin
    records = runtime.record_policy(config, receipt['project'])
    if records:
        manifest['records'] = records
    encoded = runtime.canonical(manifest)
    release = runtime.digest(encoded)
    destination = config['state'] / release
    if destination.exists():
        runtime.load_release(config, release)
        return release
    if sum(1 for p in config['state'].iterdir() if HEX.fullmatch(p.name)) >= 32:
        raise ValueError('local release capacity reached')
    temporary = Path(tempfile.mkdtemp(prefix='.cluster-stage-', dir=config['state']))
    try:
        files = [('component.wasm', component), ('manifest.json', encoded)]
        if records:
            files.append(('records.json', config['records']['bytes']))
        for name, data in files:
            with (temporary / name).open('xb') as stream:
                os.fchmod(stream.fileno(), 0o400 if name == 'manifest.json' else 0o444)
                stream.write(data)
                stream.flush()
                os.fsync(stream.fileno())
        sync_directory(temporary)
        os.rename(temporary, destination)
        sync_directory(config['state'])
    finally:
        if temporary.exists():
            for file in temporary.iterdir():
                file.unlink()
            temporary.rmdir()
    return release


def export_release(runtime, config, release):
    manifest = runtime.load_release(config, release)
    if 'cluster_origin' in manifest:
        raise ValueError('export requires an original saved release')
    receipt = {k: v for k, v in manifest.items() if k not in ('policy', 'runtime', 'records', 'https')}
    component = runtime.read_regular(config['state'] / release / 'component.wasm', MAX_COMPONENT, 0)
    validate_receipt(runtime, receipt, component)
    return {'release': release, 'receipt': receipt, 'component': base64.b64encode(component).decode('ascii')}


def cluster_config(runtime, config):
    protected(NODE_CONFIG)
    membership = tomllib.loads(runtime.read_regular(NODE_CONFIG, 32768, 0).decode())
    if set(membership) != {'cluster', 'node', 'epoch', 'instances'}:
        raise ValueError('invalid node membership configuration')
    for key in ('cluster', 'node', 'epoch'):
        require_hex(membership[key], ID)
    slots = {slot['project']: slot for slot in config.get('apps', [])}
    grants = membership.pop('instances')
    if not isinstance(grants, dict) or not slots or grants.keys() != slots.keys():
        raise ValueError('every node slot must have one explicit origin project')
    adapter_digest = runtime.digest(runtime.read_regular(ADAPTER, 131072, 0))
    for instance, project in grants.items():
        require_hex(project, ID)
        if 'https' in slots[instance]:
            raise ValueError('remote HTTPS credential delivery is not supported')
        slots[instance]['cluster_source'] = project
        slots[instance]['fingerprint'] = runtime.digest(runtime.canonical(
            [slots[instance]['fingerprint'], membership, instance, project,
             adapter_digest]))
    return membership, slots


def require_hex(value, pattern):
    if not isinstance(value, str) or not pattern.fullmatch(value):
        raise ValueError('invalid identity or digest')
    return value


def cluster_origin_matches(slot, manifest):
    origin = manifest.get('cluster_origin')
    return (isinstance(origin, dict) and origin.get('project') == slot['cluster_source']
            and manifest.get('project') == slot['project'] and 'https' not in manifest)


def cluster_owned(runtime, slot):
    record = runtime.desired(slot)
    if record:
        if not cluster_origin_matches(slot, runtime.load_release(slot, record['release'])):
            raise ValueError('slot contains unmanaged or unverifiable deployment state')
    else:
        for unit in (slot['unit'], slot['validate_unit']):
            observed = runtime.unit_state(unit)
            if observed.get('ActiveState') not in ('inactive', 'failed') or observed.get('ControlGroup'):
                raise ValueError('slot contains an unrecorded active unit')


def cluster_stage(runtime, slot, package, fingerprint):
    if fingerprint != slot['fingerprint']:
        raise ValueError('node configuration changed')
    cluster_owned(runtime, slot)
    if not isinstance(package, dict) or set(package) != {'release', 'receipt', 'component'}:
        raise ValueError('invalid release package')
    release = require_hex(package['release'], HEX)
    receipt = package['receipt']
    if not isinstance(receipt, dict) or receipt.get('project') != slot['cluster_source']:
        raise ValueError('release belongs to another source project')
    blob = package['component']
    if not isinstance(blob, str) or len(blob) > 4 * ((MAX_COMPONENT + 2) // 3):
        raise ValueError('invalid component size')
    component = base64.b64decode(blob, validate=True)
    validate_receipt(runtime, receipt, component)
    origin = {'project': receipt['project'], 'release': release, 'receipt_sha256': runtime.digest(runtime.canonical(receipt))}
    result = save_release(runtime, slot, dict(receipt, project=slot['project']), component, origin)
    return {'release': result, 'origin': origin}


def cluster_status(runtime, slot, operation=None, logs=False):
    cluster_owned(runtime, slot)
    observed = runtime.status(slot, include_log=logs)
    releases = [
        {**{k: value[k] for k in ('release', 'commit', 'artifact_sha256', 'cluster_origin')},
         'record_grant_sha256': value.get('records', {}).get('grant_sha256')}
        for value in observed['releases'] if cluster_origin_matches(slot, value)
    ]
    result = {k: observed[k] for k in ('fingerprint', 'generation', 'deployment', 'ingress')}
    policy = runtime.record_policy(slot, slot['project'])
    grant = ({'sha256': policy['grant_sha256'], 'read': policy.get('allow_read', False),
              'jobs': 'jobs' in policy} if policy else None)
    result.update(releases=releases, record_grant=grant,
                  hostname=slot['ingress']['app_hostname'], source=slot['cluster_source'])
    if logs:
        result['log'] = observed['log']
    if operation is not None:
        path = slot['state'] / 'cluster-receipts' / require_hex(operation, ID)
        result['receipt'] = json.loads(runtime.read_regular(path, 4096, 0)) if path.exists() else None
    return result


def cluster_mutate(runtime, slot, binding, args):
    action = args['action']
    op = require_hex(args['operation'], ID)
    expected = require_hex(args['generation'], HEX)
    fingerprint = require_hex(args['fingerprint'], HEX)
    if type(args['expires']) is not int:
        raise ValueError('invalid approval expiry')
    receipts = slot['state'] / 'cluster-receipts'
    receipts.mkdir(mode=0o700, exist_ok=True)
    runtime.protected(receipts)
    path = receipts / op
    request_hash = runtime.digest(runtime.canonical([binding, slot['project'], args]))
    if path.exists():
        receipt = json.loads(runtime.read_regular(path, 4096, 0))
        if receipt['request'] != request_hash:
            raise ValueError('operation identity already used for different inputs')
        return receipt
    cluster_owned(runtime, slot)
    now = int(time.time())
    if not now < args['expires'] <= now + 310:
        raise ValueError('approval expired or node clock differs')
    if fingerprint != slot['fingerprint'] or expected != runtime.digest(runtime.canonical(runtime.desired(slot))):
        raise ValueError('destination changed after review')
    if not slot.get('ingress') or args['hostname'] != slot['ingress']['app_hostname']:
        raise ValueError('reviewed hostname changed')
    if action == 'start':
        manifest = runtime.load_release(slot, require_hex(args['release'], HEX))
        if not cluster_origin_matches(slot, manifest):
            raise ValueError('release is not bound to this deployment instance')
        if manifest.get('records') != runtime.record_policy(slot, slot['project']):
            raise ValueError('record grant changed; stage the release again')
        if not isinstance(args['health'], str) or not re.fullmatch(r'[\x20-\x7e]{1,128}', args['health']):
            raise ValueError('invalid health body')
    if sum(1 for _ in receipts.iterdir()) >= 4096:
        raise ValueError('operation history capacity reached')
    receipt = {'operation': op, 'request': request_hash, 'state': 'unknown'}
    runtime.atomic(path, receipt)
    try:
        if action == 'start':
            runtime.start(slot, args['release'], args['health'], fingerprint, expected)
        else:
            runtime.dispatch(slot, ['stop'])
    except (ValueError, OSError, subprocess.SubprocessError):
        receipt['state'] = 'failed'
    else:
        receipt['state'] = 'succeeded'
    runtime.atomic(path, receipt)
    return receipt


def cluster_node(runtime, binding, slots, packet):
    if not isinstance(packet, dict) or set(packet) != {'version', 'target', 'action', 'instance', 'args'}:
        raise ValueError('invalid cluster protocol request')
    if type(packet['version']) is not int or packet['version'] != 1 or packet['target'] != binding:
        raise ValueError('node identity or membership changed')
    action, args = packet['action'], packet['args']
    if not isinstance(action, str) or not isinstance(args, dict):
        raise ValueError('invalid request fields')
    if action == 'identify' and packet['instance'] is None and not args:
        for slot in slots.values():
            cluster_owned(runtime, slot)
        return {'target': binding, 'instances': [
            {'instance': i, 'source': s['cluster_source'], 'hostname': s['ingress']['app_hostname']}
            for i, s in sorted(slots.items())]}
    instance = require_hex(packet['instance'], ID)
    if instance not in slots:
        raise ValueError('instance not assigned to this node')
    slot = slots[instance]
    if action == 'status' and args.keys() <= {'operation', 'logs'} and type(args.get('logs', False)) is bool:
        return cluster_status(runtime, slot, args.get('operation'), args.get('logs', False))
    if action == 'stage' and args.keys() == {'package', 'fingerprint'}:
        return cluster_stage(runtime, slot, args['package'], args['fingerprint'])
    if action == 'apply':
        keys = {'action', 'operation', 'generation', 'fingerprint', 'expires', 'hostname'}
        if (args.get('action') == 'start' and args.keys() == keys | {'release', 'health'}
                or args.get('action') == 'stop' and args.keys() == keys):
            return cluster_mutate(runtime, slot, binding, args)
    raise ValueError('unsupported cluster operation')


def read_cluster_packet():
    deadline = time.monotonic() + 30
    data = bytearray()
    while len(data) <= MAX_PACKET:
        if not select.select([sys.stdin.fileno()], [], [], max(0, deadline - time.monotonic()))[0]:
            raise ValueError('node request timed out')
        chunk = os.read(sys.stdin.fileno(), min(65536, MAX_PACKET + 1 - len(data)))
        if not chunk:
            return json.loads(data)
        data.extend(chunk)
    raise ValueError('node request exceeds limit')


def cluster_recover(runtime, slots):
    failed = []
    for instance, slot in slots.items():
        try:
            record = runtime.desired(slot)
            cluster_owned(runtime, slot)
            runtime.dispatch(slot, ['reconcile'])
            after = runtime.desired(slot)
            if record and record['intent'] == 'running' and after['intent'] != 'running':
                failed.append(instance)
        except (ValueError, OSError, subprocess.SubprocessError, KeyError, TypeError):
            failed.append(instance)
    if failed:
        raise ValueError('node recovery needs inspection for instances: ' + ', '.join(failed))
    return {'ok': True}


def execute(runtime, config, args, node_mode, packet=None):
    if node_mode:
        if args not in (['cluster-recover'], ['cluster-node']):
            raise ValueError('node mode does not export controller releases')
        binding, slots = cluster_config(runtime, config)
        return cluster_recover(runtime, slots) if args == ['cluster-recover'] else cluster_node(runtime, binding, slots, packet)
    if len(args) != 3 or args[0] != 'export':
        raise ValueError('source mode requires export PROJECT RELEASE')
    project = require_hex(args[1], ID)
    release = require_hex(args[2], HEX)
    slots = config.get('apps')
    slot = next((entry for entry in slots if entry['project'] == project), None) if slots is not None else config
    if slot is None:
        raise ValueError('project has no source slot')
    package = export_release(runtime, slot, release)
    if package['receipt']['project'] != project:
        raise ValueError('release belongs to another project')
    return package


def main():
    os.umask(0o077)
    if os.geteuid() != 0:
        raise ValueError('installed root cluster adapter required')
    args = sys.argv[1:]
    node_mode = os.path.lexists(NODE_CONFIG)
    packet = read_cluster_packet() if node_mode and args == ['cluster-node'] else None
    runtime = local_broker()
    config = runtime.configuration()
    with (config['state'] / 'broker.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        result = execute(runtime, config, args, node_mode, packet)
    print(json.dumps(result))


if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, TypeError, KeyError, subprocess.SubprocessError) as error:
        print('Cluster broker refused request: ' + str(error), file=sys.stderr)
        sys.exit(1)
