"""Disposable real WASIp2/PostgreSQL/process-restart/HTTPS delivery proof.

No host service, public listener, provider, real recipient, installed trust or
production secret. Custom TLS/resolution exists only in the ignored test binary.
"""
import argparse
import getpass
import hashlib
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import socket
import sqlite3
import ssl
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
TOKEN = 'synthetic-https-proof-token'


def check(pg_bin, worker, component, release=False):
    def run(args, **kw):
        return subprocess.run(list(map(str,args)),check=True,capture_output=True,text=True,timeout=600,**kw).stdout.strip()
    def wait(predicate, label, seconds=15):
        deadline=time.monotonic()+seconds
        while not predicate():
            if time.monotonic()>=deadline:
                raise TimeoutError(label)
            time.sleep(.05)
    # Launch the actual test executable directly, so kill/TERM target the
    # dispatcher process, not Cargo's parent process or an unrelated process.
    built=run(['cargo','test','-p','devoption-runtime','--lib','--locked','--offline',
        *(['--release'] if release else []),'--no-run','--message-format=json'],cwd=ROOT)
    test_binary=next(r['executable'] for line in built.splitlines()
        if (r:=json.loads(line)).get('reason')=='compiler-artifact'
        and r['target']['name']=='devoption_runtime' and r.get('executable'))
    with tempfile.TemporaryDirectory(prefix='devoption-jobs-',dir='/tmp') as temporary:
        root=Path(temporary);root.chmod(0o700)
        data,sockets=root/'data',root/'socket';sockets.mkdir(mode=0o700)
        user=getpass.getuser();pg_started=False;app=None;dispatcher=None;server=None
        log=(root/'process.log').open('w+')
        receiver=root/'receiver.sqlite'
        with sqlite3.connect(receiver) as db:
            db.execute('CREATE TABLE effects (operation TEXT PRIMARY KEY,digest TEXT NOT NULL,response TEXT NOT NULL)')
        committed=threading.Event();requests=[];failures=[]
        class Handler(BaseHTTPRequestHandler):
            def log_message(self,*_): pass
            def do_POST(self):
                try:
                    assert self.path=='/events' and self.headers.get('Authorization')=='Bearer '+TOKEN
                    key=self.headers['Idempotency-Key'];assert len(key)==69 and key[32]==':'
                    raw=self.rfile.read(int(self.headers['Content-Length']));event=json.loads(raw)
                    requests.append((key,event.get('mode')))
                    status=400 if event.get('mode')=='reject' else 500 if event.get('mode')=='retry' else 200
                    fresh=False;body=b'{"accepted":true}'
                    if status==200:
                        digest=hashlib.sha256(json.dumps(event,sort_keys=True,separators=(',',':')).encode()).hexdigest()
                        with sqlite3.connect(receiver) as db:
                            db.execute('BEGIN IMMEDIATE')
                            row=db.execute('SELECT digest,response FROM effects WHERE operation=?',(key,)).fetchone()
                            if row is None:
                                db.execute('INSERT INTO effects VALUES (?,?,?)',(key,digest,body.decode()));fresh=True
                            elif row[0]!=digest: status=409
                            else:body=row[1].encode()
                        if fresh and event.get('mode') in ('lost','cancel'):
                            committed.set();time.sleep(4)
                    self.send_response(status);self.send_header('Content-Length',str(len(body)))
                    self.end_headers();self.wfile.write(body)
                except (BrokenPipeError,ConnectionResetError,ssl.SSLError): pass
                except Exception: failures.append('unexpected receiver request')
        def pg_start(fsync='on'):
            nonlocal pg_started
            run([pg_bin/'pg_ctl','-D',data,'-l',root/'postgres.log','-o',f"-k {sockets} -h '' -p 5432 -c fsync={fsync}",'-w','start']);pg_started=True
        def pg_stop():
            nonlocal pg_started
            run([pg_bin/'pg_ctl','-D',data,'-w','stop','-m','immediate']);pg_started=False
        def sql(text,database='jobs_proof'):
            return run([pg_bin/'psql','-X','-At','-h',sockets,'-U',user,'-d',database,'-v','ON_ERROR_STOP=1','-c',text])
        grant={'app_id':'a'*32,'collection':'notes','socket_dir':str(sockets),'database':'jobs_proof','user':user,
            'fields':{'label':{'required':True,'max_chars':100}},
            'jobs':{'project':'a'*32,'environment':'preview','secret':'SERVICE_API_KEY','revision':1,
                    'endpoint':'https://api.example.test/events','receiver_dedup_seconds':172800}}
        grant_path=root/'grant.json';dispatch_path=root/'dispatcher.json';credential_path=root/'credential.json'
        def save():
            grant_path.write_text(json.dumps(grant));grant_path.chmod(0o600)
            dispatch_path.write_text(json.dumps(grant|{'user':'app_dispatch'}));dispatch_path.chmod(0o600)
            credential_path.write_text(json.dumps({'policy':{k:v for k,v in grant['jobs'].items() if k!='receiver_dedup_seconds'},'addresses':['1.1.1.1'],'value':TOKEN}));credential_path.chmod(0o600)
        with socket.socket() as s:
            s.bind(('127.0.0.1',0));port=s.getsockname()[1]
        def request(path,body=None):
            c=http.client.HTTPConnection('127.0.0.1',port,timeout=6)
            try:
                c.request('GET' if body is None else 'POST',path,body=body)
                r=c.getresponse();return r.status,r.read(32769).decode()
            finally:c.close()
        def app_start():
            nonlocal app
            save();app=subprocess.Popen([str(worker),'records-worker',str(component),str(port),'serve',str(grant_path)],stdout=log,stderr=log)
            def ready():
                if app.poll() is not None:raise RuntimeError('job app exited')
                try:return request('/healthz')==(200,'ok records')
                except OSError:return False
            wait(ready,'app readiness',30)
        def stop(child):
            if child is not None and child.poll() is None:
                child.terminate()
                try:child.wait(timeout=12)
                except subprocess.TimeoutExpired:child.kill();child.wait(timeout=5)
        def start_dispatcher():
            nonlocal dispatcher
            env=os.environ|{'HTTPS_PROXY':'http://127.0.0.1:1','HTTP_PROXY':'http://127.0.0.1:1','ALL_PROXY':'http://127.0.0.1:1','NO_PROXY':'',
                'DEVOPTION_JOB_PROOF_GRANT':str(dispatch_path),'DEVOPTION_JOB_PROOF_CREDENTIAL':str(credential_path),
                'DEVOPTION_JOB_PROOF_CA':str(root/'ca.pem'),'DEVOPTION_JOB_PROOF_ADDRESS':f'127.0.0.1:{server.server_port}'}
            dispatcher=subprocess.Popen([test_binary,'--ignored','--exact','jobs::tests::manual_delivery_worker','--nocapture'],env=env,stdout=log,stderr=log)
        def state(id,database='jobs_proof'):
            return sql(f"SELECT state FROM app_jobs.deliveries WHERE app_id='{grant['app_id']}' AND id='{id}'",database)
        def status():
            return json.loads(run([worker,'jobs',dispatch_path,'status']))
        try:
            run([pg_bin/'initdb','-D',data,'-A','peer','--no-locale','-E','UTF8'])
            # Test-only peer map lets the existing test OS identity exercise two
            # distinct least-privilege DB roles; no OS accounts/passwords/services.
            (data/'pg_ident.conf').write_text(f'jobs_fixture {user} {user}\njobs_fixture {user} app_enqueue\njobs_fixture {user} app_dispatch\n')
            (data/'pg_hba.conf').write_text('local all all peer map=jobs_fixture\n')
            pg_start()
            run([pg_bin/'createdb','-h',sockets,'-U',user,'jobs_proof'])
            sql((ROOT/'crates/devoption-runtime/src/jobs.sql').read_text())
            sql((ROOT/'crates/devoption-runtime/src/records.sql').read_text())
            sql("CREATE ROLE app_enqueue LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE CONNECTION LIMIT 2; CREATE ROLE app_dispatch LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE CONNECTION LIMIT 2; GRANT USAGE ON SCHEMA app_jobs TO app_enqueue,app_dispatch; GRANT SELECT,INSERT ON app_jobs.deliveries TO app_enqueue; GRANT SELECT,UPDATE(state,attempts,generation,lease_until,due_at,outcome) ON app_jobs.deliveries TO app_dispatch")
            grant['user']='app_enqueue'
            for role,query in [('app_enqueue',"UPDATE app_jobs.deliveries SET state='succeeded'"),('app_dispatch',"INSERT INTO app_jobs.deliveries(app_id,id,policy,payload,delay_seconds,due_at,state) VALUES('x','x','x','{}',0,now(),'queued')"),('app_dispatch',"UPDATE app_jobs.deliveries SET payload='{}'")]:
                denied=subprocess.run([str(pg_bin/'psql'),'-X','-h',str(sockets),'-U',role,'-d','jobs_proof','-v','ON_ERROR_STOP=1','-c',query],capture_output=True)
                assert denied.returncode!=0 and b'permission denied' in denied.stderr
            def openssl(*args):run(['openssl',*args])
            openssl('req','-x509','-newkey','rsa:2048','-nodes','-days','1','-keyout',root/'ca.key','-out',root/'ca.pem','-subj','/CN=Disposable jobs proof CA','-addext','basicConstraints=critical,CA:TRUE')
            openssl('req','-new','-newkey','rsa:2048','-nodes','-keyout',root/'leaf.key','-out',root/'leaf.csr','-subj','/CN=api.example.test')
            (root/'leaf.ext').write_text('basicConstraints=critical,CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:api.example.test\n')
            openssl('x509','-req','-in',root/'leaf.csr','-CA',root/'ca.pem','-CAkey',root/'ca.key','-CAcreateserial','-days','1','-extfile',root/'leaf.ext','-out',root/'leaf.pem')
            server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
            tls=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);tls.load_cert_chain(root/'leaf.pem',root/'leaf.key')
            server.socket=tls.wrap_socket(server.socket,server_side=True)
            thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
            save()
            disabled=root/'disabled.json';disabled.write_text(json.dumps({k:v for k,v in grant.items() if k!='jobs'}))
            assert subprocess.run([str(worker),'records-worker',str(component),str(port),'validate',str(disabled)],capture_output=True,timeout=15).returncode!=0
            # The production entry point has no custom CA/loopback option, and
            # rejects a credential whose exact revision differs before any claim.
            packet=json.loads(credential_path.read_text());packet['policy']['revision']=2;credential_path.write_text(json.dumps(packet))
            assert subprocess.run([str(worker),'job-worker',str(grant_path),str(credential_path)],capture_output=True,timeout=15).returncode!=0
            app_start();assert request('/denials')[0]==200
            id='12345678-1234-4234-8234-123456789abc';body='{"mode":"lost","label":"durable proof"}'
            assert request('/enqueue/'+id,body)==(201,'accepted')
            assert request('/enqueue/'+id,body)==(200,'replayed')
            assert request('/later/'+id,body)==(409,'conflict')
            assert request('/enqueue/'+id,'{"changed":true}')==(409,'conflict')
            for invalid in ('[]','{"a":1,"a":2}','{"a":{"b":1,"b":2}}','x'*24577):
                assert request('/enqueue/'+id,invalid)==(422,'invalid')
            stop(app);app_start();assert state(id)=='queued'
            backup=root/'queued.dump';run([pg_bin/'pg_dump','-h',sockets,'-U',user,'-d','jobs_proof','-Fc','-f',backup])
            start_dispatcher();assert committed.wait(15),'receiver did not commit'
            assert request('/healthz')==(200,'ok records')
            assert state(id)=='leased','HTTP handling waited for background completion'
            dispatcher.kill();dispatcher.wait(timeout=5)
            assert state(id)=='leased'
            sql(f"UPDATE app_jobs.deliveries SET lease_until=CURRENT_TIMESTAMP-interval '1 second' WHERE id='{id}'")
            pg_stop();pg_start();start_dispatcher()
            wait(lambda:state(id)=='succeeded','recovered delivery')
            with sqlite3.connect(receiver) as db:assert db.execute('SELECT count(*) FROM effects').fetchone()[0]==1
            later='22222222-1234-4234-8234-123456789abc'
            assert request('/later/'+later,'{}')==(201,'accepted');time.sleep(.4);assert state(later)=='queued'
            run([worker,'jobs',dispatch_path,'cancel',later]);assert state(later)=='cancelled'
            assert request('/later/'+later,'{}')==(200,'replayed')
            reject='33333333-1234-4234-8234-123456789abc';assert request('/enqueue/'+reject,'{"mode":"reject"}')[0]==201
            wait(lambda:state(reject)=='failed','terminal rejection')
            retry='44444444-1234-4234-8234-123456789abc';assert request('/enqueue/'+retry,'{"mode":"retry"}')[0]==201
            for attempt in range(1,6):
                wait(lambda:sql(f"SELECT state||':'||attempts FROM app_jobs.deliveries WHERE id='{retry}'") in (f'retry_wait:{attempt}',f'needs_review:{attempt}'),f'attempt {attempt}')
                assert sql(f"SELECT attempts FROM app_jobs.deliveries WHERE id='{retry}'")==str(attempt)
                if attempt<5:sql(f"UPDATE app_jobs.deliveries SET due_at=CURRENT_TIMESTAMP-interval '1 second' WHERE id='{retry}'")
            assert state(retry)=='needs_review'
            observed=status();assert observed['counts']['needs_review']==1 and TOKEN not in json.dumps(observed)
            stop(dispatcher);assert dispatcher.returncode==0,'graceful shutdown failed'
            # Restoring an older accepted queue must reuse its operation key.
            run([pg_bin/'createdb','-h',sockets,'-U',user,'jobs_restore'])
            run([pg_bin/'pg_restore','-h',sockets,'-U',user,'-d','jobs_restore','--no-owner','--no-privileges',backup])
            sql('GRANT USAGE ON SCHEMA app_jobs TO app_dispatch; GRANT SELECT,UPDATE(state,attempts,generation,lease_until,due_at,outcome) ON app_jobs.deliveries TO app_dispatch',database='jobs_restore')
            grant['database']='jobs_restore';save();start_dispatcher()
            wait(lambda:state(id,'jobs_restore')=='succeeded','restored delivery')
            stop(dispatcher)
            with sqlite3.connect(receiver) as db:assert db.execute('SELECT count(*) FROM effects').fetchone()[0]==1
            grant['database']='jobs_proof';save()
            # Cancellation after receiver acceptance remains explicitly ambiguous.
            committed.clear();start_dispatcher()
            cancel='77777777-1234-4234-8234-123456789abc'
            assert request('/enqueue/'+cancel,'{"mode":"cancel"}')[0]==201
            assert committed.wait(15)
            run([worker,'jobs',dispatch_path,'cancel',cancel])
            assert state(cancel)=='needs_review'
            stop(dispatcher)
            assert sql(f"SELECT outcome FROM app_jobs.deliveries WHERE id='{cancel}'")=='cancel_ambiguous'
            # A changed exact policy/secret revision blocks old pending work.
            pending='55555555-1234-4234-8234-123456789abc';assert request('/later/'+pending,'{}')[0]==201
            grant['jobs']['revision']=2;save();start_dispatcher()
            wait(lambda:state(pending)=='needs_review','changed policy refusal')
            packet=json.loads(credential_path.read_text());packet['value']='synthetic-rotated-token';credential_path.write_text(json.dumps(packet))
            wait(lambda:dispatcher.poll() is not None,'live credential change refusal')
            assert dispatcher.returncode!=0
            pg_stop()
            assert request('/enqueue/66666666-1234-4234-8234-123456789abc','{}')==(503,'unavailable')
            assert subprocess.run([str(worker),'jobs',str(grant_path),'status'],capture_output=True,timeout=5).returncode!=0
            pg_start('off')
            assert request('/enqueue/66666666-1234-4234-8234-123456789abc','{}')==(503,'unavailable')
            pg_stop();pg_start()
            # Separate namespace for actual lease fencing/retry-window assertions.
            grant['app_id']='d'*32;grant['jobs']['project']='d'*32;grant['user']=user;save()
            env=os.environ|{'DEVOPTION_JOB_PROOF_GRANT':str(grant_path)}
            run([test_binary,'--ignored','--exact','jobs::tests::manual_delivery_store','--nocapture'],env=env)
            assert not failures,failures
            print(json.dumps({'wasip2_durable_admission':True,'app_and_database_restart':True,
                'dispatcher_killed_after_receiver_commit':True,'single_receiver_side_effect':True,
                'backup_replay_deduplicated':True,'exact_retry_and_conflict':True,
                'scheduled_and_cancelled':True,'bounded_retry_and_visible_terminal_failure':True,
                'policy_revision_refusal':True,'unavailable_and_unsafe_database_refused':True,
                'lease_generation_fencing':True,'graceful_shutdown':True},indent=2))
        except Exception:
            log.flush();log.seek(max(0,log.tell()-12000));print(log.read(12000));raise
        finally:
            stop(dispatcher);stop(app)
            if server is not None:server.shutdown();server.server_close()
            if pg_started:pg_stop()
            log.close()


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('pg-bin','worker','component'):parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--release',action='store_true')
    args=parser.parse_args()
    check(args.pg_bin.resolve(),args.worker.resolve(),args.component.resolve(),args.release)
