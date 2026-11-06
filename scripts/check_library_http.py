"""Library review/approval/download acceptance within check_library_build's disposable host."""
import hashlib
import http.client
import re
import socket
import subprocess
import time
import tomllib

from check_bundle_installer import enroll
from check_named_web_app import run


def check(binary, root, fixture):
    source = root / 'repositories/library'
    (source / 'src/lib.rs').write_bytes((fixture / 'src/lib.rs').read_bytes())
    run('git', '-C', source, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false',
        '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'add', '--all')
    run('git', '-C', source, '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false',
        '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'HTTP library fixture')
    enroll(binary, root / 'data')
    with socket.socket() as probe:
        probe.bind(('127.0.0.1', 0))
        port = probe.getsockname()[1]
    origin = f'http://127.0.0.1:{port}'
    cookie, csrf, controller = '', '', None
    log_path = root / 'library-http.log'
    log = log_path.open('wb')

    def request(method, path, body='', authenticated=True, with_csrf=True):
        headers = {'Host': f'127.0.0.1:{port}'}
        if method == 'POST':
            headers.update({'Origin': origin, 'Content-Type': 'text/plain;charset=UTF-8'})
        if authenticated and cookie:
            headers['Cookie'] = cookie
            if with_csrf:
                headers['X-CSRF-Token'] = csrf
        connection = http.client.HTTPConnection('127.0.0.1', port, timeout=30)
        try:
            connection.request(method, path, body, headers)
            response = connection.getresponse()
            return response.status, {k.lower(): v for k, v in response.getheaders()}, response.read(4 * 1024 * 1024 + 16385)
        finally:
            connection.close()

    def start():
        nonlocal controller, cookie, csrf
        cookie, csrf = '', ''
        controller = subprocess.Popen([str(binary), 'serve', '--port', str(port),
            '--data-dir', str(root / 'data'), '--repository-root', str(root / 'repositories'),
            '--build-history-dir', str(root / 'history'), '--build-execution', 'enabled'],
            stdin=subprocess.DEVNULL, stdout=log, stderr=log)
        deadline = time.monotonic() + 40
        while True:
            if controller.poll() is not None:
                raise ValueError('Library HTTP controller failed: ' + log_path.read_text()[-4096:])
            try:
                if request('GET', '/sign-in', authenticated=False)[0] == 200:
                    break
            except OSError:
                pass
            if time.monotonic() > deadline:
                raise TimeoutError('Library HTTP readiness')
            time.sleep(.1)

    def stop():
        if controller and controller.poll() is None:
            controller.terminate()
            try:
                controller.wait(timeout=10)
            except subprocess.TimeoutExpired:
                controller.kill()
                controller.wait(timeout=5)

    def login():
        nonlocal cookie, csrf
        status, headers, _ = request('POST', '/auth/challenge')
        assert status == 204
        cookie, csrf = headers['set-cookie'].split(';', 1)[0], headers['x-csrf-token']
        connection = http.client.HTTPConnection('127.0.0.1', port, timeout=30)
        try:
            connection.request('POST', '/auth/sign-in', 'password=disposable-installer-fixture-only',
                {'Host': f'127.0.0.1:{port}', 'Origin': origin, 'Cookie': cookie,
                 'X-CSRF-Token': csrf, 'Content-Type': 'application/x-www-form-urlencoded'})
            response = connection.getresponse()
            assert response.status == 204
            headers = {k.lower(): v for k, v in response.getheaders()}
            response.read()
            cookie, csrf = headers['set-cookie'].split(';', 1)[0], headers['x-csrf-token']
        finally:
            connection.close()

    def mutate(path, body='', expected=204):
        nonlocal csrf
        status, headers, _ = request('POST', '/auth/session')
        assert status == 204
        csrf = headers['x-csrf-token']
        status, headers, data = request('POST', path, body)
        if status != expected:
            raise ValueError(f'Library action returned {status}, expected {expected}: ' + log_path.read_text()[-4096:])
        return headers, data

    try:
        start()
        login()
        projects = set(re.findall(rb'href="/projects/([0-9a-f]{32})"', request('GET', '/projects')[2]))
        assert len(projects) == 1
        project = next(iter(projects)).decode()
        history_path = f'/projects/{project}/builds'
        history = request('GET', history_path)[2]
        repository, = set(re.findall(rb'/builds/([0-9a-f]{32})/run', history))
        base = history_path + '/' + repository.decode()
        source_page = request('GET', base)[2]
        assert b'Save verified source' in source_page
        before = set(re.findall(rb'/attempts/([0-9a-f]{32})', history))
        assert request('POST', base + '/prepare', authenticated=False)[0] == 401
        assert request('POST', base + '/prepare', with_csrf=False)[0] == 403
        mutate(base + '/prepare')
        page = request('GET', base + '/run')[2]
        for expected in [b'LibraryBuildV1', b'Library source package (.tar)', b'cargo build --release --locked --offline', b'cargo test --release --locked --offline', b'wasm32-wasip2']:
            assert expected in page, expected
        assert b'Deployment requires its own approval' not in page
        token = re.search(rb'data-build-action="approve" data-token="([0-9a-f]{32})"', page)[1].decode()
        mutate(base + '/approve', '00' * 16, expected=409)
        mutate(base + '/approve', token, expected=202)
        mutate(base + '/approve', token, expected=409)
        deadline = time.monotonic() + 75
        attempt = None
        while True:
            current = request('GET', history_path)[2]
            created = set(re.findall(rb'/attempts/([0-9a-f]{32})', current)) - before
            if len(created) == 1:
                attempt = next(iter(created)).decode()
                detail = request('GET', history_path + '/attempts/' + attempt)[2]
                if b'Download source package' in detail:
                    break
                if b'>Failed<' in detail:
                    raise ValueError('Library HTTP build failed: ' + detail.decode(errors='replace')[-4096:])
            if time.monotonic() > deadline:
                raise TimeoutError('Library HTTP build completion: ' + log_path.read_text()[-4096:])
            time.sleep(.2)
        package_url = history_path + '/attempts/' + attempt + '/package'
        assert request('GET', package_url, authenticated=False)[0] == 303
        mutate('/projects', 'Other library fixture')
        other, = set(re.findall(rb'href="/projects/([0-9a-f]{32})"', request('GET', '/projects')[2])) - projects
        assert request('GET', package_url.replace(project, other.decode()))[0] == 404
        status, headers, package = request('GET', package_url)
        assert status == 200 and headers['cache-control'] == 'no-store'
        assert headers['x-content-type-options'] == 'nosniff'
        assert headers['content-type'] == 'application/x-tar'
        assert headers['content-disposition'] == 'attachment; filename=library-source.tar'
        status, headers, receipt_bytes = request('GET', package_url + '/receipt')
        assert status == 200 and headers['content-disposition'] == 'attachment; filename=library-build-receipt.toml'
        receipt = tomllib.loads(receipt_bytes.decode())
        assert receipt['job'] == attempt and receipt['project'] == project
        assert receipt['artifact_sha256'] == hashlib.sha256(package).hexdigest()
        assert receipt['artifact_bytes'] == len(package)
        retained, = [p for p in (root / 'jobs').glob('*/artifact/receipt.toml')
                     if tomllib.loads(p.read_text()).get('job') == attempt]
        archive = retained.with_name('source.tar')
        archive.write_bytes(b'corrupt retained package')
        assert request('GET', package_url)[0] == 503
        archive.write_bytes(package)
        stop()
        start()
        login()
        assert request('GET', package_url)[2] == package
        assert request('GET', package_url + '/receipt')[2] == receipt_bytes
        (root / 'verified-source.tar').write_bytes(package)
        print('Authenticated HTTP library review/approval, replay and CSRF refusal, scoped verified downloads, corrupt-archive refusal and restart passed.', flush=True)
    finally:
        stop()
        log.close()
