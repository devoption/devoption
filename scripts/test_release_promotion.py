"""Readiness boundaries, incomplete/failing evidence and live canary stop controls."""

import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from bundle_candidate import identity
from release_bundle import canonical
import release_canary as collector
import release_promotion as gate


class PromotionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.policy = self.root / 'policy.toml'
        self.policy.write_bytes(gate.POLICY.read_bytes().replace(b'stopped = true', b'stopped = false'))
        self.policy_sha = gate.read_policy(self.policy)[1]
        self.record = {'schema_version': 1,
                       'candidate': identity('a' * 40, '0.1.0-preview.3', 'b' * 64, 'c' * 64, '12', '1'),
                       'artifact': {'id': '34', 'transport_sha256': 'd' * 64}}
        self.now = 1792500000
        self.app = 'e' * 64
        self.reports = {case: gate.sha(canonical({'fixture': case})) for case in gate.CHECKS}
        self.suite = {'schema_version': 1, 'reservation': self.record,
                      'completed_at': gate.timestamp(self.now - 3600),
                      'checks': [{'case': case, 'passed': True, 'report_sha256': digest}
                                 for case, digest in self.reports.items()]}
        self.observation = self.canary()

    def probes(self, latency=10):
        return {name: {'passed': True, 'latency_ms': latency} for name in gate.PROBES}

    def canary(self, seconds=3600):
        return {'schema_version': 1, 'reservation': copy.deepcopy(self.record), 'complete': True,
                'policy_sha256': self.policy_sha, 'workload': 'trusted-wasip2-example-A-v1',
                'app_sha256': self.app, 'started_at': gate.timestamp(self.now - seconds),
                'completed_at': gate.timestamp(self.now),
                'samples': [{'at': gate.timestamp(at), 'probes': self.probes()}
                            for at in range(self.now - seconds, self.now + 1, 60)]}

    def evaluate(self, **options):
        return gate.evaluate(self.record, options.get('channel', 'preview'), self.suite,
                             self.observation, self.app, self.now, self.reports, self.policy)

    def test_preview_readiness_preserves_every_identity_and_never_publication(self):
        before = canonical(self.record)
        result = self.evaluate()
        self.assertTrue(result['readiness_passed'])
        self.assertFalse(result['publication_eligible'])
        self.assertEqual(canonical(result['reservation']), before)
        self.assertEqual(canonical(self.record), before)
        self.assertEqual(result['canary']['samples'], 61)

    def test_cli_rejects_corrupt_packet_before_acceptance_can_be_consumed(self):
        reservation = self.root / 'reservation.json'
        reservation.write_bytes(canonical(self.record))
        packet = self.root / 'packet.zip'
        packet.write_bytes(b'forged packet')
        result = subprocess.run([sys.executable, str(Path(gate.__file__)), '--reservation', str(reservation),
                                 '--packet', str(packet), '--gh', str(self.root / 'must-not-execute'),
                                 '--channel', 'preview', '--app-sha256', self.app,
                                 '--acceptance', str(self.root / 'missing-suite'),
                                 '--reports', str(self.root / 'missing-reports'),
                                 '--canary', str(self.root / 'missing-canary')],
                                capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 1)
        self.assertIn(b'Transport ZIP differs from reviewed digest', result.stderr)
        self.assertNotIn(b'readiness_passed', result.stdout)

    def test_checked_in_stop_and_unsupported_stable_are_independent_blockers(self):
        self.observation = self.canary(86400)
        self.policy.write_bytes(gate.POLICY.read_bytes())
        result = self.evaluate(channel='stable')
        self.assertFalse(result['readiness_passed'])
        self.assertIn('promotion-stopped', result['blockers'])
        self.policy.write_bytes(self.policy.read_bytes().replace(b'stopped = true', b'stopped = false'))
        result = self.evaluate(channel='stable')
        self.assertIsNotNone(result['canary'])  # Full 24h canary passes; stable version policy still blocks.
        self.assertEqual(result['blockers'], ['stable-version-and-publication-policy-not-enabled'])

    def test_missing_failed_and_unbound_acceptance_never_passes(self):
        cases = [lambda s: s.pop('checks'),
                 lambda s: s['checks'].pop(),
                 lambda s: s['checks'].__setitem__(0, s['checks'][1]),
                 lambda s: s['checks'][0].update(passed=False),
                 lambda s: s['checks'][0].update(passed=1),
                 lambda s: s['checks'][0].update(report_sha256='0' * 64),
                 lambda s: s['reservation']['artifact'].update(id='56'),
                 lambda s: s.update(completed_at=gate.timestamp(self.now + 1)),
                 lambda s: s.update(completed_at=gate.timestamp(self.now - 86401))]
        original = copy.deepcopy(self.suite)
        for mutate in cases:
            self.suite = copy.deepcopy(original)
            mutate(self.suite)
            with self.subTest(evidence=self.suite):
                self.assertFalse(self.evaluate()['readiness_passed'])
        self.suite = original
        self.reports.pop(gate.CHECKS[0])
        self.assertFalse(self.evaluate()['readiness_passed'])

    def test_canary_rejects_gaps_identity_replay_nonbooleans_and_failed_routes(self):
        cases = [lambda v: v.update(samples=[]),
                 lambda v: v.update(complete=False),
                 lambda v: v['samples'].pop(10),
                 lambda v: v['samples'].reverse(),
                 lambda v: v['samples'].pop(),
                 lambda v: v['samples'][0]['probes'].pop('sample_app'),
                 lambda v: v['samples'][8]['probes']['sample_app'].update(passed=False),
                 lambda v: v['samples'][8]['probes']['sign_in'].update(passed=1),
                 lambda v: v['samples'][8]['probes']['sign_in'].update(latency_ms=True),
                 lambda v: v['samples'][8]['probes']['sign_in'].update(latency_ms=-1),
                 lambda v: v.update(app_sha256='f' * 64),
                 lambda v: v.update(policy_sha256='f' * 64),
                 lambda v: v.update(workload='unknown'),
                 lambda v: v['reservation']['candidate'].update(archive_sha256='f' * 64),
                 lambda v: v['reservation'].update(schema_version=True),
                 lambda v: v.update(completed_at=gate.timestamp(self.now + 1)),
                 lambda v: v.update(started_at=gate.timestamp(self.now - 3599))]
        original = copy.deepcopy(self.observation)
        for index, mutate in enumerate(cases):
            self.observation = copy.deepcopy(original)
            mutate(self.observation)
            with self.subTest(case=index):
                self.assertFalse(self.evaluate()['readiness_passed'])

    def test_latency_is_per_route_and_boundary_is_inclusive(self):
        for sample in self.observation['samples']:
            sample['probes']['sample_app']['latency_ms'] = 2000
        self.assertTrue(self.evaluate()['readiness_passed'])
        # Four of 61 slow app samples exceed nearest-rank p95, even when both
        # platform routes are fast. Combining routes would hide this failure.
        for sample in self.observation['samples'][:4]:
            sample['probes']['sample_app']['latency_ms'] = 2001
        self.assertFalse(self.evaluate()['readiness_passed'])

    def test_policy_cannot_weaken_minimum_duration_latency_or_failure_limits(self):
        original = self.policy.read_bytes()
        cases = [(b'preview_seconds = 3600', b'preview_seconds = 3599'),
                 (b'stable_seconds = 86400', b'stable_seconds = 3600'),
                 (b'max_failures = 0', b'max_failures = 1'),
                 (b'p95_latency_ms = 2000', b'p95_latency_ms = 2001'),
                 (b'automatic_publication = false', b'automatic_publication = true'),
                 (b'stopped = false', b'stopped = 0')]
        for before, after in cases:
            self.policy.write_bytes(original.replace(before, after))
            with self.subTest(change=after), self.assertRaises(ValueError):
                gate.read_policy(self.policy)

    def test_document_rejects_duplicate_keys_nonfinite_symlink_and_oversize(self):
        path = self.root / 'evidence.json'
        for raw in (b'{"passed":true,"passed":false}', b'{"latency":NaN}', b'{"latency":Infinity}'):
            path.write_bytes(raw)
            with self.assertRaises(ValueError):
                gate.document(path)
        linked = self.root / 'link.json'
        linked.symlink_to(path)
        with self.assertRaises(OSError):
            gate.document(linked)
        path.write_bytes(b' ' * (gate.LIMIT + 1))
        with self.assertRaises(ValueError):
            gate.document(path)

    def collect(self, inspect=lambda: 'same', probe=None, sleep=None):
        elapsed = [0]
        def advance(delay):
            elapsed[0] += delay
            if sleep:
                sleep(delay)
        with patch.object(collector.time, 'monotonic', side_effect=lambda: elapsed[0]), \
                patch.object(collector.time, 'time', side_effect=lambda: self.now - 3600 + elapsed[0]), \
                patch.object(collector.time, 'sleep', side_effect=advance), \
                patch.object(collector, 'probe', side_effect=probe or (lambda *args: self.probes())):
            return collector.collect(self.record, 'preview', self.app, 'platform.example.com', 'app.example.com',
                                     self.root / 'canary.json', inspect, self.policy)

    def test_collector_full_window_and_exclusive_output(self):
        self.observation = self.collect()
        self.assertTrue(self.evaluate()['readiness_passed'])
        with self.assertRaises(FileExistsError):
            self.collect()

    def test_collector_live_stop_during_wait_retains_ineligible_partial_evidence(self):
        with self.assertRaisesRegex(ValueError, 'policy changed'):
            self.collect(sleep=lambda delay: self.policy.write_bytes(gate.POLICY.read_bytes()))
        self.observation = gate.document(self.root / 'canary.json')
        self.assertEqual(len(self.observation['samples']), 1)
        self.assertFalse(self.evaluate()['readiness_passed'])

    def test_collector_failed_probe_retains_failure_and_stops(self):
        def failure(*args):
            return self.probes() | {'sample_app': {'passed': False, 'latency_ms': 10}}
        with self.assertRaisesRegex(ValueError, 'Failed canary probe'):
            self.collect(probe=failure)
        report = gate.document(self.root / 'canary.json')
        self.assertFalse(report['samples'][0]['probes']['sample_app']['passed'])

    def test_collector_identity_switch_and_default_stop_prevent_requests(self):
        calls = iter(('initial', 'before', 'changed'))
        with self.assertRaisesRegex(ValueError, 'identity changed'):
            self.collect(inspect=lambda: next(calls))
        (self.root / 'canary.json').unlink()
        self.policy.write_bytes(gate.POLICY.read_bytes())
        with patch.object(collector, 'probe') as request:
            with self.assertRaisesRegex(ValueError, 'Promotion stopped'):
                collector.collect(self.record, 'preview', self.app, 'p.example.com', 'a.example.com',
                                  self.root / 'canary.json', lambda: 'same', self.policy)
            request.assert_not_called()
        self.assertFalse((self.root / 'canary.json').exists())

    def test_probe_checks_redirect_health_body_and_tls_failure(self):
        with patch.object(collector, 'request', side_effect=[(200, None, b'login'),
                    (303, '/elsewhere', b''), (200, None, b'wrong app')]):
            result = collector.probe('platform.example.com', 'app.example.com')
        self.assertTrue(result['sign_in']['passed'])
        self.assertFalse(result['owner_redirect']['passed'])
        self.assertFalse(result['sample_app']['passed'])
        with patch.object(collector, 'request', side_effect=OSError('TLS unavailable')):
            self.assertFalse(any(r['passed'] for r in collector.probe('p.example.com', 'a.example.com').values()))

    def test_request_total_deadline_and_handler_cleanup(self):
        before = collector.signal.getsignal(collector.signal.SIGALRM)
        with self.assertRaises(TimeoutError):
            with collector.request_deadline(.01):
                time.sleep(.1)
        self.assertEqual(collector.signal.getsignal(collector.signal.SIGALRM), before)
        self.assertEqual(collector.signal.getitimer(collector.signal.ITIMER_REAL), (0.0, 0.0))

    def test_local_identity_rejects_other_binary_invocation_or_stale_runtime_config(self):
        release = '/opt/devoption/releases/0.1.0-preview.3/'
        config = ('worker = "' + release + 'bin/devoption-runtime"\n'
                  'state = "/var/lib/devoption-app"\nport = 8081\n'
                  '[ingress]\nplatform_hostname = "p.example.com"\napp_hostname = "a.example.com"\n'
                  'project = "' + '1' * 32 + '"\nplatform_port = 3000\n').encode()
        receipt = {'schema_version': 1, 'reservation': self.record, 'hostname': 'p.example.com',
                   'platform_port': 3000, 'proxy_sha512': collector.CADDY_SHA512}
        desired = {'release': 'f' * 64, 'intent': 'running', 'observed': 'healthy', 'health': 'ok A',
                   'project': '1' * 32, 'invocation': '2' * 32,
                   'fingerprint': gate.sha(config + b'broker' + b'worker')}
        reads = {'/var/lib/devoption-installer/installation.json': canonical(receipt),
                 '/etc/devoption/deployment.toml': config,
                 '/var/lib/devoption-app/desired.json': canonical(desired),
                 str(collector.BROKER): b'broker', release + 'bin/devoption-runtime': b'worker'}
        binaries = {'bin/devoption': b'platform', 'bin/devoption-runtime': b'worker'}
        def state(args, **options):
            return SimpleNamespace(stdout=('MainPID=' + ('123' if args[2] == 'devoption-public.service' else '456') +
                                           '\nInvocationID=' + '2' * 32 + '\nActiveState=active\n').encode())
        def link(path):
            return release + ('bin/devoption' if path == '/proc/123/exe' else 'bin/devoption-runtime')
        def inspect():
            return collector.installed(self.record, binaries, 'p.example.com', 'a.example.com', self.app)
        with patch.object(collector.os, 'geteuid', return_value=0), \
                patch.object(collector, 'protected'), \
                patch.object(collector, 'read_regular', side_effect=lambda path, *args: reads[str(path)]), \
                patch.object(collector, 'load_release', return_value={'artifact_sha256': self.app, 'project': '1' * 32}), \
                patch.object(collector, 'snapshot', side_effect=lambda path, *args: binaries[str(path)[len(release):]]), \
                patch.object(collector.subprocess, 'run', side_effect=state), \
                patch.object(collector.os, 'readlink', side_effect=link) as readlink:
            self.assertEqual(set(inspect()), {'devoption-public.service', 'devoption-app.service'})
            readlink.side_effect = lambda path: '/opt/foreign-binary'
            with self.assertRaisesRegex(ValueError, 'authenticated binary'):
                inspect()
            readlink.side_effect = link
            for field, value in [('invocation', '3' * 32), ('fingerprint', '0' * 64), ('intent', 'stopped')]:
                bad = desired | {field: value}
                reads['/var/lib/devoption-app/desired.json'] = canonical(bad)
                with self.subTest(field=field), self.assertRaises(ValueError):
                    inspect()
            reads['/var/lib/devoption-app/desired.json'] = canonical(desired)
            reads['/var/lib/devoption-installer/installation.json'] = canonical(receipt | {'reservation': {}})
            with self.assertRaisesRegex(ValueError, 'Local installation differs'):
                inspect()


if __name__ == '__main__':
    unittest.main()
