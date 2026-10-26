#!/usr/bin/env python3
"""Real guest → host → disposable HTTPS proof using only synthetic credentials.

Custom CA/loopback transport injection exists only in the Rust test binary;
production credential loading rejects it. Does not install trust or call a provider.
"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import ssl
import socket
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
TOKEN = 'synthetic-https-proof-token'


def check(component, release=False):
    records, failures = [], []
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass
        def do_POST(self):
            try:
                if self.path != '/events' or self.headers.get('Authorization') != 'Bearer '+TOKEN or self.headers.get('Idempotency-Key') != 'fixture-operation-001':
                    raise AssertionError('fixed endpoint/credential/retry header mismatch')
                event = json.loads(self.rfile.read(int(self.headers['Content-Length'])))['event']
                records.append(event)
                body = b'{"accepted":true}'
                status = 200
                if event == '/redirect': status = 302
                if event == '/large': body = b'x'*65537
                if event == '/echo': body = TOKEN.encode()
                if event == '/slow': time.sleep(3.3)
                if event == '/disconnect':
                    self.connection.shutdown(socket.SHUT_RDWR);self.connection.close();return
                self.send_response(status)
                if status == 302: self.send_header('Location','https://other.example.test/should-not-follow')
                if event == '/headers': self.send_header('X-Fixture-Large','x'*17000)
                self.send_header('Content-Length',str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError, ssl.SSLError):
                pass
            except Exception:
                failures.append('unexpected HTTPS fixture request')
    with tempfile.TemporaryDirectory(prefix='devoption-https-') as tmp:
        root=Path(tmp)
        cert, key, ca=root/'certificate.pem',root/'key.pem',root/'ca.pem'
        def openssl(*args): subprocess.run(['openssl',*map(str,args)],check=True,capture_output=True,timeout=30)
        openssl('req','-x509','-newkey','rsa:2048','-nodes','-days','1','-keyout',root/'ca.key','-out',ca,'-subj','/CN=Disposable HTTPS proof CA','-addext','basicConstraints=critical,CA:TRUE')
        openssl('req','-new','-newkey','rsa:2048','-nodes','-keyout',key,'-out',root/'leaf.csr','-subj','/CN=api.example.test')
        (root/'leaf.ext').write_text('basicConstraints=critical,CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:api.example.test\n')
        openssl('x509','-req','-in',root/'leaf.csr','-CA',ca,'-CAkey',root/'ca.key','-CAcreateserial','-days','1','-extfile',root/'leaf.ext','-out',cert)
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.load_cert_chain(cert,key)
        server.socket=context.wrap_socket(server.socket,server_side=True)
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        try:
            env=os.environ | {'HTTPS_PROXY':'http://127.0.0.1:1','HTTP_PROXY':'http://127.0.0.1:1','ALL_PROXY':'http://127.0.0.1:1','NO_PROXY':'','DEVOPTION_HTTPS_PROOF_COMPONENT':str(component.resolve()),'DEVOPTION_HTTPS_PROOF_CA':str(ca),'DEVOPTION_HTTPS_PROOF_ADDRESS':f'127.0.0.1:{server.server_port}'}
            subprocess.run(['cargo','test','-p','devoption-runtime','--lib','--locked','--offline',*(['--release'] if release else []),'manual_https_component','--','--ignored','--nocapture'],cwd=ROOT,env=env,check=True,timeout=600)
            if failures or sorted(records) != sorted(['/send','/twice','/redirect','/large','/echo','/slow','/headers','/disconnect']):
                raise AssertionError('unexpected number/scope of HTTPS requests')
        finally:
            server.shutdown();server.server_close();thread.join(timeout=2)
    print('Real WASIp2 HTTPS, TLS refusal, endpoint/credential isolation, redirects, size, deadline and one-call bounds passed; no provider contacted.')


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--component',type=Path,required=True)
    parser.add_argument('--release',action='store_true')
    args=parser.parse_args()
    check(args.component,args.release)
