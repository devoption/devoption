"""Read-only candidate readiness gates; never rewrite, retag or publish a packet."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess
import tomllib

from install_bundle import verified_files
from release_candidates import load, validate
from release_bundle import canonical
from verify_bundle_provenance import snapshot, unique_object

POLICY = Path(__file__).resolve().parents[1] / 'release-promotion.toml'
LIMIT = 8 * 1024 * 1024
MAX_SAMPLES = 10081  # Seven days, one observation per minute, including endpoints.
PROBES = ('sign_in', 'owner_redirect', 'sample_app')
CHECKS = ('fresh_install', 'owner_login', 'project_git_persistence', 'app_build_deploy',
          'platform_app_upgrade', 'running_app_reboot', 'stopped_app_reboot',
          'failed_upgrade_recovery', 'interrupted_upgrade_recovery', 'corrupt_packet_refusal')
FIELDS = {'schema_version', 'stopped', 'automatic_publication', 'preview_seconds',
          'stable_seconds', 'max_gap_seconds', 'max_age_seconds', 'p95_latency_ms', 'max_failures'}


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def read_policy(path=POLICY):
    raw = snapshot(path, 4096)
    policy = tomllib.loads(raw.decode())
    if (set(policy) != FIELDS or type(policy['schema_version']) is not int or policy['schema_version'] != 1 or
            type(policy['stopped']) is not bool or policy['automatic_publication'] is not False):
        raise ValueError('Invalid policy; automatic publication is not implemented.')
    bounds = {'preview_seconds': (3600, 604800), 'stable_seconds': (86400, 604800),
              'max_gap_seconds': (60, 90), 'max_age_seconds': (60, 86400),
              'p95_latency_ms': (1, 2000), 'max_failures': (0, 0)}
    for key, (low, high) in bounds.items():
        if type(policy[key]) is not int or not low <= policy[key] <= high:
            raise ValueError('Policy threshold outside reviewed bounds: ' + key)
    if policy['stable_seconds'] < policy['preview_seconds']:
        raise ValueError('Stable observation must be at least as long as preview.')
    return policy, sha(raw)


def stamp(value):
    if not isinstance(value, str) or not re.fullmatch(r'\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ', value):
        raise ValueError('Evidence times must be UTC seconds with a Z suffix.')
    return datetime.strptime(value, '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=timezone.utc).timestamp()


def timestamp(seconds):
    return datetime.fromtimestamp(seconds, timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')


def document(path):
    return json.loads(snapshot(path, LIMIT), object_pairs_hook=unique_object,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError('Nonfinite JSON number.')))


def same_record(value, record):
    # Comparing Python dictionaries alone would equate JSON true and integer 1.
    if canonical(value) != canonical(record):
        raise ValueError('Evidence must identify the complete independently reviewed reservation.')


def recent(value, now, age):
    time = stamp(value)
    if not 0 <= now - time <= age:
        raise ValueError('Evidence is stale or from the future.')
    return time


def acceptance(value, record, now, policy, reports):
    if (not isinstance(value, dict) or set(value) != {'schema_version', 'reservation', 'completed_at', 'checks'} or
            type(value['schema_version']) is not int or value['schema_version'] != 1):
        raise ValueError('Incomplete acceptance evidence schema.')
    same_record(value['reservation'], record)
    recent(value['completed_at'], now, policy['max_age_seconds'])
    rows = value['checks']
    if not isinstance(rows, list) or len(rows) != len(CHECKS):
        raise ValueError('Every required acceptance case needs exactly one result.')
    seen = set()
    for row in rows:
        if (not isinstance(row, dict) or set(row) != {'case', 'passed', 'report_sha256'} or
                not isinstance(row['case'], str) or row['case'] not in CHECKS or row['case'] in seen or
                row['passed'] is not True or not isinstance(row['report_sha256'], str) or
                not re.fullmatch('[0-9a-f]{64}', row['report_sha256']) or
                reports.get(row['case']) != row['report_sha256']):
            raise ValueError('Missing, failed, duplicate or unbound acceptance case.')
        seen.add(row['case'])


def canary(value, record, now, policy, policy_sha, channel, app_sha):
    if (not isinstance(value, dict) or set(value) != {'schema_version', 'reservation', 'policy_sha256',
            'workload', 'app_sha256', 'started_at', 'completed_at', 'samples', 'complete'} or
            type(value['schema_version']) is not int or value['schema_version'] != 1 or
            value['complete'] is not True or
            value['policy_sha256'] != policy_sha or value['workload'] != 'trusted-wasip2-example-A-v1' or
            value['app_sha256'] != app_sha):
        raise ValueError('Canary identity, workload or policy differs.')
    same_record(value['reservation'], record)
    end = recent(value['completed_at'], now, policy['max_age_seconds'])
    start = stamp(value['started_at'])
    duration = end - start
    if not policy[channel + '_seconds'] <= duration <= 604800:
        raise ValueError('Insufficient or excessive canary duration.')
    rows = value['samples']
    if not isinstance(rows, list) or not 2 <= len(rows) <= MAX_SAMPLES:
        raise ValueError('Incomplete or oversized canary samples.')
    last = None
    latencies = {probe: [] for probe in PROBES}
    for sample in rows:
        if not isinstance(sample, dict) or set(sample) != {'at', 'probes'}:
            raise ValueError('Invalid canary sample.')
        at = stamp(sample['at'])
        if (not start <= at <= end or (last is None and at != start) or
                (last is not None and not 60 <= at - last <= policy['max_gap_seconds'])):
            raise ValueError('Canary samples are missing, reordered or too far apart.')
        probes = sample['probes']
        if not isinstance(probes, dict) or set(probes) != set(PROBES):
            raise ValueError('Every sample needs all three workload probes.')
        for name, result in probes.items():
            if (not isinstance(result, dict) or set(result) != {'passed', 'latency_ms'} or
                    result['passed'] is not True or type(result['latency_ms']) is not int or
                    not 0 <= result['latency_ms'] <= 15000):
                raise ValueError('A failed or invalid canary probe stops readiness.')
            latencies[name].append(result['latency_ms'])
        last = at
    if last != end:
        raise ValueError('Missing final canary sample.')
    p95 = {name: sorted(values)[math.ceil(len(values) * .95) - 1] for name, values in latencies.items()}
    if any(value > policy['p95_latency_ms'] for value in p95.values()):
        raise ValueError('Canary per-route p95 latency exceeds policy.')
    return {'duration_seconds': int(duration), 'samples': len(rows), 'failures': 0, 'p95_latency_ms': p95}


def evaluate(record, channel, suite, observation, app_sha, now, reports, policy_path=POLICY):
    validate(record)
    if channel not in ('preview', 'stable') or not re.fullmatch('[0-9a-f]{64}', app_sha):
        raise ValueError('Select preview/stable and the independently reviewed app component digest.')
    policy, policy_sha = read_policy(policy_path)
    blockers = []
    metrics = None
    if policy['stopped']:
        blockers.append('promotion-stopped')
    for name, check in (
            ('acceptance', lambda: acceptance(suite, record, now, policy, reports)),
            ('canary', lambda: canary(observation, record, now, policy, policy_sha, channel, app_sha))):
        try:
            result = check()
            if name == 'canary':
                metrics = result
        except (ValueError, TypeError, KeyError, OverflowError) as error:
            blockers.append(name + ': ' + str(error))
    # Version/metadata currently admit numbered previews only. Passing a 24-hour
    # workload must never turn a preview into a stable artifact with a new digest.
    if channel == 'stable':
        blockers.append('stable-version-and-publication-policy-not-enabled')
    return {'schema_version': 1, 'checked_at': timestamp(now), 'reservation': record,
            'channel': channel, 'policy_sha256': policy_sha, 'readiness_passed': not blockers,
            'blockers': blockers, 'canary': metrics, 'publication_eligible': False,
            'reviewed_reports': reports, 'app_sha256': app_sha,
            'acceptance_sha256': sha(canonical(suite)), 'canary_sha256': sha(canonical(observation))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reservation', type=Path, required=True)
    parser.add_argument('--packet', type=Path, required=True)
    parser.add_argument('--gh', type=Path, required=True)
    parser.add_argument('--channel', choices=('preview', 'stable'), required=True)
    parser.add_argument('--acceptance', type=Path, required=True)
    parser.add_argument('--reports', type=Path, required=True, help='Reviewed case.json reports, one per required case')
    parser.add_argument('--canary', type=Path, required=True)
    parser.add_argument('--app-sha256', required=True)
    args = parser.parse_args()
    try:
        _, before = read_policy()
        record = load(snapshot(args.reservation, 8192))
        manifest, _ = verified_files(args.packet, record, args.gh, True)
        if manifest['schema_version'] != 3:
            raise ValueError('Readiness requires a schema-3 packet with the bundled app worker.')
        reports = {case: sha(snapshot(args.reports / (case + '.json'), LIMIT)) for case in CHECKS}
        result = evaluate(record, args.channel, document(args.acceptance), document(args.canary),
                          args.app_sha256, datetime.now(timezone.utc).timestamp(), reports)
        if read_policy()[1] != before:
            raise ValueError('Promotion policy changed during verification; rerun against current stop state.')
        print(json.dumps(result, indent=2))
        return 0 if result['readiness_passed'] else 1
    except (ValueError, TypeError, KeyError, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, f'Readiness stopped: {error}\nNo publication performed.\n')


if __name__ == '__main__':
    raise SystemExit(main())
