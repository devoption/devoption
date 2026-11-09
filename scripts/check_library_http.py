"""Library review/approval/download acceptance within check_library_build's disposable host."""
import fcntl
import hashlib
import http.client
import json
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

    registry = root / 'http-registry'
    registry.mkdir(mode=0o700)
    for name in ('candidates', 'policy'):
        (registry / name).mkdir(mode=0o700)
    registry_config = registry / 'config.toml'
    registry_config.write_text("schema=1\nregistry='team'\nindex='sparse+https://packages.example.test/index/'\n")
    registry_config.chmod(0o600)

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
            '--build-history-dir', str(root / 'history'), '--build-execution', 'enabled',
            '--package-registry-dir', str(registry)],
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
        # The cookie+CSRF adapter treats a missing token as unauthenticated.
        assert request('POST', base + '/prepare', with_csrf=False)[0] == 401
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

        # Review/stage from a real successful controller build. These operations
        # store private candidates only; synthetic publication is separate below.
        staging = history_path + '/attempts/' + attempt + '/registry'
        package_registry = f'/projects/{project}/registry/reusable-fixture/0.1.0'
        assert request('POST', staging + '/prepare', authenticated=False)[0] == 401
        assert request('POST', staging + '/prepare', with_csrf=False)[0] == 401
        mutate(staging.replace(project, other.decode()) + '/prepare', expected=404)
        mutate(staging.replace(attempt, '00' * 16) + '/prepare', expected=404)
        mutate(staging + '/prepare', 'unexpected', expected=400)
        mutate(staging + '/prepare', 'x' * 33, expected=413)

        def stage_review():
            headers, data = mutate(staging + '/prepare', expected=200)
            assert headers['cache-control'] == 'no-store'
            assert headers['x-content-type-options'] == 'nosniff'
            review = json.loads(data)
            assert review['change'] == {'action': 'stage'}
            assert review['attempt'] == attempt and review['source_project'] == project
            assert review['registry'] == 'team' and review['name'] == 'reusable-fixture'
            assert review['version'] == '0.1.0' and review['commit'] == receipt['commit']
            assert review['source_sha256'] == receipt['source_sha256']
            assert review['source_archive_sha256'] == hashlib.sha256(package).hexdigest()
            assert review['receipt_sha256'] == hashlib.sha256(receipt_bytes).hexdigest()
            return review

        # The cross-process retention lease rejects concurrent artifact changes.
        with (root / 'jobs/retention.lock').open('rb') as retention:
            fcntl.flock(retention, fcntl.LOCK_EX | fcntl.LOCK_NB)
            mutate(staging + '/prepare', expected=409)
        review = stage_review()
        assert not list((registry / 'candidates').glob('*.cargo'))
        # Deliver an approval body only after logout. A conflicting read confirms
        # the staging handler admitted its locks and is waiting for the body.
        delayed = http.client.HTTPConnection('127.0.0.1', port, timeout=10)
        try:
            delayed.putrequest('POST', staging + '/approve', skip_host=True)
            for key, value in {'Host': f'127.0.0.1:{port}', 'Origin': origin,
                    'Cookie': cookie, 'X-CSRF-Token': csrf, 'Content-Type': 'text/plain',
                    'Content-Length': '32'}.items():
                delayed.putheader(key, value)
            delayed.endheaders()
            deadline = time.monotonic() + 2
            while request('GET', package_registry + '/status')[0] != 409:
                if time.monotonic() > deadline:
                    raise TimeoutError('Staging body/lock admission')
                time.sleep(.01)
            assert request('POST', '/auth/sign-out')[0] == 204
            delayed.send(review['review_token'].encode())
            response = delayed.getresponse()
            assert response.status == 401
            response.read()
        finally:
            delayed.close()
        cookie, csrf = '', ''
        login()
        assert not list((registry / 'candidates').glob('*.cargo'))
        review = stage_review()
        original_source = (source / 'src/lib.rs').read_bytes()
        (source / 'src/lib.rs').write_bytes(original_source + b'// changed after review\n')
        mutate(staging + '/approve', review['review_token'], expected=409)
        (source / 'src/lib.rs').write_bytes(original_source)
        mutate(staging + '/approve', review['review_token'], expected=409)
        review = stage_review()
        mutate(f'/projects/{project}/name', 'Renamed library fixture')
        mutate(staging + '/approve', review['review_token'], expected=409)
        review = stage_review()
        changed_receipt = receipt_bytes.replace(receipt['review_sha256'].encode(), b'01' * 32)
        assert changed_receipt != receipt_bytes
        retained.write_bytes(changed_receipt)
        mutate(staging + '/approve', review['review_token'], expected=409)
        retained.write_bytes(receipt_bytes)
        review = stage_review()
        archive.write_bytes(b'corrupt retained package')
        mutate(staging + '/approve', review['review_token'], expected=503)
        archive.write_bytes(package)
        review = stage_review()
        newer = stage_review()
        assert newer['review_token'] != review['review_token']
        mutate(staging + '/approve', review['review_token'], expected=409)
        mutate(staging + '/approve', newer['review_token'], expected=409)
        review = stage_review()
        _, data = mutate(staging + '/approve', review['review_token'], expected=200)
        assert json.loads(data) == {'staged': True, 'already_present': False}
        mutate(staging + '/approve', review['review_token'], expected=409)
        files = list((registry / 'candidates').glob('*.cargo'))
        assert len(files) == 1 and files[0].read_bytes().startswith(b'DVCARGO2')
        assert request('GET', package_registry + '/status')[0] == 404
        assert request('GET', package_registry + '/index')[0] == 404
        review = stage_review()
        _, data = mutate(staging + '/approve', review['review_token'], expected=200)
        assert json.loads(data) == {'staged': True, 'already_present': True}
        # A distinct explicit publication review grants the disposable consumer.
        _, data = mutate(package_registry + '/prepare', json.dumps({'action': 'publish', 'readers': [other.decode()]}), expected=200)
        publication = json.loads(data)
        mutate(package_registry + '/approve', publication['review_token'], expected=200)
        consumer_registry = package_registry.replace(project, other.decode())
        status, _, line = request('GET', consumer_registry + '/index')
        assert status == 200
        checksum = json.loads(line)['cksum']
        assert checksum == review['archive_sha256']
        entry_url = f'/projects/{other.decode()}/registry-index/re/us/reusable-fixture'
        status, headers, entry = request('GET', entry_url)
        assert status == 200 and entry == line
        assert headers['content-type'] == 'application/x-ndjson'
        assert headers['cache-control'] == 'no-store'
        assert headers['x-content-type-options'] == 'nosniff'
        assert request('GET', entry_url, authenticated=False)[0] == 303
        assert request('GET', entry_url.replace('re/us/', 'wrong/'))[0] == 404
        _, data = mutate(package_registry + '/prepare', json.dumps({'action': 'yank', 'yanked': True}), expected=200)
        mutate(package_registry + '/approve', json.loads(data)['review_token'], expected=200)
        assert json.loads(request('GET', entry_url)[2])['yanked'] is True
        _, data = mutate(package_registry + '/prepare', json.dumps({'action': 'readers', 'readers': []}), expected=200)
        mutate(package_registry + '/approve', json.loads(data)['review_token'], expected=200)
        assert request('GET', entry_url)[0] == 404
        _, data = mutate(package_registry + '/prepare', json.dumps({'action': 'readers', 'readers': [other.decode()]}), expected=200)
        mutate(package_registry + '/approve', json.loads(data)['review_token'], expected=200)
        assert json.loads(request('GET', entry_url)[2])['yanked'] is True
        download = consumer_registry + '/archive/' + checksum
        status, _, cargo_archive = request('GET', download)
        assert status == 200 and hashlib.sha256(cargo_archive).hexdigest() == checksum
        assert request('GET', download, authenticated=False)[0] == 303
        restart_review = stage_review()
        stop()
        start()
        login()
        assert request('GET', package_url)[2] == package
        assert request('GET', package_url + '/receipt')[2] == receipt_bytes
        mutate(staging + '/approve', restart_review['review_token'], expected=409)
        assert request('GET', download)[2] == cargo_archive
        assert json.loads(request('GET', entry_url)[2])['yanked'] is True
        (root / 'verified-source.tar').write_bytes(package)
        print('Authenticated HTTP library build and candidate staging: exact review/receipt, source and catalog revalidation, corruption/replay/CSRF refusal, private staging, separate publication/project grant, complete project index with live yanks/revocation, exact archive acquisition and restart passed.', flush=True)
    finally:
        stop()
        log.close()
