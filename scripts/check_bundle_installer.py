"""Exercise trusted local bundle installation in disposable paths, without system changes.

CI source/build comparison authenticates this local input. This is not a substitute
for Sigstore verification of downloaded packets; those policy tests run separately.
"""

import http.client
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import subprocess
import sys
import tarfile
import tempfile
import time

from install_bundle import Installer
from bundle_candidate import identity
from release_bundle import probe
from test_install_bundle import FixtureHost


def enroll(binary, data):
    enroll_command([str(binary), 'setup', '--data-dir', str(data)])


def enroll_command(arguments, password=b'disposable-installer-fixture-only', timeout=30):
    """Exercise hidden terminal prompts through a selected trusted fixture command."""
    pid, terminal = pty.fork()
    if pid == 0:
        os.execvp(arguments[0], arguments)
    try:
        prompts = [b'New owner password (15+ characters): ', b'Confirm owner password: ']
        output = b''
        deadline = time.monotonic() + timeout
        while prompts and time.monotonic() < deadline:
            if select.select([terminal],[],[],1)[0]:
                output += os.read(terminal,8192)
                if prompts[0] in output:
                    os.write(terminal,password+b'\n')
                    prompts.pop(0)
        if prompts:
            raise ValueError('Disposable PTY setup did not request both passwords.')
        while time.monotonic() < deadline:
            done, status = os.waitpid(pid, os.WNOHANG)
            if done:
                pid = None
                if os.waitstatus_to_exitcode(status) != 0:
                    raise ValueError('Disposable owner setup failed.')
                return
            time.sleep(.05)
        raise ValueError('Disposable owner setup exceeded its deadline.')
    finally:
        os.close(terminal)
        if pid is not None:
            os.kill(pid,signal.SIGKILL)
            os.waitpid(pid,0)


def restart(binary, data, repositories):
    child = subprocess.Popen([str(binary),'serve','--port','0','--data-dir',str(data),
                              '--repository-root',str(repositories)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        if not select.select([child.stdout],[],[],10)[0]:
            raise ValueError('Installed binary did not become ready.')
        line = child.stdout.readline(4096)
        match = re.match(rb'Listening on http://127\.0\.0\.1:([0-9]+)',line)
        if not match:
            raise ValueError('Installed binary did not report a loopback listener.')
        connection = http.client.HTTPConnection('127.0.0.1',int(match[1]),timeout=5)
        try:
            connection.request('GET','/projects')
            response = connection.getresponse()
            assert response.status == 303 and response.getheader('Location') == '/sign-in'
        finally:
            connection.close()
    finally:
        child.terminate()
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=5)
        child.stdout.close()
        child.stderr.close()


def check(output):
    manifest = json.loads((output/'manifest.json').read_bytes())
    archive_path = output/f"devoption-{manifest['platform_version']}-{manifest['target']}.tar.gz"
    with tarfile.open(archive_path) as archive:
        files = {m.name: archive.extractfile(m).read() for m in archive}
    # Only the reservation identity fields affect file placement in this local CI
    # test; no synthetic proof is accepted by the production signature verifier.
    import hashlib
    record = {"schema_version": 1,
              "candidate": identity(manifest['source_commit'],manifest['platform_version'],
                                    hashlib.sha256(archive_path.read_bytes()).hexdigest(),'0'*64,'1','1'),
              "artifact": {"id": "1", "transport_sha256": "0"*64}}
    with tempfile.TemporaryDirectory(prefix='devoption-install-fixture-') as directory:
        root = Path(directory).resolve()
        for parent in ('opt','var/lib','etc/systemd/system'):
            (root/parent).mkdir(parents=True)
        host = FixtureHost(root)
        installer = Installer(root,host,record,manifest,files,b'fixture proxy, never executed','platform.example.com')
        assert not installer.preflight()['resumed']
        assert installer.install()['missing_files']==[]
        binary = installer.path(installer.binary)
        assert binary.read_bytes()==files['bin/devoption']
        probe(binary,manifest['platform_version'])
        # Verify generated system units with actual systemd's parser. Substitute
        # only disposable paths/current UID; no system service is installed.
        import pwd
        import grp
        for name in ('devoption-public.service','caddy-devoption.service'):
            unit = installer.path('/etc/systemd/system/'+name).read_text()
            for account in ('devoption','caddy'):
                unit=unit.replace('User='+account,'User='+pwd.getpwuid(os.getuid()).pw_name)
                unit=unit.replace('Group='+account,'Group='+grp.getgrgid(os.getgid()).gr_name)
            unit=unit.replace(installer.binary,str(binary)).replace(installer.caddy,'/usr/bin/true')
            unit=unit.replace('/var/lib/devoption',str(root/'var/lib/devoption')).replace('/var/lib/caddy',str(root/'var/lib/caddy'))
            path=root/name
            path.write_text(unit)
            subprocess.run(['systemd-analyze','--man=no','--recursive-errors=no','verify',str(path)],check=True,capture_output=True,timeout=30)
        data=installer.path('/var/lib/devoption/data')
        enroll(binary,data)
        before=(data/'owner.bin').read_bytes()
        for _ in range(2):
            restart(binary,data,installer.path('/var/lib/devoption/repositories'))
        assert installer.install()['missing_files']==[]
        assert (data/'owner.bin').read_bytes()==before
        assert json.loads(installer.path('/var/lib/devoption-installer/installation.json').read_bytes())==installer.identity
    print('Disposable bundle installer: exact executable, PTY enrollment, readiness, restart and data-preserving reinstall passed; no system changes.')


if __name__=='__main__':
    if sys.platform!='linux' or len(sys.argv)!=2:
        raise SystemExit('usage on Linux: python3 scripts/check_bundle_installer.py TRUSTED_LOCAL_BUNDLE_OUTPUT')
    check(Path(sys.argv[1]).resolve(strict=True))
