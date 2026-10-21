"""Observe an already installed, approved candidate; never install or start workloads.

Run on the canary node as root to read protected installation/process identities.
Requests use normal system TLS trust, pinned loopback and no credentials/cookies.
"""

import argparse
from contextlib import contextmanager
import http.client
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import tomllib

from check_public_app import request
from deployment_broker import protected, read_regular, load_release, hostname, validate_ingress, BROKER
from install_bundle import verified_files, STATE
from release_bundle import canonical, RUNTIME, CADDY_SHA512
from release_candidates import load
from release_promotion import POLICY, MAX_SAMPLES, sha, read_policy, timestamp
from verify_bundle_provenance import snapshot, unique_object


@contextmanager
def request_deadline(seconds=10):
    # Socket timeouts alone do not bound a peer that drips bytes indefinitely.
    # This Linux operator CLI runs in its main thread with no other alarm owner.
    if signal.getitimer(signal.ITIMER_REAL) != (0.0, 0.0):
        raise ValueError('Canary request deadline already owned by another operation.')
    previous = signal.getsignal(signal.SIGALRM)
    def expired(signum, frame):
        raise TimeoutError('Canary request exceeded its total deadline.')
    signal.signal(signal.SIGALRM, expired)
    signal.setitimer(signal.ITIMER_REAL, seconds)
    try:
        yield
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)


def installed(record, files, platform, app, app_sha):
    if os.geteuid() != 0:
        raise ValueError('Canary collection needs local root inspection of the installed candidate.')
    receipt_path = Path(STATE) / 'installation.json'
    protected(receipt_path)
    receipt = json.loads(read_regular(receipt_path, 16384, 0), object_pairs_hook=unique_object)
    if (set(receipt) != {'schema_version', 'reservation', 'hostname', 'platform_port', 'proxy_sha512'} or
            canonical(receipt['reservation']) != canonical(record) or receipt['hostname'] != platform or
            type(receipt['schema_version']) is not int or receipt['schema_version'] != 1 or
            type(receipt['platform_port']) is not int or receipt['platform_port'] != 3000 or
            receipt['proxy_sha512'] != CADDY_SHA512):
        raise ValueError('Local installation differs from the reviewed candidate/hostname.')
    config_path = Path('/etc/devoption/deployment.toml')
    protected(config_path)
    config_bytes = read_regular(config_path, 16384, 0)
    config = tomllib.loads(config_bytes.decode())
    release_root = Path('/opt/devoption/releases') / record['candidate']['platform_version']
    worker = release_root / RUNTIME
    if config['worker'] != str(worker):
        raise ValueError('App must use this candidate bundled worker.')
    ingress = config['ingress']
    validate_ingress(ingress, config['port'])
    if ingress['platform_hostname'] != platform or ingress['app_hostname'] != app or 'ca_file' in ingress:
        raise ValueError('Canary requires both reviewed hostnames and normal public TLS trust.')
    config['state'] = Path(config['state'])
    protected(config['state'])
    desired = json.loads(read_regular(config['state'] / 'desired.json', 16384, 0), object_pairs_hook=unique_object)
    manifest = load_release(config, desired['release'])
    if (desired['intent'] != 'running' or desired['observed'] != 'healthy' or desired['health'] != 'ok A' or
            manifest['artifact_sha256'] != app_sha or manifest['project'] != desired['project'] or
            desired['project'] != ingress['project'] or ingress['platform_port'] != 3000):
        raise ValueError('Reviewed example A is not the healthy selected deployment.')
    protected(BROKER)
    protected(worker)
    if desired['fingerprint'] != sha(config_bytes + read_regular(BROKER, 65536, 0) +
                                     read_regular(worker, 256 * 1024 * 1024, 0)):
        raise ValueError('Saved app runtime fingerprint differs from installed configuration.')
    processes = {}
    for unit, name in (('devoption-public.service', 'bin/devoption'), ('devoption-app.service', RUNTIME)):
        binary = release_root / name
        protected(binary)
        if snapshot(binary, 256 * 1024 * 1024) != files[name]:
            raise ValueError('Installed executable differs from the authenticated packet.')
        result = subprocess.run(['/usr/bin/systemctl', 'show', unit, '--property=MainPID,InvocationID,ActiveState'],
                                capture_output=True, check=True, timeout=5,
                                env={'PATH': '/usr/bin:/bin', 'LANG': 'C.UTF-8'})
        values = dict(line.split('=', 1) for line in result.stdout.decode().splitlines() if '=' in line)
        pid = values['MainPID']
        if (not pid.isdecimal() or int(pid) <= 1 or values['ActiveState'] != 'active' or
                os.readlink('/proc/' + pid + '/exe') != str(binary)):
            raise ValueError('Active service does not execute the authenticated binary.')
        if unit == 'devoption-app.service' and values['InvocationID'] != desired['invocation']:
            raise ValueError('Active app invocation differs from the healthy deployment.')
        processes[unit] = (pid, values['InvocationID'])
    return processes


def probe(platform, app):
    results = {}
    for name, host, path, expected in (('sign_in', platform, '/sign-in', 200),
                                       ('owner_redirect', platform, '/projects', 303),
                                       ('sample_app', app, '/healthz', 200)):
        start = time.monotonic()
        try:
            with request_deadline():
                status, location, body = request('127.0.0.1', host, path)
            passed = status == expected and (name != 'owner_redirect' or location == '/sign-in')
            passed = passed and (name != 'sample_app' or body == b'ok A')
        except (OSError, ValueError, http.client.HTTPException):
            passed = False
        results[name] = {'passed': passed, 'latency_ms': math_ms(time.monotonic() - start)}
    return results


def math_ms(seconds):
    return max(0, int(seconds * 1000 + .5))


def write(path, report):
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as stream:
        temporary = Path(stream.name)
        try:
            stream.write(canonical(report))
            stream.flush()
            os.fsync(stream.fileno())
            os.replace(temporary, path)
        finally:
            temporary.unlink(missing_ok=True)


def collect(record, channel, app_sha, platform, app, output, inspect, policy_path=POLICY):
    # inspect() is the trusted local identity check, injected only by unit tests.
    hostname(platform)
    hostname(app)
    if platform == app or channel not in ('preview', 'stable'):
        raise ValueError('Use distinct hostnames and a preview/stable observation window.')
    policy, checksum = read_policy(policy_path)
    if policy['stopped']:
        raise ValueError('Promotion stopped; no canary requests made.')
    identity = inspect()
    with output.open('xb') as stream:  # Never adopt stale/partial output from a previous collection.
        os.fchmod(stream.fileno(), 0o600)
    start = time.monotonic()
    wall_start = int(time.time())
    report = {'schema_version': 1, 'reservation': record, 'policy_sha256': checksum,
              'workload': 'trusted-wasip2-example-A-v1', 'app_sha256': app_sha,
              'started_at': timestamp(wall_start), 'completed_at': timestamp(wall_start),
              'samples': [], 'complete': False}
    write(output, report)
    for count in range(MAX_SAMPLES):
        current, current_sha = read_policy(policy_path)
        if current['stopped'] or current_sha != checksum:
            raise ValueError('Promotion policy changed/stopped during observation.')
        at = int(time.time())
        elapsed = time.monotonic() - start
        if abs(at - wall_start - elapsed) > 5:
            raise ValueError('Wall clock changed during observation.')
        before = inspect()
        if before != identity:
            raise ValueError('Service identity changed since observation started.')
        results = probe(platform, app)
        if inspect() != before:
            raise ValueError('Service identity changed during probes.')
        report['samples'].append({'at': timestamp(at), 'probes': results})
        report['completed_at'] = timestamp(at)
        write(output, report)
        if not all(value['passed'] for value in results.values()):
            raise ValueError('Failed canary probe; partial failure evidence retained.')
        # Check live stop state after requests too; a changed file cannot pass.
        if read_policy(policy_path)[1] != checksum:
            raise ValueError('Promotion policy changed during probes.')
        if at - wall_start >= policy[channel + '_seconds']:
            report['complete'] = True
            write(output, report)
            return report
        delay = start + (count + 1) * 60 - time.monotonic()
        if delay <= 0:
            raise ValueError('Probe deadline exceeded; observation coverage lost.')
        # Wake at most five seconds after a stop policy change. These are
        # local waits, not service management or background scheduling.
        while delay > 0:
            time.sleep(min(5, delay))
            if read_policy(policy_path)[1] != checksum:
                raise ValueError('Promotion policy changed during observation wait.')
            delay = start + (count + 1) * 60 - time.monotonic()
    raise ValueError('Canary sample budget exhausted.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reservation', type=Path, required=True)
    parser.add_argument('--packet', type=Path, required=True)
    parser.add_argument('--gh', type=Path, required=True)
    parser.add_argument('--channel', choices=('preview', 'stable'), required=True)
    parser.add_argument('--platform-hostname', required=True)
    parser.add_argument('--app-hostname', required=True)
    parser.add_argument('--app-sha256', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        record = load(snapshot(args.reservation, 8192))
        _, files = verified_files(args.packet, record, args.gh, True)
        inspect = lambda: installed(record, files, args.platform_hostname, args.app_hostname, args.app_sha256)
        collect(record, args.channel, args.app_sha256, args.platform_hostname, args.app_hostname, args.output, inspect)
        print('Canary observation complete; evaluate readiness separately. No publication performed.')
    except (ValueError, TypeError, KeyError, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, f'Canary stopped: {error}\nNo services or publication changed.\n')


if __name__ == '__main__':
    main()
