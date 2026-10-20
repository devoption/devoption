"""Trusted signed-packet inputs for the explicit disposable reboot fixture only."""
import hashlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import time

from install_bundle import Installer, verified_files, proxy_binary
from release_candidates import load
from upgrade_bundle import Upgrade, UpgradeHost
from check_bundle_upgrade import HOSTNAME
from verify_bundle_provenance import snapshot

GH_SHA256 = 'bb766f710eef8ede859c18578c72c327597cd4c8a85b06001b1f3843c6019386'


def verifier_archive(directory):
    raw = snapshot(directory/'gh.tar.gz', 128*1024**2)
    if hashlib.sha256(raw).hexdigest() != GH_SHA256:
        raise ValueError('Signed fixture requires the pinned Linux GitHub CLI archive.')
    return raw


def linux_verifier(directory):
    with tarfile.open(fileobj=io.BytesIO(verifier_archive(directory))) as archive:
        member = archive.getmember('gh_2.102.0_linux_amd64/bin/gh')
        if not member.isfile() or not 0 < member.size < 128*1024**2:
            raise ValueError('Invalid pinned verifier executable.')
        path = directory/'gh'
        path.write_bytes(archive.extractfile(member).read())
        path.chmod(0o755)
    return path


def installers(directory, gh=None):
    if gh is None:
        gh = linux_verifier(directory)
    else:
        verifier_archive(directory)
    host = UpgradeHost()
    result = []
    for name in ('previous', 'candidate'):
        record = load((directory/(name+'.json')).read_bytes())
        manifest, files = verified_files(directory/(name+'.zip'), record, gh, True)
        result.append(Installer(Path('/'), host, record, manifest, files,
                                proxy_binary(directory/'caddy.tar.gz', manifest), HOSTNAME))
    Upgrade(*result)  # Compatibility must pass before booting/mutating a guest.
    return result


def command(directory, mode, attempt=1, candidate_packet=None, expect_failure=False):
    args = [sys.executable, str(Path(__file__).with_name('upgrade_bundle.py')), mode]
    for name in ('previous', 'candidate'):
        args += ['--'+name+'-reservation', str(directory/(name+'.json')),
                 '--'+name+'-packet', str(candidate_packet if name == 'candidate' and candidate_packet else directory/(name+'.zip'))]
    args += ['--gh', str(directory/'gh'), '--caddy-archive', str(directory/'caddy.tar.gz'),
             '--hostname', HOSTNAME, '--attempt', str(attempt), '--allow-validation-candidate']
    if mode == 'recover':
        args += ['--confirm-discard-post-backup-data']
    process = subprocess.run(args, capture_output=True, timeout=300)
    if expect_failure:
        if process.returncode == 0 or b'Transport ZIP differs from reviewed digest.' not in process.stderr:
            raise ValueError('Corrupt packet did not fail at the production transport-digest gate.')
        return
    if process.returncode:
        raise ValueError('Production upgrade CLI failed: '+process.stderr[-4096:].decode(errors='replace'))
    return json.loads(process.stdout)


def validate_report(result, previous, candidate):
    from check_upgrade_reboot import validate_report as reboot_report
    reboot_report(result)
    if (result.get('authenticated_packets') is not True or result.get('production_cli') is not True or
            result.get('corrupt_packet_rejected') is not True or result.get('previous_identity') != previous or
            result.get('candidate_identity') != candidate):
        raise ValueError('Signed acceptance report differs from independently authenticated inputs.')
    return result


def install(directory):
    args = [sys.executable, str(Path(__file__).with_name('install_bundle.py')), 'install',
            '--reservation', str(directory/'previous.json'), '--packet', str(directory/'previous.zip'),
            '--gh', str(directory/'gh'), '--caddy-archive', str(directory/'caddy.tar.gz'),
            '--hostname', HOSTNAME, '--allow-validation-candidate']
    process = subprocess.run(args, capture_output=True, timeout=300)
    if process.returncode:
        raise ValueError('Production install CLI failed: '+process.stderr[-4096:].decode(errors='replace'))


def trust_guest_ca():
    # This explicitly provisioned, disposable .localhost guest uses normal OS
    # trust for production readiness. No production TLS/verifier bypass is added.
    root = Path('/var/lib/caddy/data/caddy/pki/authorities/local/root.crt')
    for _ in range(60):
        if root.is_file():
            break
        time.sleep(.5)
    else:
        raise ValueError('Disposable local CA was not issued.')
    shutil.copyfile(root, '/usr/local/share/ca-certificates/devoption-fixture.crt')
    subprocess.run(['update-ca-certificates'], check=True, capture_output=True, timeout=60)
