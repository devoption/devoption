#!/usr/bin/python3 -I
"""Operator CLI for the fixed, instance-scoped managed-node protocol."""
import argparse
import base64
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import select
import stat
import subprocess
import sys
import tempfile
import time


BROKER = '/usr/local/libexec/devoption-cluster-broker'
MAX_PACKET = 6 * 1024 * 1024
MAX_CATALOG = 2 * 1024 * 1024
REVIEW_SECONDS = 300
SSH = '/usr/bin/ssh'


def identifier(value):
    if not isinstance(value, str) or not re.fullmatch(r'[0-9a-f]{32}', value):
        raise ValueError('expected a 32-character lowercase hexadecimal ID')
    return value


def checksum(value):
    if not isinstance(value, str) or not re.fullmatch(r'[0-9a-f]{64}', value):
        raise ValueError('expected a 64-character lowercase hexadecimal digest')
    return value


def new_id():
    return secrets.token_hex(16)


def label(value):
    if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9 ._-]{0,63}', value):
        raise ValueError('invalid display name')
    return value


def endpoint(host, port, user, public_key):
    if (not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9.-]{0,252}', host)
            or not 1 <= port <= 65535 or not re.fullmatch(r'[a-z_][a-z0-9_-]{0,31}', user)):
        raise ValueError('invalid SSH endpoint (use a DNS name or IPv4 address)')
    fields = public_key.split()
    if len(fields) != 2 or fields[0] != 'ssh-ed25519':
        raise ValueError('supply the verified node host key as ssh-ed25519 followed by its base64')
    if len(fields[1]) != 68:
        raise ValueError('invalid Ed25519 host key')
    decoded = base64.b64decode(fields[1], validate=True)
    if len(decoded) != 51 or not decoded.startswith(b'\0\0\0\x0bssh-ed25519\0\0\0\x20'):
        raise ValueError('invalid Ed25519 host key')
    return {'host': host.lower(), 'port': port, 'user': user, 'key': ' '.join(fields)}


def fsync_directory(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def private_file(fd):
    meta = os.fstat(fd)
    if (meta.st_uid != os.geteuid() or not stat.S_ISREG(meta.st_mode)
            or meta.st_mode & 0o7777 != 0o600 or meta.st_nlink != 1):
        raise ValueError('unsafe catalog or lock permissions')


class Store:
    def __init__(self, path):
        self.path = path.absolute()

    @contextmanager
    def locked(self):
        # Do not follow a symlink or write into another user's state directory.
        self.path.mkdir(mode=0o700, exist_ok=True)
        meta = self.path.lstat()
        if (self.path.resolve() != self.path or meta.st_uid != os.geteuid()
                or not stat.S_ISDIR(meta.st_mode) or meta.st_mode & 0o7777 != 0o700):
            raise ValueError('catalog directory must be canonical and private (mode 0700)')
        flags = os.O_NOFOLLOW | os.O_NONBLOCK
        fd = os.open(self.path / 'lock', os.O_CREAT | os.O_RDWR | flags, 0o600)
        with os.fdopen(fd, 'rb') as lock:
            private_file(lock.fileno())
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise ValueError('another controller operation is in progress') from None
            try:
                fd = os.open(self.path / 'catalog.json', os.O_RDONLY | flags)
            except FileNotFoundError:
                data = {'version': 1, 'clusters': {}, 'nodes': {}, 'instances': {}, 'operations': {}}
            else:
                with os.fdopen(fd, 'rb') as stream:
                    private_file(stream.fileno())
                    raw = stream.read(MAX_CATALOG + 1)
                if len(raw) > MAX_CATALOG:
                    raise ValueError('catalog exceeds size limit')
                data = json.loads(raw)
                if (not isinstance(data, dict) or data.keys() != {'version', 'clusters', 'nodes', 'instances', 'operations'}
                        or type(data['version']) is not int or data['version'] != 1
                        or not all(isinstance(data[key], dict) for key in data if key != 'version')):
                    raise ValueError('unsupported catalog format')
            yield data

    def save(self, data):
        payload = json.dumps(data, sort_keys=True, separators=(',', ':')).encode()
        if len(payload) > MAX_CATALOG:
            raise ValueError('catalog capacity reached')
        fd, temp = tempfile.mkstemp(prefix='.catalog-', dir=self.path)
        try:
            with os.fdopen(fd, 'wb') as stream:
                stream.write(payload)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temp, self.path / 'catalog.json')
            fsync_directory(self.path)
        finally:
            Path(temp).unlink(missing_ok=True)


def call(argv, payload=b'', timeout=120):
    if len(payload) > MAX_PACKET:
        raise ValueError('request exceeds transport limit')
    with tempfile.TemporaryFile() as stdin:
        stdin.write(payload)
        stdin.seek(0)
        env = {'LANG': 'C', 'PATH': '/usr/bin:/bin'}
        with subprocess.Popen(argv, stdin=stdin, stdout=subprocess.PIPE,
                              stderr=subprocess.DEVNULL, env=env) as child:
            try:
                cutoff = time.monotonic() + timeout
                output = bytearray()
                while True:
                    remaining = cutoff - time.monotonic()
                    if remaining <= 0 or not select.select([child.stdout], [], [], remaining)[0]:
                        raise TimeoutError
                    chunk = os.read(child.stdout.fileno(), 65536)
                    if not chunk:
                        break
                    output.extend(chunk)
                    if len(output) > MAX_PACKET:
                        raise ValueError('response exceeds transport limit')
                if child.wait(timeout=max(0, cutoff - time.monotonic())) != 0:
                    raise ValueError('command refused or connection unavailable')
                response = json.loads(output)
                if not isinstance(response, dict):
                    raise ValueError('invalid response')
                return response
            except (TimeoutError, subprocess.TimeoutExpired):
                raise ValueError('command timed out; inspect status before retry') from None
            finally:
                if child.poll() is None:
                    child.kill()


class Transport:
    def __init__(self, identity):
        self.identity = identity

    def request(self, node, command, instance=None, **args):
        if self.identity is None:
            raise ValueError('network operations require --identity with an existing node control key')
        key_path = self.identity.absolute()
        key = key_path.lstat()
        if not stat.S_ISREG(key.st_mode) or key.st_uid != os.geteuid() or key.st_mode & 0o077:
            raise ValueError('SSH identity must be a private regular file owned by the controller operator')
        peer = node['ssh']
        alias = 'devoption-node-' + node['target']['node']
        packet = {'version': 1, 'target': node['target'], 'action': command, 'instance': instance, 'args': args}
        with tempfile.TemporaryDirectory(prefix='devoption-known-') as directory:
            known = Path(directory) / 'hosts'
            known.write_text(f"{alias} {peer['key']}\n")
            options = [
                'BatchMode=yes', 'IdentitiesOnly=yes', 'IdentityAgent=none', 'StrictHostKeyChecking=yes',
                'GlobalKnownHostsFile=/dev/null', 'UpdateHostKeys=no', 'HostKeyAlgorithms=ssh-ed25519',
                'PasswordAuthentication=no', 'KbdInteractiveAuthentication=no', 'ControlMaster=no',
                'ClearAllForwardings=yes', 'RequestTTY=no', 'ConnectTimeout=10', 'ConnectionAttempts=1',
                'ServerAliveInterval=10', 'ServerAliveCountMax=3', 'NumberOfPasswordPrompts=0',
                f'HostKeyAlias={alias}', f'UserKnownHostsFile={known}',
            ]
            argv = [SSH, '-F', '/dev/null', '-T', '-i', str(key_path), '-l', peer['user'], '-p', str(peer['port'])]
            for option in options:
                argv.extend(['-o', option])
            return call(argv + ['--', peer['host'], 'devoption-cluster-v1'],
                        json.dumps(packet, separators=(',', ':')).encode())


class Controller:
    def __init__(self, store, data, transport):
        self.store, self.data, self.transport = store, data, transport

    def save(self):
        self.store.save(self.data)

    def get(self, collection, key):
        if identifier(key) not in self.data[collection]:
            raise ValueError('unknown ' + collection.rstrip('s'))
        return self.data[collection][key]

    def create_cluster(self, name):
        clusters = self.data['clusters']
        name = label(name)
        if len(clusters) >= 256 or any(v['name'].casefold() == name.casefold() for v in clusters.values()):
            raise ValueError('duplicate cluster name or cluster capacity reached')
        key = new_id()
        clusters[key] = {'name': name}
        self.save()
        return {'cluster': key, **clusters[key]}

    def attach_node(self, cluster, node, epoch, host, port, user, public_key):
        self.get('clusters', cluster)
        identifier(node)
        identifier(epoch)
        nodes = self.data['nodes']
        peer = endpoint(host, port, user, public_key)
        if node in nodes or len(nodes) >= 1024 or any(
                e['ssh']['key'] == peer['key'] or (e['ssh']['host'], e['ssh']['port']) == (peer['host'], peer['port'])
                for e in nodes.values()):
            raise ValueError('node identity, address or host key is already recorded, or node capacity reached')
        candidate = {'target': {'cluster': cluster, 'node': node, 'epoch': epoch}, 'ssh': peer, 'enabled': True}
        found = self.transport.request(candidate, 'identify')
        if found.get('target') != candidate['target'] or not isinstance(found.get('instances'), list):
            raise ValueError('node did not confirm exact membership')
        nodes[node] = candidate
        self.save()
        return found

    def disable_node(self, node):
        entry = self.get('nodes', node)
        entry['enabled'] = False
        for record in self.data['operations'].values():
            if record['node'] == node and record['state'] == 'review':
                record['state'] = 'revoked'
        self.save()
        return {'node': node, 'enabled': False, 'remote_access_removed': False}

    def node(self, node):
        entry = self.get('nodes', node)
        if not entry['enabled']:
            raise ValueError('node is disabled in this controller')
        return entry

    def attach_instance(self, cluster, node_id, instance, project, name):
        self.get('clusters', cluster)
        node = self.node(node_id)
        identifier(instance)
        identifier(project)
        name = label(name)
        if node['target']['cluster'] != cluster:
            raise ValueError('node belongs to another cluster')
        instances = self.data['instances']
        if instance in instances or len(instances) >= 2048 or any(
                i['cluster'] == cluster and i['name'].casefold() == name.casefold() for i in instances.values()):
            raise ValueError('duplicate instance identity or name, or instance capacity reached')
        found = self.transport.request(node, 'status', instance)
        hostname = found.get('hostname')
        if found.get('source') != project or not isinstance(hostname, str) or not hostname:
            raise ValueError('node slot does not match the source project or has no hostname')
        if any(i['hostname'] == hostname for i in instances.values()):
            raise ValueError('hostname is already bound to another instance')
        entry = {'cluster': cluster, 'node': node_id, 'source': project, 'hostname': hostname, 'name': name}
        instances[instance] = entry
        self.save()
        return {'instance': instance, **entry}

    def placement(self, instance):
        entry = self.get('instances', instance)
        node = self.node(entry['node'])
        if node['target']['cluster'] != entry['cluster']:
            raise ValueError('node membership does not match the instance')
        return entry, node

    def observe(self, instance, **kwargs):
        entry, node = self.placement(instance)
        response = self.transport.request(node, 'status', instance, **kwargs)
        if response.get('source') != entry['source'] or response.get('hostname') != entry['hostname']:
            raise ValueError('node slot does not match the recorded instance')
        return response

    def stage(self, instance, source_release):
        entry, node = self.placement(instance)
        found = self.observe(instance)
        package = call(['/usr/bin/sudo', '-n', BROKER, 'export', entry['source'],
                        checksum(source_release)])
        if package.get('release') != source_release or package.get('receipt', {}).get('project') != entry['source']:
            raise ValueError('source broker returned a different release')
        return self.transport.request(node, 'stage', instance, package=package,
                                      fingerprint=checksum(found['fingerprint']))

    def inspect_node(self, node):
        node = self.node(node)
        try:
            status = self.transport.request(node, 'identify')
            return {'observed_at': int(time.time()), 'status': status}
        except (ValueError, OSError):
            return {'node': node['target']['node'], 'observation': 'unavailable'}

    def inspect(self, instance, logs):
        self.get('instances', instance)
        try:
            status = self.observe(instance, logs=logs)
            return {'observed_at': int(time.time()), 'status': status}
        except (ValueError, OSError):
            return {'instance': instance, 'observation': 'unavailable'}

    def review(self, instance, action, release=None, health=None):
        entry, node = self.placement(instance)
        observed = self.observe(instance)
        operation = new_id()
        args = {'action': action, 'operation': operation, 'generation': checksum(observed['generation']),
                'fingerprint': checksum(observed['fingerprint']), 'expires': int(time.time()) + REVIEW_SECONDS,
                'hostname': entry['hostname']}
        candidate = None
        if action == 'start':
            candidate = next((v for v in observed['releases'] if v['release'] == checksum(release)), None)
            if not candidate or not isinstance(health, str) or not re.fullmatch(r'[\x20-\x7e]{1,128}', health):
                raise ValueError('requires a staged release and printable ASCII health body')
            grant = observed['record_grant']
            if candidate['record_grant_sha256'] != (grant['sha256'] if grant else None):
                raise ValueError('record grant changed; stage the release again')
            args.update(release=release, health=health)
        records = self.data['operations']
        # Expired reviews have never been dispatched and cannot become active again.
        for key in list(records):
            prior = records[key]
            if prior['state'] == 'review' and prior['args']['expires'] <= int(time.time()):
                del records[key]
        installed = observed['deployment']
        current = {k: installed[k] for k in ('release', 'intent', 'observed')} if installed else None
        record = {'state': 'review', 'instance': instance, 'node': entry['node'], 'target': node['target'],
                  'source': entry['source'], 'args': args, 'candidate': candidate, 'current': current,
                  'record_grant': observed['record_grant']}
        records[operation] = record
        self.save()
        return record

    def approve(self, operation):
        record = self.get('operations', operation)
        instance = record['instance']
        _, node = self.placement(instance)
        if (record['state'] != 'review' or record['args']['expires'] <= int(time.time())
                or record['target'] != node['target']):
            raise ValueError('review has expired, was consumed, or its target changed')
        # Persist consumption before contacting the node. No reconnect or restart
        # can cause this command to run again without a fresh human review.
        record['state'] = 'unknown'
        self.save()
        try:
            reply = self.transport.request(node, 'apply', instance, **record['args'])
            self.accept_receipt(record, reply)
        except (ValueError, OSError):
            pass
        return record

    def accept_receipt(self, record, reply):
        binding = json.dumps([record['target'], record['instance'], record['args']],
                             sort_keys=True, separators=(',', ':')).encode()
        if (reply.get('operation') != record['args']['operation']
                or reply.get('request') != hashlib.sha256(binding).hexdigest()
                or reply.get('state') not in ('succeeded', 'failed', 'unknown')):
            raise ValueError('invalid operation receipt')
        record['state'] = reply['state']
        self.save()

    def reconcile(self, operation):
        record = self.get('operations', operation)
        if record['state'] == 'unknown':
            try:
                observed = self.observe(record['instance'], operation=operation)
                if observed.get('receipt'):
                    self.accept_receipt(record, observed['receipt'])
            except (OSError, ValueError):
                pass
        return record

    def operations(self, instance):
        self.get('instances', instance)
        now = int(time.time())
        return {'instance': instance, 'operations': {
            key: {'action': record['args']['action'], 'expires': record['args']['expires'],
                  'state': ('expired' if record['state'] == 'review' and record['args']['expires'] <= now
                            else record['state'])}
            for key, record in self.data['operations'].items() if record['instance'] == instance}}

    def inventory(self):
        return {key: {
            ident: ({'cluster': entry['target']['cluster'], 'enabled': entry['enabled'],
                     'endpoint': entry['ssh']['host']} if key == 'nodes' else entry)
            for ident, entry in self.data[key].items()} for key in ('clusters', 'nodes', 'instances')}


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument('--state', type=Path, required=True, help='private persistent controller directory')
    result.add_argument('--identity', type=Path, help='existing SSH private key for prepared nodes')
    commands = result.add_subparsers(dest='command', required=True)
    definitions = {
        'create-cluster': ('name',), 'attach-node': ('cluster', 'node', 'epoch', 'host', 'port', 'user', 'public_key'),
        'disable-node': ('node',), 'attach-instance': ('cluster', 'node_id', 'instance', 'project', 'name'),
        'inventory': (), 'operations': ('instance',), 'node-status': ('node',), 'status': ('instance',),
        'stage': ('instance', 'source_release'), 'review-start': ('instance', 'release', 'health'),
        'review-stop': ('instance',), 'approve': ('operation',), 'reconcile': ('operation',),
    }
    for name, fields in definitions.items():
        sub = commands.add_parser(name)
        for field in fields:
            sub.add_argument(field, type=int if field == 'port' else str)
        if name == 'status':
            sub.add_argument('--logs', action='store_true')
    return result


def main():
    os.umask(0o077)
    received = vars(parser().parse_args())
    store = Store(received.pop('state'))
    transport = Transport(received.pop('identity'))
    command = received.pop('command')
    methods = {'status': 'inspect', 'node-status': 'inspect_node', 'review-start': 'review', 'review-stop': 'review'}
    if command in ('review-start', 'review-stop'):
        received['action'] = command.removeprefix('review-')
    with store.locked() as data:
        controller = Controller(store, data, transport)
        reply = getattr(controller, methods.get(command, command.replace('-', '_')))(**received)
    print(json.dumps(reply, indent=2, sort_keys=True))
    if reply.get('observation') == 'unavailable' or (command in ('approve', 'reconcile')
                                                     and reply['state'] != 'succeeded'):
        return 2
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (ValueError, OSError, KeyError, TypeError) as error:
        print(f'Cluster operation refused: {error}', file=sys.stderr)
        raise SystemExit(1) from None
