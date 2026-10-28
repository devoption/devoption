"""Authenticated two-app lifecycle on an explicitly disposable Linux runner.

Uses only public source, synthetic owner credentials and a private test CA. Refuses
existing installations and reserved units; never run on a workstation/installation.
"""
import argparse
import http.client
import json
import os
from pathlib import Path
import re
import shutil
import socket
import ssl
import subprocess
import sys
import tempfile
import time

from check_bundle_installer import enroll
from check_public_proxy import fixture_clients
from configure_app_hostname import render_apps

ROOT = Path(__file__).resolve().parents[1]
STATE = Path('/var/lib/devoption-multi-proof')
CONFIG = Path('/etc/devoption/deployment.toml')
BUILD_CONFIG = Path('/etc/devoption/build.toml')
BROKER = Path('/usr/local/libexec/devoption-deployment-broker')
WORKER = Path('/usr/local/libexec/devoption-app-worker')
BUILD_BROKER = Path('/usr/local/libexec/devoption-build-broker')
BUILD_WORKER = Path('/usr/local/libexec/devoption-build-worker')
PROXY_UNIT = 'devoption-multi-proof-proxy.service'


def run(*args, timeout=180):
    result = subprocess.run([str(x) for x in args], capture_output=True, timeout=timeout)
    if result.returncode:
        raise ValueError('Fixture command failed: '+str(args[0])+' '+result.stderr.decode(errors='replace')[-4096:])
    return result.stdout


def check(binary, runtime, caddy, reboot_fixture=None):
    if sys.platform != 'linux' or os.geteuid() == 0:
        raise ValueError('Use an unprivileged user on a fresh disposable Linux systemd host')
    reserved = (STATE, CONFIG, BUILD_CONFIG, BROKER, WORKER, BUILD_BROKER, BUILD_WORKER)
    if any(p.exists() or p.is_symlink() for p in reserved):
        raise ValueError('Existing installation or fixture; nothing changed')
    for unit in (PROXY_UNIT, 'devoption-app.service', 'devoption-app-validate.service', 'devoption-build.service'):
        if run('systemctl', 'show', unit, '-p', 'LoadState', '--value').strip() != b'not-found':
            raise ValueError('Reserved unit exists; nothing changed')
    for port in (443, 15336, 15337, 15338):
        with socket.socket() as probe:
            if port != 443: probe.bind(('127.0.0.1', port))
            if port == 443 and probe.connect_ex(('127.0.0.1', port)) == 0:
                raise ValueError('Existing HTTPS listener; nothing changed')
    installed, projects, controller = [], [], None
    config_parent_existed = CONFIG.parent.exists()
    fixture_created = False
    with tempfile.TemporaryDirectory(prefix='devoption-multi-input-') as temporary:
        root = Path(temporary).resolve()
        for name in ('data', 'repositories', 'history', 'jobs'):
            (root/name).mkdir(mode=0o700)
        try:
            run('sudo', '-n', 'install', '-d', '-m755', STATE, CONFIG.parent, BROKER.parent)
            fixture_created = True
            run('sudo', '-n', 'install', '-d', '-m700', STATE/'state')
            for original, target, mode in ((ROOT/'scripts/deployment_broker.py', BROKER, '755'),
                    (runtime, WORKER, '755'), (ROOT/'scripts/build_broker.py', BUILD_BROKER, '755'),
                    (ROOT/'scripts/build_worker.py', BUILD_WORKER, '755')):
                run('sudo', '-n', 'install', '-m'+mode, original, target); installed.append(target)
            enroll(binary, root/'data')
            origin = 'https://platform.example.test'
            arguments = [str(binary), 'serve', '--port', '15336', '--data-dir', str(root/'data'),
                         '--repository-root', str(root/'repositories'), '--build-history-dir', str(root/'history'),
                         '--public-origin', origin]
            log = (root/'controller.log').open('wb')

            def start(enabled=False):
                nonlocal controller
                controller = subprocess.Popen(arguments + (['--build-execution', 'enabled', '--app-deployment', 'enabled'] if enabled else []), stdout=log, stderr=log)
                deadline = time.monotonic()+40
                while True:
                    if controller.poll() is not None:
                        raise ValueError('Controller failed: '+(root/'controller.log').read_text()[-4096:])
                    try:
                        if request('GET', '/sign-in')[0] == 200: break
                    except OSError: pass
                    if time.monotonic() > deadline: raise TimeoutError('controller readiness')
                    time.sleep(.1)

            cookie, csrf, tls = '', '', None
            def request(method, path, body='', target='platform.example.test', authenticated=True, content_type='text/plain'):
                fields = {'Host': target, 'Connection': 'close', 'Content-Length': str(len(body.encode()))}
                if method == 'POST': fields.update(Origin=origin, **{'Content-Type': content_type})
                if authenticated and cookie: fields.update(Cookie=cookie, **{'X-CSRF-Token': csrf})
                if tls:
                    with tls.wrap_socket(socket.create_connection(('127.0.0.1', 443), 5), server_hostname=target) as stream:
                        stream.settimeout(95)
                        stream.sendall((f'{method} {path} HTTP/1.1\r\n'+''.join(f'{k}: {v}\r\n' for k, v in fields.items())+'\r\n'+body).encode())
                        response = http.client.HTTPResponse(stream); response.begin()
                        return response.status, dict(response.getheaders()), response.read(1024*1024)
                connection = http.client.HTTPConnection('127.0.0.1', 15336, timeout=95)
                try:
                    connection.request(method, path, body, fields)
                    response = connection.getresponse()
                    return response.status, dict(response.getheaders()), response.read(1024*1024)
                finally: connection.close()

            def login():
                nonlocal cookie, csrf
                cookie, csrf = '', ''
                code, headers, _ = request('POST', '/auth/challenge')
                assert code == 204
                cookie, csrf = headers['Set-Cookie'].split(';', 1)[0], headers['X-Csrf-Token']
                code, headers, _ = request('POST', '/auth/sign-in', 'password=disposable-installer-fixture-only',
                                           content_type='application/x-www-form-urlencoded')
                assert code == 204
                cookie, csrf = headers['Set-Cookie'].split(';', 1)[0], headers['X-Csrf-Token']

            def mutate(path, body='', expected=204):
                nonlocal csrf
                code, headers, _ = request('POST', '/auth/session')
                assert code == 204
                csrf = headers['X-Csrf-Token']
                result = request('POST', path, body)
                if result[0] != expected:
                    raise ValueError(f'Action {path} returned {result[0]}: '+(root/'controller.log').read_text()[-4096:])

            start(); login()
            for index in range(2):
                before = set(re.findall(rb'href="/projects/([0-9a-f]{32})"', request('GET', '/projects')[2]))
                mutate('/projects', 'Multi app fixture '+str(index))
                after = set(re.findall(rb'href="/projects/([0-9a-f]{32})"', request('GET', '/projects')[2]))
                project, = after-before; project = project.decode()
                for suffix in ('', '-validate'):
                    unit = 'devoption-app-'+project+suffix+'.service'
                    assert run('systemctl', 'show', unit, '-p', 'LoadState', '--value').strip() == b'not-found'
                projects.append(project)
                run('sudo', '-n', 'install', '-d', '-m700', STATE/'state'/project)
                checkout = root/'repositories'/('app'+str(index))
                shutil.copytree(ROOT/'examples/static-site', checkout)
                path = checkout/'src/lib.rs'
                path.write_text(path.read_text().replace('"ok A"', '"ok '+str(index)+'"'))
                run('git', '-C', checkout, 'init', '-q')
                run('git', '-C', checkout, 'add', '.')
                run('git', '-C', checkout, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture source')
                mutate(f'/projects/{project}/repositories', checkout.name)
            controller.terminate(); controller.wait(timeout=15); controller = None
            source = root/'repositories/app0'
            run('cargo', 'vendor', '--locked', '--manifest-path', source/'Cargo.toml', root/'vendor')
            archives = list((Path(os.environ.get('CARGO_HOME', str(Path.home()/'.cargo')))/'registry/cache').glob('*'))
            assert len(archives) == 1
            toolchain = run('rustc', '--print', 'sysroot').decode().strip()
            build = run('python3', ROOT/'scripts/prepare_build_config.py', '--devoption', binary,
                        '--toolchain', toolchain, '--vendor', root/'vendor', '--lockfile', source/'Cargo.lock',
                        '--archives', archives[0], '--work', root/'jobs', '--owner-data', root/'data',
                        '--repositories', root/'repositories', '--history', root/'history')
            staged = root/'build.toml'; staged.write_bytes(build)
            run('sudo', '-n', 'install', '-m644', staged, BUILD_CONFIG); installed.append(BUILD_CONFIG)
            config = dict(uid=os.geteuid(), build_work=str(root/'jobs'), state=str(STATE/'state'), worker=str(WORKER),
                          owner_data=str(root/'data'), history=str(root/'history'), repositories=str(root/'repositories'))
            config['apps'] = [dict(project=project, port=15337+i, ingress=dict(project=project,
                             platform_hostname='platform.example.test', platform_port=15336,
                             app_hostname=f'app{i}.example.test', ca_file=str(STATE/'ca.pem')))
                             for i, project in enumerate(projects)]
            policy, clients = fixture_clients(root)
            proxy = render_apps(config, policy).replace('admin off', 'admin off\n    auto_https disable_redirects\n    skip_install_trust')
            proxy = proxy.replace('    tls {', '    tls {\n        issuer internal', 1)
            for name in ('platform', 'app0', 'app1'):
                proxy = proxy.replace(name+'.example.test {', name+'.example.test {\n    bind 127.0.0.1'+('' if name == 'platform' else '\n    tls internal'))
            (root/'Caddyfile').write_text(proxy)
            run('sudo', '-n', 'systemd-run', '--quiet', '--collect', '--unit='+PROXY_UNIT,
                '--setenv=XDG_DATA_HOME='+str(STATE/'caddy'), '--setenv=XDG_CONFIG_HOME='+str(STATE/'caddy-config'),
                caddy, 'run', '--config', root/'Caddyfile', '--adapter', 'caddyfile')
            deadline = time.monotonic()+15
            while True:
                result = subprocess.run(['sudo', '-n', 'cat', str(STATE/'caddy/caddy/pki/authorities/local/root.crt')], capture_output=True)
                if result.returncode == 0:
                    (root/'ca.pem').write_bytes(result.stdout); break
                if time.monotonic() > deadline: raise TimeoutError('test CA readiness')
                time.sleep(.1)
            run('sudo', '-n', 'install', '-m644', root/'ca.pem', STATE/'ca.pem')
            raw = ''.join(f'{k}={json.dumps(v)}\n' for k, v in config.items() if k != 'apps')
            for app in config['apps']:
                raw += f'\n[[apps]]\nproject="{app["project"]}"\nport={app["port"]}\n[apps.ingress]\n'
                raw += ''.join(f'{k}={json.dumps(v)}\n' for k, v in app['ingress'].items())
            (root/'deployment.toml').write_text(raw)
            run('sudo', '-n', 'install', '-m644', root/'deployment.toml', CONFIG); installed.append(CONFIG)
            tls = ssl.create_default_context(cafile=str(root/'ca.pem'))
            tls.load_cert_chain(*map(str, clients['allowed']))
            start(True); login()

            def snapshot(project):
                return json.loads(run('sudo', '-n', BROKER, 'project', project, 'status'))

            def deploy(project, release, health):
                mutate(f'/projects/{project}/releases/prepare', release+'\n'+health)
                page = request('GET', f'/projects/{project}/releases')[2]
                assert b'Project destination' in page and b'independent' in page
                token = re.search(rb'data-deploy-action="approve" data-value="([0-9a-f]+)"', page)[1].decode()
                mutate(f'/projects/{project}/releases/approve', token)

            def build(project):
                old_jobs = {r['job'] for r in snapshot(project)['releases']}
                page = request('GET', f'/projects/{project}/builds')[2]
                repository = re.search(rb'/builds/([0-9a-f]{32})', page)[1].decode()
                base = f'/projects/{project}/builds/{repository}'
                mutate(base+'/prepare')
                token = re.search(rb'data-build-action="approve" data-token="([0-9a-f]+)"', request('GET', base+'/run')[2])[1].decode()
                mutate(base+'/approve', token, 202)
                deadline = time.monotonic()+120
                while time.monotonic() < deadline:
                    page = request('GET', f'/projects/{project}/releases')[2]
                    jobs = re.findall(rb'data-deploy-action="promote" data-value="([0-9a-f]{32})"', page)
                    fresh = [job.decode() for job in jobs if job.decode() not in old_jobs]
                    if fresh:
                        mutate(f'/projects/{project}/releases/promote', fresh[0])
                        return next(r['release'] for r in snapshot(project)['releases'] if r['job'] == fresh[0])
                    time.sleep(.25)
                raise TimeoutError('authenticated build completion: '+(root/'controller.log').read_text()[-4096:])

            releases = []
            for i, project in enumerate(projects):
                releases.append(build(project)); deploy(project, releases[-1], 'ok '+str(i))
                assert snapshot(project)['deployment']['observed'] == 'healthy'
                assert snapshot(project)['ingress']['status'].startswith('Local HTTPS route verified')
            anonymous = ssl.create_default_context(cafile=str(root/'ca.pem'))
            authorized = tls
            for i in range(2):
                tls = anonymous
                assert request('GET', '/healthz', target=f'app{i}.example.test')[2] == ('ok '+str(i)).encode()
                assert request('GET', '/auth/session', target=f'app{i}.example.test')[0] == 404
            try:
                request('GET', '/sign-in')
            except (ssl.SSLError, ConnectionResetError): pass
            else: raise AssertionError('platform accepted missing client certificate')
            tls = authorized
            untouched = snapshot(projects[1])['deployment']
            # A new source revision goes through a real second build and review.
            source_file = source/'src/lib.rs'
            source_file.write_text(source_file.read_text().replace('"ok 0"', '"ok updated"'))
            run('git', '-C', source, 'add', '.')
            run('git', '-C', source, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture update')
            newer = build(projects[0]); assert newer != releases[0]
            deploy(projects[0], newer, 'ok updated')
            assert snapshot(projects[1])['deployment'] == untouched
            deploy(projects[0], releases[0], 'ok 0')
            assert snapshot(projects[1])['deployment'] == untouched
            mutate(f'/projects/{projects[0]}/releases/stop')
            assert snapshot(projects[1])['deployment'] == untouched
            controller.terminate(); controller.wait(timeout=15); controller = None
            start(True); login()
            assert snapshot(projects[0])['deployment']['intent'] == 'stopped'
            assert snapshot(projects[1])['deployment']['observed'] == 'healthy'
            deploy(projects[0], releases[0], 'ok 0')
            controller.terminate(); controller.wait(timeout=15); controller = None
            start(True); login()
            for project in projects:
                assert snapshot(project)['deployment']['observed'] == 'healthy'
                assert snapshot(project)['ingress']['status'].startswith('Local HTTPS route verified')
            if reboot_fixture is not None:
                # Export only this disposable fixture's owned files. No owner
                # store, source, compiler cache or real credentials are exported.
                reboot_fixture.mkdir(mode=0o700)
                (reboot_fixture/'snapshot.tar').write_bytes(run('sudo', '-n', 'tar', '-C', '/', '-cf', '-',
                    str(STATE).lstrip('/'), str(CONFIG).lstrip('/'), str(BROKER).lstrip('/'), str(WORKER).lstrip('/')))
                (reboot_fixture/'metadata.json').write_text(json.dumps({'uid': os.geteuid(), 'root': str(root),
                    'projects': projects, 'proxy': proxy, 'policy': policy}))
                shutil.copyfile(caddy, reboot_fixture/'caddy')
                for value in policy.values():
                    shutil.copyfile(value, reboot_fixture/Path(value).name)
            print('Two authenticated builds/releases/deployments, protected HTTPS origins, update/rollback/stop isolation and independent service recovery passed.')
        finally:
            if controller is not None:
                controller.terminate(); controller.wait(timeout=15)
            for project in projects:
                for suffix in ('', '-validate'):
                    subprocess.run(['sudo', '-n', 'systemctl', 'stop', 'devoption-app-'+project+suffix+'.service'], capture_output=True, timeout=15)
            for unit in (PROXY_UNIT, 'devoption-build.service'):
                subprocess.run(['sudo', '-n', 'systemctl', 'stop', unit], capture_output=True, timeout=15)
            for target in reversed(installed): run('sudo', '-n', 'rm', '--', target)
            if fixture_created: run('sudo', '-n', 'rm', '-rf', '--', STATE)
            if not config_parent_existed and CONFIG.parent.exists(): run('sudo', '-n', 'rmdir', CONFIG.parent)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--disposable-system-host', action='store_true', required=True)
    for name in ('binary', 'runtime', 'caddy'):
        parser.add_argument('--'+name, type=Path, required=True)
    parser.add_argument('--reboot-fixture', type=Path)
    args = parser.parse_args()
    check(*(getattr(args, name).resolve(strict=True) for name in ('binary', 'runtime', 'caddy')), args.reboot_fixture)
