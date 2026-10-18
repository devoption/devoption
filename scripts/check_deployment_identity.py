#!/usr/bin/python3
"""Opt-in destructive validation on a dedicated disposable Linux installation.

Uses the installed broker config, an explicit saved fixture release and reserved
units. Never run against a user's installation. See docs/deployment-identity-validation.md.
"""
import argparse
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import time
from unittest.mock import patch

import deployment_broker as broker

SENTINEL = 'devoption-identity-sentinel.service'


def fixture():
    if sys.argv[1] == 'fixture-orphan':
        if os.fork() == 0:
            signal.signal(signal.SIGTERM, signal.SIG_IGN)
        time.sleep(120)
        return
    port, marker, body = int(sys.argv[2]), sys.argv[3], sys.argv[4]
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(200)
            if marker != 'missing':
                self.send_header('X-DevOption-Invocation', os.environ['INVOCATION_ID'] if marker == 'current' else marker)
            self.end_headers()
            self.wfile.write(body.encode())
        def log_message(self, *_): pass
    HTTPServer(('127.0.0.1', port), Handler).serve_forever()


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def wait_for(probe):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if probe(): return
        time.sleep(.1)
    raise AssertionError('fixture readiness deadline')


def listening(port):
    try:
        with socket.create_connection(('127.0.0.1', port), timeout=.2): return True
    except OSError: return False


def run_fixture(unit, *args, orphan=False):
    properties = ['DynamicUser=yes', 'NoNewPrivileges=yes', 'MemoryMax=67108864',
                  'TasksMax=8', 'RuntimeMaxSec=120', 'TimeoutStopSec=1', 'Restart=no']
    if orphan: properties += ['KillMode=process', 'SendSIGKILL=no']
    command = ['/usr/bin/systemd-run', '--quiet', '--collect', '--unit='+unit]
    for prop in properties: command += ['--property='+prop]
    broker.command(command + ['/usr/bin/python3', str(Path(__file__).resolve()), *args])


def cleanup(unit):
    # Only the explicitly reserved fixture units; never a PID or a port owner.
    broker.command(['/usr/bin/systemctl', 'kill', '--kill-whom=all', '--signal=KILL', unit], check=False)
    broker.stop_unit(unit)


def rejected(operation, message):
    try: operation()
    except ValueError as error:
        require(message in str(error), 'unexpected rejection: '+str(error))
        return str(error)
    raise AssertionError('operation was accepted')


def check(args):
    require(sys.platform == 'linux' and os.geteuid() == 0, 'disposable Linux root session required')
    require(Path(__file__).with_name('deployment_broker.py').read_bytes() == broker.BROKER.read_bytes(),
            'install the exact reviewed broker beside this checker')
    config = broker.configuration()
    broker.load_release(config, args.release)
    with (config['state'] / 'broker.lock').open('a') as lock:
        broker.fcntl.flock(lock, broker.fcntl.LOCK_EX | broker.fcntl.LOCK_NB)
        validate(config, args)


def validate(config, args):
    original = broker.desired(config)
    require(not original or original['intent'] == 'stopped', 'stop the disposable app first')
    for unit in [broker.UNIT, broker.VALIDATE_UNIT, SENTINEL]:
        state = broker.unit_state(unit)
        require(state.get('ActiveState') in ('inactive', 'failed') and not state.get('ControlGroup'), 'reserved fixture unit is busy')
    require(not listening(config['port']), 'fixture port already occupied')
    evidence = {'broker_sha256': broker.digest(broker.BROKER.read_bytes()),
                'worker_sha256': broker.digest(config['worker'].read_bytes()),
                'release': args.release, 'cases': {}}
    def passed(name, **details):
        evidence['cases'][name] = details
        print(json.dumps({'passed': name, **details}), flush=True)
    try:
        # A real Wasmtime candidate must not mistake an unrelated HTTP listener
        # (even one serving the exact expected body) for successful deployment.
        run_fixture(SENTINEL, 'fixture-http', str(config['port']), 'missing', args.health)
        wait_for(lambda: listening(config['port']))
        sentinel = broker.unit_state(SENTINEL)['InvocationID']
        error = rejected(lambda: broker.start(config, args.release, args.health, config['fingerprint']), 'health check failed')
        record = broker.status(config)['deployment']
        require(record['observed'] == 'failed' and record['intent'] == 'stopped', 'collision incorrectly reported success')
        require(broker.unit_state(SENTINEL)['InvocationID'] == sentinel and listening(config['port']), 'unrelated listener was stopped')
        require(not broker.unit_state().get('ControlGroup'), 'candidate cgroup remains')
        passed('occupied-port', result=record['observed'], sentinel_preserved=True, error=error)
        cleanup(SENTINEL)

        # An owned fixture with no header exercises the missing/missing regression
        # over a real socket. A correct marker is the positive control.
        for marker in ['missing', 'b'*32, 'current']:
            run_fixture(broker.UNIT, 'fixture-http', str(config['port']), marker, args.health)
            wait_for(lambda: listening(config['port']))
            invocation = broker.unit_state()['InvocationID']
            require(broker.health(config, args.health, invocation) == (marker == 'current'), 'wrong marker acceptance')
            if marker == 'missing':
                require(not broker.health(config, args.health, None), 'missing identity accepted')
            if marker == 'current':
                first = invocation
            passed('marker-'+{'missing': 'missing', 'b'*32: 'wrong', 'current': 'current'}[marker], accepted=marker == 'current')
            cleanup(broker.UNIT)
        # Restart with a new systemd identity but replay the previous header.
        run_fixture(broker.UNIT, 'fixture-http', str(config['port']), first, args.health)
        wait_for(lambda: listening(config['port']))
        second = broker.unit_state()['InvocationID']
        require(first != second and not broker.health(config, args.health, first), 'changed invocation accepted')
        record = broker.desired(config)
        record.update(intent='running', observed='healthy', invocation=first)
        broker.save(config, record)
        require(broker.status(config)['deployment']['observed'] == 'failed', 'stale status reported healthy')
        passed('changed-invocation', status='failed', invocation_changed=True)

        # Suppress only systemctl stop as explicit fault injection. Unit state,
        # cgroup, compatibility validation and HTTP are real Linux observations.
        command = broker.command
        def suppress_stop(argv, *a, **kw):
            if argv[:3] == ['/usr/bin/systemctl', 'stop', broker.UNIT]:
                return subprocess.CompletedProcess(argv, 1, b'', b'injected stop refusal')
            return command(argv, *a, **kw)
        with patch.object(broker, 'command', side_effect=suppress_stop):
            rejected(lambda: broker.dispatch(config, ['stop']), 'stop is unconfirmed')
            require(broker.status(config)['deployment']['observed'] == 'stopping', 'uncertain stop reported stopped')
            rejected(lambda: broker.start(config, args.release, args.health, config['fingerprint']), 'stop is unconfirmed')
        require(broker.unit_state()['InvocationID'] == second, 'uncertain cleanup replaced worker')
        passed('unconfirmed-stop', status='stopping', replacement_blocked=True, fault_injection='systemctl stop suppressed')
        cleanup(broker.UNIT)

        # Real inactive unit with a surviving child: KillMode=process is a test
        # fault, never the production policy. No mock state/cgroup is supplied.
        run_fixture(broker.UNIT, 'fixture-orphan', orphan=True)
        def two_processes():
            group = broker.unit_state().get('ControlGroup')
            return group and len((Path('/sys/fs/cgroup') / group.lstrip('/') / 'cgroup.procs').read_text().split()) >= 2
        wait_for(two_processes)
        rejected(lambda: broker.dispatch(config, ['stop']), 'stop is unconfirmed')
        state = broker.unit_state()
        require(state.get('ActiveState') in ('inactive', 'failed') and state.get('ControlGroup'), 'nonempty inactive cgroup not established')
        rejected(lambda: broker.start(config, args.release, args.health, config['fingerprint']), 'stop is unconfirmed')
        require(broker.status(config)['deployment']['observed'] == 'stopping', 'residual cgroup reported stopped')
        passed('nonempty-cgroup', active_state=state['ActiveState'], cgroup_retained=True, replacement_blocked=True)
        cleanup(broker.UNIT)
        broker.dispatch(config, ['stop'])
        # Finally exercise the real Wasmtime positive control and explicit stop.
        broker.start(config, args.release, args.health, config['fingerprint'])
        require(broker.status(config)['deployment']['observed'] == 'healthy', 'positive runtime control failed')
        broker.dispatch(config, ['stop'])
        require(broker.status(config)['deployment']['observed'] == 'stopped', 'final stop not confirmed')
        passed('real-worker-control', healthy=True, final_status='stopped')
    finally:
        for unit in [broker.UNIT, broker.VALIDATE_UNIT, SENTINEL]: cleanup(unit)
        # Never leave approved running intent behind, including after an assertion.
        record = broker.desired(config)
        if record:
            record.update(intent='stopped', observed='stopped')
            broker.save(config, record)
    print(json.dumps(evidence, sort_keys=True), flush=True)


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] in ['fixture-http', 'fixture-orphan']:
        fixture()
    else:
        parser = argparse.ArgumentParser(description=__doc__)
        parser.add_argument('--disposable-systemd', required=True, action='store_true')
        parser.add_argument('--release', required=True)
        parser.add_argument('--health', required=True)
        check(parser.parse_args())
