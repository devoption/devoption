"""OpenSSH and real worker acceptance, called only by check_multi_app's guarded fixture."""
import getpass
import http.client
import json
from pathlib import Path
import shutil
import socket
import subprocess
import time

import cluster_broker
from cluster_control import Controller, Store, Transport, new_id


SSH_UNIT = 'devoption-cluster-proof-ssh.service'
SSH_PORT = 15339
APP_PORT = 15340
NODE_CONFIG = cluster_broker.NODE_CONFIG
EXPORT_CONFIG = cluster_broker.EXPORT_CONFIG
ADAPTER = cluster_broker.ADAPTER


def config_text(config):
    raw = ''.join(f'{k}={json.dumps(v)}\n' for k, v in config.items() if k != 'apps')
    for app in config['apps']:
        raw += f'\n[[apps]]\nproject="{app["project"]}"\nport={app["port"]}\n[apps.ingress]\n'
        raw += ''.join(f'{k}={json.dumps(v)}\n' for k, v in app['ingress'].items())
    return raw


def probe(port, expected):
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=5)
    try:
        connection.request('GET', '/healthz')
        response = connection.getresponse()
        assert response.status == 200 and response.read(256) == expected.encode()
    finally:
        connection.close()


def check(run, root, state, config_path, config, project, releases, adapter):
    sshd = shutil.which('sshd') or '/usr/sbin/sshd'
    if not Path(sshd).is_file():
        raise ValueError('cluster acceptance requires OpenSSH server')
    if any(p.exists() or p.is_symlink() for p in (ADAPTER, NODE_CONFIG, EXPORT_CONFIG)):
        raise ValueError('existing cluster adapter or node configuration')
    if run('systemctl', 'show', SSH_UNIT, '-p', 'LoadState', '--value').strip() != b'not-found':
        raise ValueError('reserved cluster SSH fixture unit exists')
    for port in (SSH_PORT, APP_PORT, APP_PORT + 1):
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', port))
    inputs = root / 'cluster-input'
    inputs.mkdir(mode=0o700)
    ssh_root = state / 'cluster-ssh'
    node_state = state / 'cluster-apps'
    slots = [new_id(), new_id()]
    username = getpass.getuser()
    for instance in slots:
        for suffix in ('', '-validate'):
            unit = 'devoption-app-' + instance + suffix + '.service'
            assert run('systemctl', 'show', unit, '-p', 'LoadState', '--value').strip() == b'not-found'
    staged, host = inputs / 'operator-key', inputs / 'host-key'
    source = inputs / 'original-deployment.toml'
    source.write_text(config_text(config))
    store = Store(root / 'cluster-controller')
    controller_key = Transport(staged)
    installed = False
    activated = False
    export_configured = False
    ssh_started = False
    created_sshd_directory = False

    def install(content, target, mode='644'):
        scratch = inputs / 'install-input'
        scratch.write_text(content)
        run('sudo', '-n', 'install', '-m' + mode, scratch, target)

    try:
        run('sudo', '-n', 'install', '-m755', adapter, ADAPTER)
        installed = True
        # Model two distinct callers on the disposable host. Only the fixture's
        # existing root runner may set the synthetic caller for this check.
        operator_uid = config['uid'] + 1
        install(f'operator_uid = {operator_uid}\n', EXPORT_CONFIG, '600')
        export_configured = True
        refused = subprocess.run(['sudo', '-n', str(ADAPTER), 'export', project, releases[0]],
                                 capture_output=True, timeout=15)
        assert refused.returncode and b'not the configured cluster export operator' in refused.stderr
        packages = [json.loads(run('sudo', '-n', 'env', f'SUDO_UID={operator_uid}',
                                   ADAPTER, 'export', project, release)) for release in releases]
        assert [package['release'] for package in packages] == list(releases)
        for private in (staged, host):
            run('/usr/bin/ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', private)
        public = lambda path: ' '.join(path.with_suffix('.pub').read_text().split()[:2])
        run('sudo', '-n', 'install', '-d', '-m755', ssh_root)
        run('sudo', '-n', 'install', '-m600', host, ssh_root / 'host-key')
        install(f'restrict,command="/usr/bin/sudo -n {ADAPTER} cluster-node" {public(staged)}\n',
                ssh_root / 'authorized_keys')
        install(f'''Port {SSH_PORT}
ListenAddress 127.0.0.1
HostKey {ssh_root}/host-key
PidFile {ssh_root}/sshd.pid
AuthorizedKeysFile {ssh_root}/authorized_keys
AllowUsers {username}
AuthenticationMethods publickey
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM yes
PermitRootLogin no
DisableForwarding yes
PermitTTY no
StrictModes yes
''', ssh_root / 'sshd_config')
        if not Path('/run/sshd').exists():
            run('sudo', '-n', 'install', '-d', '-m755', '/run/sshd')
            created_sshd_directory = True
        run('sudo', '-n', sshd, '-t', '-f', ssh_root / 'sshd_config')
        run('sudo', '-n', 'install', '-d', '-m700', node_state, *(node_state / s for s in slots))
        target = {'node': new_id(), 'epoch': new_id()}
        with store.locked() as catalog:
            c = Controller(store, catalog, controller_key)
            target['cluster'] = c.create_cluster('Disposable cluster')['cluster']
            membership = ''.join(f'{k}="{value}"\n' for k, value in target.items())
            membership += '[instances]\n' + ''.join(f'"{instance}"="{project}"\n' for instance in slots)
            node_apps = [{'project': instance, 'port': APP_PORT + i,
                          'ingress': {'project': instance, 'platform_hostname': 'platform.example.test',
                                      'platform_port': 15336, 'app_hostname': f'node-app{i}.example.test'}}
                         for i, instance in enumerate(slots)]
            activated = True
            install(config_text(dict(config, state=str(node_state), apps=node_apps)), config_path)
            install(membership, NODE_CONFIG, '600')
            run('sudo', '-n', 'systemd-run', '--quiet', '--collect', '--unit=' + SSH_UNIT,
                sshd, '-D', '-e', '-f', ssh_root / 'sshd_config')
            ssh_started = True
            deadline = time.monotonic() + 10
            while True:
                try:
                    with socket.create_connection(('127.0.0.1', SSH_PORT), timeout=1):
                        break
                except OSError:
                    if time.monotonic() >= deadline:
                        raise TimeoutError('disposable SSH endpoint readiness')
                    time.sleep(.1)
            try:
                c.attach_node(target['cluster'], target['node'], target['epoch'], '127.0.0.1',
                              SSH_PORT, username, public(staged))
            except ValueError:
                pass
            else:
                raise AssertionError('controller connected with wrong host key pin')
            c.attach_node(target['cluster'], target['node'], target['epoch'], '127.0.0.1',
                          SSH_PORT, username, public(host))

            def stage(instance, package):
                return controller_key.request(c.node(target['node']), 'stage', instance,
                    fingerprint=c.observe(instance)['fingerprint'], package=package)['release']

            def approve(instance, action, release=None, health=None):
                review = c.review(instance, action, release, health)
                result = c.approve(review['args']['operation'])
                assert result['state'] == 'succeeded', result

            first_releases = []
            for i, instance in enumerate(slots):
                c.attach_instance(target['cluster'], target['node'], instance, project, f'Instance {i}')
                first_releases.append(stage(instance, packages[0]))
                approve(instance, 'start', first_releases[-1], 'ok 0')
                probe(APP_PORT + i, 'ok 0')
            assert first_releases[0] != first_releases[1]
            updated = stage(slots[0], packages[1])
            sibling = c.observe(slots[1])['deployment']
            approve(slots[0], 'start', updated, 'ok updated')
            probe(APP_PORT, 'ok updated')
            assert c.observe(slots[1])['deployment'] == sibling
            approve(slots[0], 'start', first_releases[0], 'ok 0')
            probe(APP_PORT, 'ok 0')
            approve(slots[0], 'stop')
            run('sudo', '-n', ADAPTER, 'cluster-recover')
            assert c.observe(slots[0])['deployment']['intent'] == 'stopped'
            probe(APP_PORT+1, 'ok 0')
            assert c.observe(slots[1])['deployment']['observed'] == 'healthy'
        print('Pinned SSH host, forced node command, two same-release systemd instances, isolated update, rollback and approved intent recovery passed.')
    finally:
        if ssh_started:
            run('sudo', '-n', 'systemctl', 'stop', SSH_UNIT)
        for instance in slots:
            for suffix in ('', '-validate'):
                subprocess.run(['sudo', '-n', 'systemctl', 'stop', 'devoption-app-' + instance + suffix + '.service'],
                               capture_output=True, timeout=15)
        if activated:
            run('sudo', '-n', 'rm', '-f', '--', NODE_CONFIG)
            run('sudo', '-n', 'install', '-m644', source, config_path)
        if export_configured:
            run('sudo', '-n', 'rm', '--', EXPORT_CONFIG)
        if installed:
            run('sudo', '-n', 'rm', '--', ADAPTER)
        if created_sshd_directory:
            run('sudo', '-n', 'rmdir', '/run/sshd')
