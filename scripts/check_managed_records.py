#!/usr/bin/env python3
"""Real broker/systemd/peer-PostgreSQL acceptance on a fresh disposable Linux runner.

Refuses an existing deployment installation and reserved identities/units. Uses
no passwords, remote database, owner enrollment, public ingress or private app.
"""
import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
CONFIG = Path('/etc/devoption/deployment.toml')
BROKER = Path('/usr/local/libexec/devoption-deployment-broker')
WORKER = Path('/usr/local/libexec/devoption-app-worker')
FIXTURE = Path('/var/lib/devoption-records-proof')
APP_USER, DB_USER = 'dorecordapp', 'dorecorddb'
DB_UNIT = 'devoption-records-proof-db.service'


def run(*args, check=True, **kw):
    return subprocess.run([str(x) for x in args], check=check, capture_output=True,
                          text=True, timeout=60, **kw).stdout.strip()


def check(pg_bin, worker, component, stateless_component, https_component=None):
    if sys.platform != 'linux' or os.geteuid() == 0:
        raise ValueError('Requires unprivileged user on a fresh disposable Linux systemd runner')
    if any(p.exists() or p.is_symlink() for p in (CONFIG, BROKER, WORKER, FIXTURE)):
        raise ValueError('Existing deployment installation/fixture; nothing changed')
    for unit in ('devoption-app.service', 'devoption-app-validate.service', DB_UNIT):
        if run('systemctl', 'show', unit, '-p', 'LoadState', '--value') != 'not-found':
            raise ValueError('Reserved fixture unit already exists')
    for user in (APP_USER, DB_USER):
        if subprocess.run(['getent', 'passwd', user], capture_output=True).returncode == 0:
            raise ValueError('Reserved fixture identity already exists')
    installed, users = [], []
    config_parent_existed = CONFIG.parent.exists()
    with tempfile.TemporaryDirectory(prefix='devoption-records-input-') as temp:
        source = Path(temp)
        fixture_created = False
        try:
            for user in (APP_USER, DB_USER):
                run('sudo', '-n', 'useradd', '--system', '--no-create-home', '--user-group',
                    '--shell', '/usr/sbin/nologin', user)
                users.append(user)
            run('sudo', '-n', 'install', '-d', '-m755', FIXTURE)
            fixture_created = True
            for name, owner, mode in [('data',DB_USER,'700'), ('socket',DB_USER,'755'),
                                      ('state','root','700'), ('jobs',str(os.geteuid()),'700'),
                                      ('owner','root','700'), ('history','root','700'),
                                      ('repositories','root','700')]:
                run('sudo','-n','install','-d','-m'+mode,'-o',owner,FIXTURE/name)
            run('sudo','-n','-u',DB_USER,pg_bin/'initdb','-D',FIXTURE/'data','-A','peer','--no-locale','-E','UTF8')
            run('sudo','-n','systemd-run','--quiet','--collect','--unit='+DB_UNIT,
                '--property=User='+DB_USER,pg_bin/'postgres','-D',FIXTURE/'data',
                '-k',FIXTURE/'socket','-h','','-p','5432')
            deadline=time.monotonic()+10
            while subprocess.run(['sudo','-n','-u',DB_USER,str(pg_bin/'pg_isready'),'-h',str(FIXTURE/'socket')],capture_output=True).returncode:
                if time.monotonic()>deadline: raise TimeoutError('database readiness')
                time.sleep(.1)
            def sql(text):
                return run('sudo','-n','-u',DB_USER,pg_bin/'psql','-X','-At','-h',FIXTURE/'socket',
                           '-d','records_proof','-v','ON_ERROR_STOP=1','-c',text)
            run('sudo','-n','-u',DB_USER,pg_bin/'createdb','-h',FIXTURE/'socket','records_proof')
            sql((ROOT/'crates/devoption-runtime/src/records.sql').read_text())
            sql(f"CREATE ROLE {APP_USER} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION CONNECTION LIMIT 2; GRANT USAGE ON SCHEMA app_records TO {APP_USER}; GRANT SELECT, INSERT ON app_records.entries TO {APP_USER}")
            grant={'app_id':'c'*32,'collection':'notes','socket_dir':str(FIXTURE/'socket'),
                   'database':'records_proof','user':APP_USER,
                   'fields':{'label':{'required':True,'max_chars':100},'note':{'required':False,'max_chars':1000,'multiline':True}}}
            grant_source=source/'grant.json';grant_source.write_text(json.dumps(grant))
            grant_target=FIXTURE/'grant.json'
            run('sudo','-n','install','-m600',grant_source,grant_target)
            # Synthetic *build receipt*, real compiled component. This bypasses no
            # runtime/broker check; compiler execution has its own fixtures.
            artifact=source/'component.wasm';shutil.copyfile(component,artifact)
            raw=artifact.read_bytes()
            job='a'*32
            staging=FIXTURE/'jobs'/('b'*32)/'artifact'
            run('sudo','-n','install','-d','-m700','-o',str(os.geteuid()),staging.parent,staging)
            receipt={'version':2,'job':job,'project':'c'*32,'repository':'d'*32,'commit':'e'*40,
                     'source_sha256':'1'*64,'review_sha256':'2'*64,
                     'artifact_sha256':hashlib.sha256(raw).hexdigest(),'artifact_bytes':len(raw),'artifact':'component.wasm',
                     'toolchain_sha256':'3'*64,'dependency_sha256':'4'*64,'configuration_sha256':'5'*64,
                     'build_policy':'WasmBuildV1','target':'wasm32-wasip2','abi':'wasi:http/proxy@0.2.4'}
            receipt_source=source/'receipt.toml';receipt_source.write_text(''.join(f'{k}={json.dumps(v)}\n' for k,v in receipt.items()))
            for src,name in ((artifact,'component.wasm'),(receipt_source,'receipt.toml')):
                run('sudo','-n','install','-m600','-o',str(os.geteuid()),src,staging/name)
            values={'uid':os.geteuid(),'build_work':str(FIXTURE/'jobs'),'state':str(FIXTURE/'state'),
                    'worker':str(WORKER),'port':18983,'owner_data':str(FIXTURE/'owner'),
                    'history':str(FIXTURE/'history'),'repositories':str(FIXTURE/'repositories')}
            config_source=source/'deployment.toml'
            base=''.join(f'{k}={json.dumps(v)}\n' for k,v in values.items())
            config_source.write_text(base)
            run('sudo','-n','install','-d','-m755',CONFIG.parent,BROKER.parent)
            for src,dest,mode in ((ROOT/'scripts/deployment_broker.py',BROKER,'755'),(worker,WORKER,'755'),(config_source,CONFIG,'644')):
                run('sudo','-n','install','-m'+mode,src,dest);installed.append(dest)
            def broker(*args, delivery=None):
                return json.loads(run('sudo','-n','env','SUDO_UID='+str(os.geteuid()),BROKER,*args,input=json.dumps(delivery) if delivery is not None else None))
            def promote():
                return broker('promote',job,receipt['artifact_sha256'],receipt['project'],receipt['repository'],
                              receipt['commit'],receipt['source_sha256'],str(receipt['artifact_bytes']))['release']
            def stage(path):
                shutil.copyfile(path,artifact)
                encoded=artifact.read_bytes()
                receipt['artifact_sha256']=hashlib.sha256(encoded).hexdigest()
                receipt['artifact_bytes']=len(encoded)
                receipt_source.write_text(''.join(f'{k}={json.dumps(v)}\n' for k,v in receipt.items()))
                for src,name in ((artifact,'component.wasm'),(receipt_source,'receipt.toml')):
                    run('sudo','-n','install','-m600','-o',str(os.geteuid()),src,staging/name)
            ordinary=promote()
            stage(stateless_component)
            saved_stateless=promote()
            stage(component)
            config_source.write_text(base+'\n[records]\ngrant_file='+json.dumps(str(grant_target))+'\n')
            run('sudo','-n','install','-m644',config_source,CONFIG)
            release=promote()
            assert release != ordinary
            status=broker('status')
            broker('start',release,'ok records',status['fingerprint'],status['generation'])
            assert broker('status')['deployment']['observed']=='healthy'
            def request(path,body=None):
                connection=http.client.HTTPConnection('127.0.0.1',18983,timeout=5)
                try:
                    connection.request('POST' if body else 'GET',path,body=body)
                    response=connection.getresponse();return response.status,response.read().decode()
                finally:connection.close()
            assert request('/denials')==(200,'files environment sockets DNS outgoing HTTP denied')
            key='/append/12345678-1234-4234-8234-123456789abc'
            assert request(key,'{"label":"managed proof"}')==(201,'created')
            assert request(key,'{"label":"managed proof"}')==(200,'replayed')
            assert sql('SELECT count(*) FROM app_records.entries')=='1'
            assert sql(f"SELECT count(*) FROM information_schema.role_table_grants WHERE grantee='{APP_USER}' AND privilege_type IN ('UPDATE','DELETE')")=='0'
            if https_component is not None:
                policy={'project':receipt['project'],'environment':'preview','secret':'SERVICE_API_KEY','revision':1,'endpoint':'https://example.com/events'}
                packet={'policy':policy,'value':'synthetic-managed-https-proof-token'}
                https_text='\n[https]\n'+''.join(f'{k}={json.dumps(v)}\n' for k,v in policy.items())
                records_text='\n[records]\ngrant_file='+json.dumps(str(grant_target))+'\n'
                def configure(text):
                    config_source.write_text(text);run('sudo','-n','install','-m644',config_source,CONFIG)
                configure(base+records_text+https_text)
                with_https=promote();assert with_https != release
                current=broker('status')
                broker('start',with_https,'ok records',current['fingerprint'],current['generation'],delivery=packet)
                assert broker('status')['deployment']['observed']=='healthy'
                assert request(key,'{"label":"managed proof"}')==(200,'replayed')
                assert packet['value'] not in json.dumps(broker('status'))
                argv=run('sudo','-n','systemctl','show','devoption-app.service','-p','ExecStart')
                assert 'capability-worker' in argv and packet['value'] not in argv
                credential=Path('/run/credentials/devoption-app.service/https.json')
                copied=json.loads(run('sudo','-n','cat',credential))
                assert copied['policy']==policy and copied['value']==packet['value'] and copied['addresses']
                # Credential source is gone, so a raw systemctl restart cannot
                # recover it; the controller must resolve the exact revision.
                source_path=run('sudo','-n','systemctl','show','devoption-app.service','-p','LoadCredential','--value').split(':',1)[1].strip()
                assert run('sudo','-n','test','!','-e',source_path)==''
                broker('reconcile',delivery=packet)
                assert broker('status')['deployment']['observed']=='healthy'
                broker('reconcile')
                assert broker('status')['deployment']['observed']=='recovery failed'
                configure(base+https_text)
                stage(https_component)
                https_only=promote();current=broker('status')
                broker('start',https_only,'ok https',current['fingerprint'],current['generation'],delivery=packet)
                assert request('/isolation')==(200,'guest isolated')
                # The fixture requests a different test hostname, so no request
                # reaches example.com. DNS resolution above is the sole external IO.
                assert request('/wrong-host')==(200,'denied')
                assert packet['value'] not in run('sudo','-n','systemctl','show','devoption-app.service','-p','ExecStart')
                configure(base+records_text)
                stage(component)
                current=broker('status')
                broker('start',release,'ok records',current['fingerprint'],current['generation'])
            broker('reconcile')
            assert broker('status')['deployment']['observed']=='healthy'
            assert request(key,'{"label":"managed proof"}')==(200,'replayed')
            # Changed grant cannot regain access through an old approval/restart.
            grant['fields']['label']['max_chars']=99
            grant_source.write_text(json.dumps(grant));run('sudo','-n','install','-m600',grant_source,grant_target)
            assert broker('status')['fingerprint'] != status['fingerprint']
            failed=subprocess.run(['sudo','-n','env','SUDO_UID='+str(os.geteuid()),str(BROKER),'start',release,'ok records',status['fingerprint'],broker('status')['generation']],capture_output=True)
            assert failed.returncode != 0
            broker('reconcile')
            assert broker('status')['deployment']['observed']=='recovery failed'
            # The saved stateless promotion never gains the records import,
            # even though it contains the identical record-importing component.
            current=broker('status')
            failed=subprocess.run(['sudo','-n','env','SUDO_UID='+str(os.geteuid()),str(BROKER),'start',ordinary,'ok records',current['fingerprint'],current['generation']],capture_output=True)
            assert failed.returncode != 0
            current=broker('status')
            broker('start',saved_stateless,'ok A',current['fingerprint'],current['generation'])
            assert broker('status')['deployment']['observed']=='healthy'
            assert request('/healthz')==(200,'ok A')
            return {'managed_peer_postgres_write':True,'exact_retry':True,'sandbox_denials':True,
                    'worker_recovery':True,'grant_digest_revocation':True,'stateless_import_denied':True,
                    'return_to_saved_stateless':True,'managed_https_credentials':https_component is not None,
                    'database_row_retained':sql('SELECT count(*) FROM app_records.entries')=='1'}
        finally:
            for unit in ('devoption-app.service','devoption-app-validate.service',DB_UNIT):
                run('sudo','-n','systemctl','stop',unit,check=False)
                run('sudo','-n','systemctl','reset-failed',unit,check=False)
            for path in reversed(installed):run('sudo','-n','rm','--',path)
            if fixture_created:run('sudo','-n','rm','-r','--',FIXTURE)
            for user in reversed(users):run('sudo','-n','userdel',user)
            if not config_parent_existed:run('sudo','-n','rmdir',CONFIG.parent,check=False)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--disposable-system-host',action='store_true',required=True)
    parser.add_argument('--pg-bin',type=Path,required=True)
    parser.add_argument('--worker',type=Path,required=True)
    parser.add_argument('--component',type=Path,required=True)
    parser.add_argument('--stateless-component',type=Path,required=True)
    parser.add_argument('--https-component',type=Path)
    args=parser.parse_args()
    print(json.dumps(check(args.pg_bin.resolve(),args.worker.resolve(),args.component.resolve(),args.stateless_component.resolve(),args.https_component.resolve() if args.https_component else None),indent=2))
