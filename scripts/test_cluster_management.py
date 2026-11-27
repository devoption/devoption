"""Controller plus independently configured node protocol fixtures."""
import base64
import json
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch

import cluster_broker as node
import cluster_control as lab
import test_deployment_broker as fixtures

runtime = fixtures.broker


PROJECT = 'c' * 32
HOST_KEY_PREFIX = b'\0\0\0\x0bssh-ed25519\0\0\0\x20'


def host_key(n):
    return 'ssh-ed25519 ' + base64.b64encode(HOST_KEY_PREFIX + bytes([n]) * 32).decode()


class Protocol:
    def __init__(self, peers):
        self.peers = peers
        self.receipt_loss = False
        self.disconnected = set()

    def request(self, entry, command, instance=None, **args):
        target = entry['target']
        if target['node'] in self.disconnected:
            raise OSError('fixture offline')
        actual, slots = self.peers[entry['ssh']['host']]
        result = node.cluster_node(runtime, actual, slots, {
            'version': 1, 'target': target, 'instance': instance, 'action': command, 'args': args,
        })
        if self.receipt_loss and command == 'apply':
            raise OSError('fixture receipt lost')
        return result


class ClusterTests(unittest.TestCase):
    setUp = fixtures.DeploymentTests.setUp
    write_receipt = fixtures.DeploymentTests.write_receipt
    promote = fixtures.DeploymentTests.promote

    def init(self):
        root = self.config['state'].parent
        self.catalog = lab.Store(root / 'controller')
        self.peers = {}
        self.transport = Protocol(self.peers)
        self.fingerprint = 'f' * 64
        self.config['project'] = PROJECT
        self.original = self.promote()
        self.package = node.execute(runtime, self.config, ['export', PROJECT, self.original], False)
        initial_state = {'ActiveState': 'inactive', 'ControlGroup': ''}
        units = {}

        def start(slot, release, health, fingerprint, generation=None, delivery=None):
            runtime.load_release(slot, release)
            invocation = lab.new_id()
            units[slot['unit']] = {'ActiveState': 'active', 'ControlGroup': '/fixture', 'InvocationID': invocation}
            runtime.save(slot, {'release': release, 'project': slot['project'], 'health': health,
                                'fingerprint': fingerprint, 'intent': 'running', 'observed': 'healthy',
                                'invocation': invocation})

        def stop(unit):
            units.pop(unit, None)

        for method, implementation in (
            ('start', start), ('stop_unit', stop), ('unit_state', lambda u: units.get(u, initial_state)),
            ('health', lambda *_: True), ('ingress_status', lambda *_: None),
        ):
            patched = patch.object(runtime, method, side_effect=implementation)
            patched.start()
            self.addCleanup(patched.stop)

    def peer(self, controller, cluster, index, ids):
        binding = {'cluster': cluster, 'node': str(index) * 32, 'epoch': str(index + 2) * 32}
        host = f'node{index}.example.test'
        slots = {}
        for instance in ids:
            root = self.config['state'].parent / instance
            root.mkdir(mode=0o700)
            slots[instance] = {'state': root, 'project': instance, 'cluster_source': PROJECT,
                               'fingerprint': self.fingerprint, 'unit': 'worker-' + instance,
                               'validate_unit': 'validate-' + instance, 'port': 15000,
                               'ingress': {'app_hostname': f'app-{instance}.example.test'}}
        self.peers[host] = binding, slots
        controller.attach_node(cluster, binding['node'], binding['epoch'], host, 22, 'operator', host_key(index))
        return binding, slots

    def test_two_clusters_shared_release_and_lost_receipt_isolation(self):
        self.init()
        preview, peer_instance = 'b'*32, 'd'*32
        with self.catalog.locked() as saved:
            c = lab.Controller(self.catalog, saved, self.transport)
            left = c.create_cluster('West')['cluster']
            right = c.create_cluster('East')['cluster']
            _, west = self.peer(c, left, 1, [preview, peer_instance])
            east_binding, _ = self.peer(c, right, 2, ['a'*32])
            c.attach_instance(left, '1'*32, preview, PROJECT, 'Preview')
            c.attach_instance(left, '1'*32, peer_instance, PROJECT, 'Instance')
            c.attach_instance(right, '2'*32, 'a'*32, PROJECT, 'West copy')
            with self.assertRaisesRegex(ValueError, 'another cluster'):
                c.attach_instance(left, '2'*32, 'a'*32, PROJECT, 'Wrong cluster')
            package = self.package.copy()
            with patch.object(lab, 'call', return_value=package) as source:
                releases = {i: c.stage(i, self.original)['release'] for i in (preview, peer_instance, 'a'*32)}
                self.assertEqual(source.call_count, 3)
            self.assertEqual(len(set(releases.values())), 3)
            for i in (preview, 'a'*32):
                review = c.review(i, 'start', releases[i], 'ok')
                self.assertEqual(c.approve(review['args']['operation'])['state'], 'succeeded')
            review = c.review(peer_instance, 'start', releases[peer_instance], 'ok')
            operation = review['args']['operation']
            self.transport.receipt_loss = True
            self.assertEqual(c.approve(operation)['state'], 'unknown')
        with self.catalog.locked() as saved:
            c = lab.Controller(self.catalog, saved, self.transport)
            self.assertEqual(c.reconcile(operation)['state'], 'succeeded')
            with self.assertRaisesRegex(ValueError, 'consumed'):
                c.approve(operation)
            self.transport.receipt_loss = False
            stop = c.review(preview, 'stop')['args']['operation']
            self.assertEqual(c.approve(stop)['state'], 'succeeded')
            self.assertEqual(c.observe(peer_instance)['deployment']['observed'], 'healthy')
            self.assertEqual(c.observe('a'*32)['deployment']['observed'], 'healthy')
            node.cluster_recover(runtime, west)
            self.assertEqual(c.observe(preview)['deployment']['intent'], 'stopped')
            self.assertEqual(c.observe(peer_instance)['deployment']['intent'], 'running')
            self.transport.disconnected.add(east_binding['node'])
            self.assertEqual(c.inspect('a'*32, False)['observation'], 'unavailable')
            self.assertEqual(c.inspect_node(east_binding['node'])['observation'], 'unavailable')

    def test_rejects_tamper_wrong_membership_occupied_slot_and_stale_review(self):
        self.init()
        instance = 'b'*32
        with self.catalog.locked() as saved:
            c = lab.Controller(self.catalog, saved, self.transport)
            left = c.create_cluster('West')['cluster']
            binding, slots = self.peer(c, left, 1, [instance])
            slot = slots[instance]
            c.attach_instance(left, '1'*32, instance, PROJECT, 'Demo')
            with self.assertRaisesRegex(ValueError, 'membership'):
                node.cluster_node(runtime, binding, slots, {'version': 1, 'target': dict(binding, epoch='e'*32),
                                  'action': 'status', 'instance': instance, 'args': {}})
            tampered = dict(self.package, component=base64.b64encode(self.component+b'x').decode())
            with self.assertRaisesRegex(ValueError, 'checksum'):
                node.cluster_stage(runtime, slot, tampered, self.fingerprint)
            with self.assertRaisesRegex(ValueError, 'another source'):
                node.cluster_stage(runtime, slot, dict(self.package, receipt=dict(self.receipt, project='e'*32)), self.fingerprint)
            self.assertEqual(list(slot['state'].iterdir()), [])
            with patch.object(runtime, 'unit_state', return_value={'ActiveState': 'active', 'ControlGroup': '/foreign'}):
                with self.assertRaisesRegex(ValueError, 'unrecorded'):
                    node.cluster_stage(runtime, slot, self.package, self.fingerprint)
            saved_release = node.cluster_stage(runtime, slot, self.package, self.fingerprint)['release']
            self.assertEqual(node.cluster_stage(runtime, slot, self.package, self.fingerprint)['release'], saved_release)
            review = c.review(instance, 'start', saved_release, 'ok')
            args = review['args']
            with self.assertRaisesRegex(ValueError, 'expired'):
                node.cluster_mutate(runtime, slot, binding, dict(args, expires=int(time.time()) - 1))
            with self.assertRaisesRegex(ValueError, 'changed'):
                node.cluster_mutate(runtime, slot, binding, dict(args, fingerprint='0'*64))
            slot['fingerprint'] = '1'*64
            self.assertEqual(c.approve(args['operation'])['state'], 'unknown')
            self.assertIsNone(runtime.desired(slot))
            fresh = c.review(instance, 'start', saved_release, 'ok')['args']
            receipt = node.cluster_mutate(runtime, slot, binding, fresh)
            self.assertEqual(receipt['state'], 'succeeded')
            self.assertEqual(node.cluster_mutate(runtime, slot, binding, fresh), receipt)
            with self.assertRaisesRegex(ValueError, 'different inputs'):
                node.cluster_mutate(runtime, slot, binding, dict(fresh, health='different'))
            pending = c.review(instance, 'stop')['args']['operation']
            c.disable_node(binding['node'])
            self.assertEqual(saved['operations'][pending]['state'], 'revoked')
            with self.assertRaisesRegex(ValueError, 'disabled'):
                c.approve(pending)

    def test_records_bind_to_destination_and_require_fresh_staging_after_change(self):
        self.init()
        instance = 'b'*32
        with self.catalog.locked() as saved:
            c = lab.Controller(self.catalog, saved, self.transport)
            cluster = c.create_cluster('Records')['cluster']
            binding, slots = self.peer(c, cluster, 1, [instance])
            slot = slots[instance]
            c.attach_instance(cluster, binding['node'], instance, PROJECT, 'Demo')
            original = node.cluster_stage(runtime, slot, self.package, self.fingerprint)['release']
            stale = c.review(instance, 'start', original, 'ok')['args']
            grant = {'app_id': instance, 'allow_read': True}
            slot['records'] = {'grant': grant, 'bytes': runtime.canonical(grant),
                               'identity': {'user': 'fixture_db', 'uid': 1001, 'gid': 1001}}
            with self.assertRaisesRegex(ValueError, 'grant changed'):
                c.review(instance, 'start', original, 'ok')
            with self.assertRaisesRegex(ValueError, 'grant changed'):
                node.cluster_mutate(runtime, slot, binding, stale)
            restaged = node.cluster_stage(runtime, slot, self.package, self.fingerprint)['release']
            self.assertNotEqual(original, restaged)
            manifest = runtime.load_release(slot, restaged)
            self.assertEqual(manifest['project'], instance)
            self.assertEqual(manifest['cluster_origin']['project'], PROJECT)
            self.assertEqual(json.loads((slot['state'] / restaged / 'records.json').read_bytes()), grant)
            review = c.review(instance, 'start', restaged, 'ok')
            self.assertTrue(review['record_grant']['read'])
            self.assertEqual(c.approve(review['args']['operation'])['state'], 'succeeded')

    def test_export_checks_project_and_controller_state_refuses_aliases(self):
        self.init()
        with self.assertRaisesRegex(ValueError, 'another project'):
            node.execute(runtime, self.config, ['export', 'e'*32, self.original], False)
        with self.catalog.locked() as _:
            with self.assertRaisesRegex(ValueError, 'in progress'):
                with self.catalog.locked():
                    pass
        parent_alias = self.catalog.path.with_name('parent-alias')
        parent_alias.symlink_to(self.catalog.path.parent)
        self.assertEqual(lab.Store(parent_alias / self.catalog.path.name).path, self.catalog.path)
        link = self.catalog.path.with_name('alias')
        link.symlink_to(self.catalog.path)
        with self.assertRaisesRegex(ValueError, 'canonical'):
            with lab.Store(link).locked():
                pass

    def test_node_config_requires_an_exact_source_mapping_for_every_slot(self):
        selected, other = '1'*32, '2'*32
        root = self.config['state'].parent
        source = root / 'node.toml'
        adapter = root / 'adapter'
        adapter.write_bytes(b'fixture adapter')
        header = 'cluster="' + 'a'*32 + '"\nnode="' + 'b'*32 + '"\nepoch="' + 'c'*32 + '"\n[instances]\n'
        mapping = lambda instance: f'"{instance}" = "{PROJECT}"\n'
        source.write_text(header + mapping(selected))
        slots = [{'project': selected, 'fingerprint': 'f'*64}, {'project': other, 'fingerprint': 'f'*64}]
        with patch.object(node, 'NODE_CONFIG', source), patch.object(node, 'ADAPTER', adapter), patch.object(node, 'protected'):
            with self.assertRaisesRegex(ValueError, 'every node slot'):
                node.cluster_config(runtime, {'apps': slots})
            source.write_text(header + mapping(selected) + mapping(other))
            target, observed = node.cluster_config(runtime, {'apps': slots})
        self.assertEqual(target, {'cluster': 'a'*32, 'node': 'b'*32, 'epoch': 'c'*32})
        self.assertEqual(observed[selected]['cluster_source'], PROJECT)
        self.assertNotEqual(observed[selected]['fingerprint'], observed[other]['fingerprint'])

    def test_transport_pins_key_and_keeps_protocol_action_distinct(self):
        key = self.config['state'].parent / 'identity'
        key.write_text('fixture key placeholder')
        key.chmod(0o600)
        target = {'cluster': 'a'*32, 'node': 'b'*32, 'epoch': 'c'*32}

        def check(argv, payload):
            self.assertEqual(argv[:4], [lab.SSH, '-F', '/dev/null', '-T'])
            self.assertIn('StrictHostKeyChecking=yes', argv)
            self.assertIn('IdentityAgent=none', argv)
            self.assertIn('ClearAllForwardings=yes', argv)
            known = next(a.partition('=')[2] for a in argv if a.startswith('UserKnownHostsFile='))
            self.assertEqual(Path(known).read_text(), 'devoption-node-' + target['node'] + ' ' + host_key(1) + '\n')
            self.assertEqual(argv[-2:], ['node.example.test', 'devoption-cluster-v1'])
            self.assertEqual(json.loads(payload), {'version': 1, 'target': target, 'action': 'apply',
                             'instance': 'd'*32, 'args': {'action': 'stop'}})
            return {}

        with patch.object(lab, 'call', side_effect=check):
            lab.Transport(key).request({'target': target, 'ssh': lab.endpoint('node.example.test', 22,
                'operator', host_key(1))}, 'apply', 'd'*32, action='stop')


if __name__ == '__main__':
    unittest.main()
