"""Build real prior/next-preview fixtures and exercise upgrades on a disposable CI host.

The explicit --disposable-system-host mode creates/removes real accounts, units
and state. Never use it on a workstation or an existing DevOption installation.
Locally built archives are trusted CI inputs, not signed publication candidates.
"""
import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import pty
import re
import select
import shutil
import signal
import socket
import ssl
import subprocess
import sys
import tarfile
import time
from unittest.mock import patch

from install_bundle import Installer, STATE, PLATFORM, PROXY
from release_bundle import assemble, canonical, CADDY_SHA512
from release_candidates import load
from release_metadata import load_metadata, render_metadata
from release_preview import preview_changelog
from upgrade_bundle import Upgrade, UpgradeHost, UNIT, RECEIPT
from verify_bundle_provenance import manifest_from_archive

HOSTNAME = 'platform.upgrade.localhost'
PASSWORD = b'disposable-upgrade-fixture-only'


def run(*args, cwd=None):
    result = subprocess.run(args, cwd=cwd, check=False, capture_output=True, timeout=240)
    if result.returncode:
        raise ValueError('Fixture command failed: ' + args[0] + '\n' + result.stderr[-8192:].decode(errors='replace'))
    return result.stdout


def prepare(repository, output):
    from datetime import date
    output.mkdir(mode=0o700)
    reservation = load((repository/'release-candidates/0.1.0-preview.1.json').read_bytes())
    source = reservation['candidate']['source_commit']
    # Fetch only the exact public reserved source; no credentials or writes to a
    # remote repository. The previous executable is built from that real revision.
    run('git', 'fetch', '--no-tags', '--depth', '1', 'origin', source, cwd=repository)
    for name in ('previous', 'candidate'):
        print('Preparing real executable fixture: ' + name, flush=True)
        checkout = output/name
        run('git', 'clone', '--no-hardlinks', '--quiet', str(repository), str(checkout))
        if name == 'previous':
            run('git', 'checkout', '--quiet', '--detach', source, cwd=checkout)
        else:
            from release_plan import workspace
            current, _ = workspace(checkout)
            for filename in ('Cargo.toml', 'Cargo.lock'):
                path = checkout/filename
                path.write_text(path.read_text().replace('version = "'+current+'"', 'version = "0.1.0-preview.2"'))
            path = checkout/'release-metadata.toml'
            raw = '''schema_version = 2
version = "0.1.0-preview.2"
channel = "preview"
platforms = ["ubuntu-24.04-amd64"]
[compatibility]
installation = "fresh-and-upgrade"
upgrade_from = ["0.1.0-preview.1"]
data_schema = 1
configuration_schema = 1
data = "Unchanged schema 1 in this disposable source-build fixture."
configuration = "Unchanged base bundle service and proxy profile."
[migration]
required = false
instructions = "No migration is executed."
[recovery]
binary_rollback = false
instructions = "Restore the verified offline backup and previous binary together."
[support]
status = "development-preview"
warnings = ["Disposable CI source-build fixture; not signed, published or recommended."]
'''
            path.write_text(raw)
            metadata, digest = load_metadata(checkout, '0.1.0-preview.2')
            path = checkout/'CHANGELOG.md'
            path.write_text(preview_changelog(path.read_text(), '0.1.0-preview.2', date(2026, 1, 1), render_metadata(metadata, digest)))
            run('git', 'add', 'Cargo.toml', 'Cargo.lock', 'release-metadata.toml', 'CHANGELOG.md', cwd=checkout)
            run('git', '-c', 'user.name=Disposable fixture', '-c', 'user.email=fixture@example.invalid',
                'commit', '--quiet', '-m', 'test: prepare next preview fixture\n\nRefs #263', cwd=checkout)
        # Share only compiler artifacts; both archives snapshot their executable
        # immediately and bind it to the clean corresponding checkout.
        target = output/'target'
        run('cargo', 'build', '-p', 'devoption-cli', '--release', '--locked', '--offline',
            '--target-dir', str(target), cwd=checkout)
        assemble(checkout, target/'release/devoption', output/(name+'-bundle'))
    import urllib.request
    url = 'https://github.com/caddyserver/caddy/releases/download/v2.11.7/caddy_2.11.7_linux_amd64.tar.gz'
    with urllib.request.urlopen(url, timeout=60) as response:
        raw = response.read(256*1024*1024+1)
    if hashlib.sha512(raw).hexdigest() != CADDY_SHA512:
        raise ValueError('Fixture proxy pin mismatch.')
    (output/'caddy.tar.gz').write_bytes(raw)
    print('Real reserved prior source and next-preview source fixtures assembled; no signing/publication.')


class FixtureHost(UpgradeHost):
    fail_candidate_health = False

    def public_health(self, hostname):
        if self.fail_candidate_health and b'preview.2' in Path(RECEIPT).read_bytes():
            raise ValueError('injected candidate health rejection')
        root = Path('/var/lib/caddy/data/caddy/pki/authorities/local/root.crt')
        for _ in range(30):
            try:
                context = ssl.create_default_context(cafile=str(root))
                for path, expected in (('/sign-in',200),('/projects',303)):
                    status, _, _, _ = request(path, context=context)
                    assert status == expected
                return
            except (OSError, AssertionError):
                time.sleep(.2)
        raise ValueError('Disposable proxy TLS did not become ready.')


def request(path, method='GET', body=None, csrf=None, cookie='', context=None):
    context = context or ssl.create_default_context(cafile='/var/lib/caddy/data/caddy/pki/authorities/local/root.crt')
    class Connection(http.client.HTTPSConnection):
        def connect(self):
            self.sock = context.wrap_socket(socket.create_connection(('127.0.0.1',443),timeout=5),server_hostname=HOSTNAME)
    connection = Connection(HOSTNAME,timeout=5,context=context)
    headers = {'Host':HOSTNAME, 'Origin':'https://'+HOSTNAME, 'Cookie':cookie}
    if body is not None:
        headers['Content-Type'] = 'application/x-www-form-urlencoded' if path=='/auth/sign-in' else 'text/plain;charset=UTF-8'
    if csrf:
        headers['X-CSRF-Token'] = csrf
    try:
        connection.request(method,path,body=body,headers=headers)
        response = connection.getresponse()
        return response.status,response.read(),response.getheader('X-CSRF-Token'),response.getheader('Set-Cookie')
    finally:
        connection.close()


def session():
    from urllib.parse import urlencode
    status,_,token,cookie = request('/auth/challenge','POST')
    assert status==204
    status,_,token,cookie = request('/auth/sign-in','POST',urlencode({'password':PASSWORD.decode()}),token,cookie.split(';')[0])
    assert status==204 and 'Secure' in cookie and 'HttpOnly' in cookie
    cookie=cookie.split(';')[0]
    status,_,token,_=request('/auth/session','POST',cookie=cookie)
    assert status==204 and token
    return token,cookie


def enroll(binary):
    pid, terminal=pty.fork()
    if pid==0:
        os.execvp('runuser',['runuser','-u','devoption','--',str(binary),'setup','--data-dir',PLATFORM+'/data'])
    deadline=time.monotonic()+30
    try:
        pending=[b'New owner password (15+ characters): ',b'Confirm owner password: ']
        received=b''
        while pending and time.monotonic()<deadline:
            if select.select([terminal],[],[],.2)[0]:
                received+=os.read(terminal,8192)
                if pending[0] in received:
                    os.write(terminal,PASSWORD+b'\n'); pending.pop(0); received=b''
        assert not pending
        while time.monotonic()<deadline:
            done,status=os.waitpid(pid,os.WNOHANG)
            if done:
                pid=None
                assert os.waitstatus_to_exitcode(status)==0
                return
            time.sleep(.05)
        raise ValueError('Fixture enrollment timed out.')
    finally:
        os.close(terminal)
        if pid is not None:
            os.kill(pid,signal.SIGKILL); os.waitpid(pid,0)


def check(directory):
    if os.geteuid()!=0 or os.environ.get('DEVOPTION_DISPOSABLE_UPGRADE_FIXTURE')!='1':
        raise ValueError('Explicit disposable system host opt-in and root are required.')
    host=FixtureHost()
    host.preflight(Path('/'),1024**3)
    for path in (STATE,PLATFORM,PROXY,'/opt/devoption','/etc/caddy',UNIT,'/etc/systemd/system/caddy-devoption.service'):
        if Path(path).exists() or Path(path).is_symlink():
            raise ValueError('Disposable fixture requires an empty installation: '+path)
    for who in ('devoption','caddy'):
        if host.account(who) is not None:
            raise ValueError('Disposable fixture accounts already exist.')
    with tarfile.open(directory/'caddy.tar.gz') as archive:
        raw=(directory/'caddy.tar.gz').read_bytes()
        assert hashlib.sha512(raw).hexdigest()==CADDY_SHA512
        caddy=archive.extractfile('caddy').read()
    installers=[]
    for name in ('previous','candidate'):
        folder=directory/(name+'-bundle')
        manifest=json.loads((folder/'manifest.json').read_bytes())
        archive_path=folder/f"devoption-{manifest['platform_version']}-ubuntu-24.04-amd64.tar.gz"
        manifest_from_archive(archive_path.read_bytes(),manifest['source_commit'],manifest['platform_version'])
        with tarfile.open(archive_path) as archive:
            files={m.name:archive.extractfile(m).read() for m in archive}
        # Synthetic reservation is for trusted source-build fixtures only. The
        # public CLI has no bypass for attestation/reservation verification.
        from bundle_candidate import identity
        record={'schema_version':1,'candidate':identity(manifest['source_commit'],manifest['platform_version'],hashlib.sha256(archive_path.read_bytes()).hexdigest(),'0'*64,'1','1'),
                'artifact':{'id':'1','transport_sha256':'0'*64}}
        installers.append(Installer(Path('/'),host,record,manifest,files,caddy,HOSTNAME))
    old,new=installers
    try:
        old.install(); enroll(Path(old.binary)); old.start(); host.public_health(HOSTNAME); host.enable()
        token,cookie=session()
        assert request('/projects','POST','Upgrade fixture project',token,cookie)[0]==204
        assert request('/account/profile','POST','Upgrade fixture owner',token,cookie)[0]==204
        saved=Path(PLATFORM+'/repositories/fixture.txt'); saved.write_bytes(b'original repository bytes')
        os.chown(saved,*host.account('devoption'))
        repository=Path(PLATFORM+'/repositories/checkout')
        host.run('runuser','-u','devoption','--','git','init','--quiet',str(repository))
        host.run('runuser','-u','devoption','--','git','-C',str(repository),'-c','user.name=Fixture',
                 '-c','user.email=fixture@example.invalid','commit','--quiet','--allow-empty','-m','prior release fixture')
        prior_head=host.run('runuser','-u','devoption','--','git','-C',str(repository),'rev-parse','HEAD').stdout
        original={p.name:p.read_bytes() for p in Path(PLATFORM+'/data').iterdir() if p.is_file()}
        transaction=Upgrade(old,new)
        assert transaction.apply()['phase']=='healthy'
        assert run(new.binary,'--version')==b'devoption 0.1.0-preview.2\n'
        assert host.run('runuser','-u','devoption','--','git','-C',str(repository),'rev-parse','HEAD').stdout==prior_head
        assert request('/auth/session','POST',cookie=cookie)[0]==401
        token,cookie=session()
        assert b'Upgrade fixture project' in request('/projects',cookie=cookie)[1]
        assert request('/account/profile',cookie=cookie)[1]==b'Upgrade fixture owner'
        saved.write_bytes(b'post-upgrade bytes')
        transaction.recover(confirm=True)
        assert saved.read_bytes()==b'original repository bytes'
        assert original=={p.name:p.read_bytes() for p in Path(PLATFORM+'/data').iterdir() if p.is_file()}
        assert host.run('runuser','-u','devoption','--','git','-C',str(repository),'rev-parse','HEAD').stdout==prior_head
        session()
        transaction=Upgrade(old,new,attempt=2)
        replace=transaction.replace
        def interrupt(path,*args,**kwargs):
            replace(path,*args,**kwargs)
            if path==Path(UNIT):
                raise SystemExit('controlled unit/receipt interruption')
        with patch.object(transaction,'replace',side_effect=interrupt):
            try: transaction.apply()
            except SystemExit: pass
            else: raise AssertionError('Interruption did not occur')
        for unit in ('devoption-public.service','caddy-devoption.service'):
            assert subprocess.run(['systemctl','is-enabled',unit],capture_output=True).stdout.strip()==b'disabled'
        transaction.recover(confirm=True); session()
        transaction=Upgrade(old,new,attempt=3)
        host.fail_candidate_health=True
        try: transaction.apply()
        except ValueError as error: assert 'injected candidate health' in str(error)
        else: raise AssertionError('Failed health was accepted')
        host.fail_candidate_health=False
        assert transaction.record()['phase']=='failed'
        transaction.recover(confirm=True); session()
        assert original=={p.name:p.read_bytes() for p in Path(PLATFORM+'/data').iterdir() if p.is_file()}
        print('Disposable real-system upgrade: prior/next executables, TLS owner login, project/profile/repository persistence, full-state restore, interrupted activation and rejected-health recovery passed.')
    finally:
        if any(host.active(unit) for unit in ('devoption-public.service','caddy-devoption.service')):
            host.stop()
        subprocess.run(['systemctl','disable','devoption-public.service','caddy-devoption.service'],check=False,capture_output=True,timeout=30)
        for path in (UNIT,'/etc/systemd/system/caddy-devoption.service'):
            Path(path).unlink(missing_ok=True)
        host.reload()
        for path in ('/opt/devoption',PLATFORM,PROXY,STATE,'/etc/caddy'):
            if Path(path).exists(): shutil.rmtree(path)
        for who in ('devoption','caddy'):
            if host.account(who) is not None:
                host.run('userdel',who)
        Path('/run/lock/devoption-installer.lock').unlink(missing_ok=True)
        print('Disposable accounts/services/state removed; no host or CA data retained.')


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepare',type=Path)
    parser.add_argument('--disposable-system-host',type=Path)
    args=parser.parse_args()
    if bool(args.prepare)==bool(args.disposable_system_host):
        parser.error('Select one explicit preparation or disposable-system-host mode.')
    if args.prepare:
        prepare(Path(__file__).resolve().parents[1],args.prepare.resolve())
    else:
        check(args.disposable_system_host.resolve(strict=True))
