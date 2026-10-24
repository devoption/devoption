#!/usr/bin/env python3
"""Explicit disposable Unix-socket PostgreSQL / WASIp2 append proof.

Creates no host service, password or public listener. Requires preinstalled
PostgreSQL tools and a built worker/component. Deletes only its own temp tree.
"""
import argparse
import getpass
import http.client
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def check(pg_bin, worker, component):
    def run(args, **kw):
        return subprocess.run([str(x) for x in args], check=True, capture_output=True,
                              text=True, timeout=30, **kw).stdout.strip()

    with tempfile.TemporaryDirectory(prefix='devoption-records-', dir='/tmp') as tmp:
        root = Path(tmp)
        os.chmod(root, 0o700)
        data, sockets = root/'data', root/'socket'
        sockets.mkdir(mode=0o700)
        user = getpass.getuser()
        pg_started, proc = False, None
        log = (root/'worker.log').open('w')

        def pg_start(fsync='on'):
            nonlocal pg_started
            run([pg_bin/'pg_ctl', '-D', data, '-l', root/'postgres.log', '-o',
                 f"-k {sockets} -h '' -p 5432 -c fsync={fsync}", '-w', 'start'])
            pg_started = True

        def pg_stop():
            nonlocal pg_started
            run([pg_bin/'pg_ctl', '-D', data, '-w', 'stop', '-m', 'immediate'])
            pg_started = False

        def sql(text):
            return run([pg_bin/'psql', '-X', '-At', '-h', sockets, '-U', user,
                        '-d', 'records_proof', '-v', 'ON_ERROR_STOP=1', '-c', text])

        grant = {'app_id': '0123456789abcdef0123456789abcdef', 'collection': 'notes',
                 'socket_dir': str(sockets), 'database': 'records_proof', 'user': user,
                 'fields': {'label': {'required': True, 'max_chars': 100},
                            'note': {'required': False, 'max_chars': 1000, 'multiline': True}}}
        grant_path = root/'grant.json'
        with socket.socket() as probe:
            probe.bind(('127.0.0.1', 0))
            port = probe.getsockname()[1]

        def request(path, payload=None):
            c = http.client.HTTPConnection('127.0.0.1', port, timeout=6)
            try:
                c.request('POST' if payload is not None else 'GET', path, body=payload,
                          headers={'Content-Type': 'application/json'})
                r = c.getresponse()
                return r.status, r.read().decode()
            finally:
                c.close()

        def worker_start():
            nonlocal proc
            grant_path.write_text(json.dumps(grant))
            proc = subprocess.Popen([str(worker), 'records-worker', str(component), str(port),
                                     'serve', str(grant_path)], stdout=log, stderr=log)
            deadline = time.monotonic()+15
            while time.monotonic() < deadline:
                if proc.poll() is not None:
                    raise RuntimeError('record worker exited before readiness')
                try:
                    if request('/healthz') == (200, 'ok records'):
                        return
                except (OSError, http.client.HTTPException):
                    pass
                time.sleep(0.05)
            raise TimeoutError('worker readiness')

        def worker_stop():
            nonlocal proc
            if proc is not None:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait(timeout=5)
                proc = None

        try:
            run([pg_bin/'initdb', '-D', data, '-A', 'peer', '--no-locale', '-E', 'UTF8'])
            pg_start()
            run([pg_bin/'createdb', '-h', sockets, '-U', user, 'records_proof'])
            sql((ROOT/'crates/devoption-runtime/src/records.sql').read_text())
            # The same component must fail validation without the grant linker.
            denied = subprocess.run([str(worker), 'app-worker', str(component), str(port),
                                     'validate'], capture_output=True, timeout=15)
            assert denied.returncode != 0
            worker_start()
            assert request('/denials') == (200, 'files environment sockets DNS outgoing HTTP denied')
            key = '/append/12345678-1234-4234-8234-123456789abc'
            payload = '{"label":"generic proof","note":"optional detail"}'
            assert request(key, payload) == (201, 'created')
            assert sql('SELECT count(*) FROM app_records.entries') == '1'
            assert sql("SELECT payload->>'note' FROM app_records.entries") == 'optional detail'
            assert request(key, '{"note":"optional detail","label":" generic proof "}') == (200, 'replayed')
            assert request(key, '{"label":"different"}') == (409, 'conflict')
            for invalid in ['{}', '{"label":"x","sql":"DROP TABLE x"}',
                            '{"label":"a","label":"b"}', '{"label":"x","note":1}',
                            '{"label":"\\u0000"}']:
                assert request(key, invalid) == (422, 'invalid')
            # A lost HTTP acknowledgement must be safe to retry with the same ID.
            lost = '/append/aaaaaaaa-1234-4234-8234-123456789abc'
            body = b'{"label":"lost reply"}'
            with socket.create_connection(('127.0.0.1', port), timeout=3) as conn:
                conn.sendall(f'POST {lost} HTTP/1.1\r\nHost: fixture.test\r\nContent-Length: {len(body)}\r\n\r\n'.encode()+body)
            deadline = time.monotonic()+3
            while sql('SELECT count(*) FROM app_records.entries') != '2':
                assert time.monotonic() < deadline, 'lost reply did not commit'
                time.sleep(0.05)
            assert request(lost, body) == (200, 'replayed')
            # A blocked write returns unavailable within the host I/O budget.
            locker = subprocess.Popen([str(pg_bin/'psql'), '-X', '-At', '-h', str(sockets),
                                       '-U', user, '-d', 'records_proof', '-c',
                                       'BEGIN; LOCK app_records.entries IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(10)'],
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                lock_query = "SELECT pid FROM pg_locks WHERE relation='app_records.entries'::regclass AND mode='AccessExclusiveLock' AND granted"
                deadline = time.monotonic()+3
                while not sql(lock_query):
                    assert time.monotonic() < deadline, 'lock setup'
                    time.sleep(0.05)
                started = time.monotonic()
                assert request(key, payload) == (503, 'unavailable')
                assert time.monotonic()-started < 3
                sql('SELECT pg_terminate_backend(pid) FROM pg_locks WHERE relation=\'app_records.entries\'::regclass AND mode=\'AccessExclusiveLock\' AND granted')
            finally:
                locker.terminate()
                locker.wait(timeout=5)
            worker_stop()
            pg_stop()  # Crash-stop after acknowledged COMMIT, then recovery.
            pg_start()
            worker_start()
            assert request(key, payload) == (200, 'replayed')
            assert sql('SELECT count(*) FROM app_records.entries') == '2'
            # Different host-selected namespace cannot overwrite or replay app A.
            worker_stop()
            grant['app_id'] = 'fedcba9876543210fedcba9876543210'
            worker_start()
            assert request(key, '{"label":"isolated app"}') == (201, 'created')
            assert sql('SELECT count(*) FROM app_records.entries') == '3'
            pg_stop()
            assert request(key, payload) == (503, 'unavailable')
            pg_start('off')
            assert request(key, payload) == (503, 'unavailable')
            pg_stop()
            pg_start()
            assert request(key, '{"label":"isolated app"}') == (200, 'replayed')
            # Storage is logged, a guest has no read API, and no success stub was used.
            assert request('/records') == (404, 'not found')
            # Persistent namespace quotas preserve existing rows/replay and do
            # not silently delete data or reset when the worker restarts.
            sql("INSERT INTO app_records.entries(app_id,collection,id,payload) SELECT 'fedcba9876543210fedcba9876543210','notes', 'bbbbbbbb-1234-4234-8234-' || lpad(n::text,12,'0'), '{\"label\":\"quota fixture\",\"note\":null}'::jsonb FROM generate_series(1,20) n")
            fresh='/append/dddddddd-1234-4234-8234-123456789abc'
            assert request(fresh, '{"label":"over rate"}') == (503, 'unavailable')
            worker_stop();worker_start()
            assert request(fresh, '{"label":"over rate"}') == (503, 'unavailable')
            assert request(key, '{"label":"isolated app"}') == (200, 'replayed')
            worker_stop();grant['app_id']='a'*32;worker_start()
            sql("INSERT INTO app_records.entries(app_id,collection,id,payload,created_at) SELECT repeat('a',32),'notes', '00000000-0000-4000-8000-' || lpad(n::text,12,'0'), '{\"label\":\"quota fixture\",\"note\":null}'::jsonb, CURRENT_TIMESTAMP-interval '2 minutes' FROM generate_series(1,10000) n")
            assert request(fresh, '{"label":"over storage"}') == (503, 'unavailable')
            assert request('/append/00000000-0000-4000-8000-000000000001', '{"label":"quota fixture"}') == (200, 'replayed')
            # A PostgreSQL-aware backup restores all scoped rows and timestamps;
            # copying a live data directory is deliberately not used.
            backup=root/'records.dump'
            run([pg_bin/'pg_dump','-h',sockets,'-U',user,'-d','records_proof','-Fc','-f',backup])
            run([pg_bin/'createdb','-h',sockets,'-U',user,'records_restore'])
            run([pg_bin/'pg_restore','-h',sockets,'-U',user,'-d','records_restore','--no-owner','--no-privileges',backup])
            digest_sql="SELECT md5(string_agg(app_id || ':' || collection || ':' || id || ':' || payload::text || ':' || created_at::text, E'\\n' ORDER BY app_id,collection,id)) FROM app_records.entries"
            restored=run([pg_bin/'psql','-X','-At','-h',sockets,'-U',user,'-d','records_restore','-c',digest_sql])
            assert restored == sql(digest_sql)
            return {'postgres': run([pg_bin/'postgres', '--version']),
                    'guest_to_postgres': True, 'exact_retry': True, 'conflict': True,
                    'host_validation': True, 'ungranted_import_denied': True,
                    'guest_files_environment_network_denied': True,
                    'worker_and_database_crash_restart': True, 'app_isolation': True,
                    'database_failure': True, 'unsafe_durability_refused': True,
                    'lost_http_acknowledgement': True, 'lock_timeout': True,
                    'persistent_rate_and_storage_limits': True, 'backup_restore_exact_rows': True}
        finally:
            worker_stop()
            if pg_started:
                pg_stop()
            log.close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pg-bin', required=True, type=Path)
    parser.add_argument('--worker', required=True, type=Path)
    parser.add_argument('--component', required=True, type=Path)
    args = parser.parse_args()
    print(json.dumps(check(args.pg_bin.resolve(), args.worker.resolve(), args.component.resolve()), indent=2))
