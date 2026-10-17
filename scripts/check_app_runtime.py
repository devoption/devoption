#!/usr/bin/env python3
"""Exercise an explicitly started disposable worker; never starts a workload.

Usage: check_app_runtime.py PORT --fixture A|B|probes
Only loopback is accepted. Probe routes are supplied by fixtures/runtime-probes.rs.
"""
import argparse
import concurrent.futures
import http.client
import socket
import time


def request(port, path, method='GET', headers=None):
    connection = http.client.HTTPConnection('127.0.0.1', port, timeout=8)
    try:
        connection.request(method, path, headers=headers or {})
        response = connection.getresponse()
        return response.status, response.read(1024*1024+1)
    finally: connection.close()


def raw(port, data, expected):
    with socket.create_connection(('127.0.0.1',port),timeout=8) as sock:
        sock.sendall(data)
        status = sock.recv(1024).split(b'\r\n')[0]
        assert status.startswith(f'HTTP/1.1 {expected}'.encode()), status


def check(port, fixture):
    if fixture != 'probes':
        assert request(port,'/healthz') == (200, f'ok {fixture}'.encode())
        code,body = request(port,'/')
        assert code == 200 and f'DevOption app revision {fixture}'.encode() in body
        assert request(port,'/missing')[0] == 404
        assert request(port,'/','POST')[0] == 405
    else:
        for path in ['/capabilities','/outbound','/headers']:
            code,body=request(port,path,headers={'Cookie':'private=hidden','Authorization':'Bearer hidden','X-DevOption-Probe':'hidden'})
            assert code == 200,(path,code,body)
        for path in ['/loop','/memory','/overflow','/stdout']:
            assert request(port,path)[0] == 502,path
            assert request(port,'/healthz') == (200,b'ok probes')
        with concurrent.futures.ThreadPoolExecutor() as executor:
            sleeping=executor.submit(request,port,'/sleep')
            time.sleep(.3)
            assert request(port,'/healthz')[0] == 503
            assert sleeping.result()[0] == 504
        assert request(port,'/healthz') == (200,b'ok probes')
    raw(port,b'GET / HTTP/1.1\r\nHost: app\r\nX-Large: '+b'x'*17000+b'\r\n\r\n',431)
    raw(port,b'POST / HTTP/1.1\r\nHost: app\r\nContent-Length: 65537\r\n\r\n',413)
    raw(port,b'POST / HTTP/1.1\r\nHost: app\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n',400)
    raw(port,b'GET / HTTP/1.1\r\nHost: app\r\n',408)
    print(f'{fixture}: HTTP, input bounds and selected runtime checks passed')


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('port',type=int)
    parser.add_argument('--fixture',required=True,choices=['A','B','probes'])
    args=parser.parse_args()
    if not 1024 <= args.port <= 65535: parser.error('use an unprivileged loopback port')
    check(args.port,args.fixture)
