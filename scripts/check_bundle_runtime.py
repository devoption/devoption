"""CI smoke of a trusted local assembly with the repository's compiled WASIp2 fixture.

This is not a verifier/runner for downloaded candidates or arbitrary applications.
The source/build checker must pass before any archive executable is run.
"""

import argparse
import hashlib
import http.client
import io
import json
from pathlib import Path
import socket
import subprocess
import tarfile
import tempfile
import time
import uuid

from check_release_bundle import check as check_local
from release_bundle import MAX_BINARY, RUNTIME, TARGET, probe_runtime
from release_plan import workspace
from verify_bundle_provenance import snapshot


def smoke(binary, component, version):
    probe_runtime(binary, version)
    raw = snapshot(component, 4 * 1024 * 1024)
    with tempfile.TemporaryDirectory(prefix='devoption-worker-smoke-') as temporary:
        selected = Path(temporary) / 'fixture.wasm'
        selected.write_bytes(raw)
        environment = {'PATH': '/usr/bin:/bin', 'LANG': 'C.UTF-8', 'INVOCATION_ID': uuid.uuid4().hex}
        subprocess.run([str(binary), 'app-worker', str(selected), '0', 'validate'], env=environment,
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
        with socket.socket() as reservation:
            reservation.bind(('127.0.0.1', 0))
            port = reservation.getsockname()[1]
        child = subprocess.Popen([str(binary), 'app-worker', str(selected), str(port), 'serve'],
                                 env=environment, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                if child.poll() is not None:
                    raise ValueError('Packaged worker exited before fixture readiness.')
                connection = http.client.HTTPConnection('127.0.0.1', port, timeout=2)
                try:
                    connection.request('GET', '/healthz')
                    response = connection.getresponse()
                    body = response.read(1025)
                    if (response.status == 200 and body == b'ok A' and
                            response.getheader('X-DevOption-Invocation') == environment['INVOCATION_ID']):
                        break
                except (OSError, http.client.HTTPException):
                    pass
                finally:
                    connection.close()
                time.sleep(.1)
            else:
                raise ValueError('Packaged worker fixture readiness deadline exceeded.')
            for method, path, status, body in [('GET', '/', 200, b'DevOption app revision A'),
                                               ('GET', '/missing', 404, b'Not found'),
                                               ('POST', '/', 405, b'Method not allowed')]:
                connection = http.client.HTTPConnection('127.0.0.1', port, timeout=5)
                try:
                    connection.request(method, path)
                    response = connection.getresponse()
                    content = response.read(4097)
                    if (child.poll() is not None or response.status != status or body not in content or
                            response.getheader('X-DevOption-Invocation') != environment['INVOCATION_ID']):
                        raise ValueError('Packaged worker fixture routing or invocation differs.')
                finally:
                    connection.close()
        finally:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=5)


def check(repository, binary, runtime, output, component):
    digest = check_local(repository, binary, output, runtime)
    version, _ = workspace(repository)
    archive = output / f'devoption-{version}-{TARGET}.tar.gz'
    raw = snapshot(archive, MAX_BINARY + 8 * 1024 * 1024)
    if hashlib.sha256(raw).hexdigest() != digest:
        raise ValueError('Trusted archive changed before worker smoke; no executable was run.')
    with tarfile.open(fileobj=io.BytesIO(raw), mode='r:gz') as bundle:
        manifest = json.loads(bundle.extractfile('manifest.json').read())
        if manifest['schema_version'] != 3:
            raise ValueError('A new dual-executable bundle is required for this fixture.')
        worker = bundle.extractfile(RUNTIME).read()
    with tempfile.TemporaryDirectory(prefix='devoption-packaged-worker-') as temporary:
        executable = Path(temporary) / 'devoption-runtime'
        executable.write_bytes(worker)
        executable.chmod(0o700)
        smoke(executable, component, version)
    print('Packaged worker version, WASIp2 validation, health, routing and owned-process cleanup passed.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--repository', type=Path, default=Path.cwd())
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--component', type=Path, required=True)
    args = parser.parse_args()
    check(args.repository, args.binary, args.runtime, args.output, args.component)
