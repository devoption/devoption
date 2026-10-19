"""Operator-triggered same-schema preview upgrades and verified offline recovery.

No publication, migrations, unattended updates or app-capable source installations.
The CLI always uses / and reauthenticates both packets before every operation.
"""

import argparse
import copy
import hashlib
import http.client
import json
import os
from pathlib import Path
import re
import ssl
import stat
import subprocess
import tempfile
import uuid

from install_bundle import Installer, STATE, PLATFORM, PROXY, installation_lock, verified_files, proxy_binary
from installer_host import Host
from recovery_snapshot import LIMIT, inventory, copy_records, sync_directory
from release_bundle import canonical
from release_candidates import load
from release_metadata import validate_metadata
from verify_bundle_provenance import snapshot, unique_object

UNIT = '/etc/systemd/system/devoption-public.service'
RECEIPT = STATE + '/installation.json'
PHASES = {'prepared', 'backed-up', 'activated', 'healthy', 'failed', 'restoring', 'restored'}
TERMINAL = {'healthy', 'restored'}


class UpgradeHost(Host):
    def stop(self):
        # Pending transactions must not boot an unvalidated candidate after a
        # power interruption. Re-enable only after the recorded health result.
        self.run('systemctl', 'disable', '--now', 'caddy-devoption.service', 'devoption-public.service')
        for unit in ('caddy-devoption.service', 'devoption-public.service'):
            result = self.run('systemctl', 'show', unit, '--property=ActiveState,ControlGroup')
            values = dict(line.split('=', 1) for line in result.stdout.decode().splitlines() if '=' in line)
            if values.get('ActiveState') not in ('inactive', 'failed') or values.get('ControlGroup') != '':
                raise ValueError('Service shutdown is unconfirmed; inspect before recovery: ' + unit)

    def start(self):
        self.run('systemctl', 'start', 'devoption-public.service', 'caddy-devoption.service')

    def enable(self):
        self.run('systemctl', 'enable', 'devoption-public.service', 'caddy-devoption.service')

    def no_app_services(self):
        for unit in ('devoption-build.service', 'devoption-app.service', 'devoption-app-validate.service'):
            result = subprocess.run(['/usr/bin/systemctl', 'show', unit, '--property=LoadState', '--value'],
                                    capture_output=True, timeout=10, check=False)
            if result.returncode not in (0, 1, 4) or result.stdout.strip() != b'not-found':
                raise ValueError('App/build units need their coordinated upgrade path: ' + unit)

    def probe(self, binary, version):
        result = self.run('runuser', '-u', 'devoption', '--', binary, '--version')
        if result.stdout != f'devoption {version}\n'.encode() or result.stderr:
            raise ValueError('Installed executable version differs from the verified candidate.')

    def public_health(self, hostname):
        # Anonymous probes only; never request or store an owner password/session.
        for path, expected in (('/sign-in', 200), ('/projects', 303)):
            connection = http.client.HTTPSConnection(hostname, timeout=10, context=ssl.create_default_context())
            try:
                connection.request('GET', path)
                response = connection.getresponse()
                if response.status != expected or (expected == 303 and response.getheader('Location') != '/sign-in'):
                    raise ValueError('Public HTTPS readiness failed; inspect service and certificate status.')
            finally:
                connection.close()


class Upgrade:
    def __init__(self, previous, candidate, attempt=1):
        self.old, self.new = previous, candidate
        self.root, self.host = previous.root, previous.host
        if candidate.root != self.root or candidate.host is not self.host or previous.hostname != candidate.hostname:
            raise ValueError('Use the same host and hostname for both verified installations.')
        if type(attempt) is not int or not 1 <= attempt <= 100:
            raise ValueError('Use an explicit attempt number from 1 to 100.')
        metadata = candidate.manifest['release_metadata']
        validate_metadata(previous.manifest['release_metadata'], previous.version)
        validate_metadata(metadata, candidate.version)
        compatibility = metadata['compatibility']
        old_metadata = previous.manifest['release_metadata']
        legacy = old_metadata['schema_version'] == 1 and previous.version == '0.1.0-preview.1'
        if not legacy and (old_metadata['schema_version'] != 2 or any(
                old_metadata['compatibility'][key] != 1 for key in ('data_schema', 'configuration_schema'))):
            raise ValueError('Previous data/configuration schema is unsupported; retain the installation and use documented recovery.')
        if (metadata['schema_version'] != 2 or compatibility['installation'] != 'fresh-and-upgrade' or
                compatibility['upgrade_from'] != [previous.version] or
                any(compatibility[key] != 1 for key in ('data_schema', 'configuration_schema')) or
                metadata['migration']['required'] or metadata['recovery']['binary_rollback']):
            raise ValueError('Candidate must explicitly support this preceding preview with unchanged schema 1 and backup/restore recovery.')
        # This path changes the platform version, not proxy/configuration profiles.
        before = {k: v for k, v in previous.output.items() if not k.startswith(previous.release + '/') and k != UNIT}
        after = {k: v for k, v in candidate.output.items() if not k.startswith(candidate.release + '/') and k != UNIT}
        if (before != after or previous.version == candidate.version or
                candidate.output[UNIT][0].replace(candidate.binary.encode(), previous.binary.encode()) != previous.output[UNIT][0]):
            raise ValueError('Proxy or configuration conversion requires a separate upgrade contract.')
        self.plan = {'schema_version': 1, 'previous': previous.identity, 'candidate': candidate.identity, 'attempt': attempt}
        self.id = hashlib.sha256(canonical(self.plan)).hexdigest()
        self.directory = previous.path(STATE + '/upgrades/' + self.id)
        self.journal = self.directory / 'transaction.json'
        self.backup = self.directory / 'backup'
        self.roots = {'platform': previous.path(PLATFORM), 'proxy': previous.path(PROXY)}
        self.configuration = {
            'platform.service': previous.output[UNIT][0],
            'proxy.service': previous.output['/etc/systemd/system/caddy-devoption.service'][0],
            'Caddyfile': previous.output['/etc/caddy/Caddyfile'][0],
            'installation.json': canonical(previous.identity),
        }

    def private(self, path):
        self.old.safe(path)
        info = path.lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_uid != self.host.root_uid or stat.S_IMODE(info.st_mode) != 0o600 or info.st_nlink != 1:
            raise ValueError('Unsafe upgrade record; preserve it for inspection.')
        return snapshot(path, 8 * 1024 * 1024)

    def replace(self, path, content, mode, allowed=None):
        self.old.safe(path)
        if path.exists() or path.is_symlink():
            info = path.lstat()
            if (not stat.S_ISREG(info.st_mode) or info.st_uid != self.host.root_uid or
                    stat.S_IMODE(info.st_mode) != mode or info.st_nlink != 1):
                raise ValueError('Unsafe upgrade destination; never replace it: ' + str(path))
            if allowed is not None and snapshot(path, 8 * 1024 * 1024) not in allowed:
                raise ValueError('Changed upgrade destination; preserve it for inspection.')
        elif allowed is not None:
            raise ValueError('Missing upgrade destination; preserve interrupted state for inspection.')
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix='.upgrade-', delete=False) as stream:
            temporary = Path(stream.name)
            try:
                os.fchmod(stream.fileno(), mode)
                stream.write(content)
                stream.flush()
                os.fsync(stream.fileno())
                os.replace(temporary, path)
                sync_directory(path.parent)
            finally:
                temporary.unlink(missing_ok=True)

    def record(self):
        if not self.journal.exists() and not self.journal.is_symlink():
            return None
        info = self.directory.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != self.host.root_uid or stat.S_IMODE(info.st_mode) != 0o700:
            raise ValueError('Unsafe upgrade transaction directory.')
        data = json.loads(self.private(self.journal), object_pairs_hook=unique_object)
        if (not isinstance(data, dict) or set(data) != {'plan', 'phase', 'backup_sha256'} or
                data['plan'] != self.plan or data['phase'] not in PHASES or
                (data['backup_sha256'] is not None and not re.fullmatch('[0-9a-f]{64}', data['backup_sha256']))):
            raise ValueError('Upgrade journal identity or schema differs; inspect before continuing.')
        return data

    def phase(self, phase, backup_sha256=None):
        before = self.record()
        record = {'plan': self.plan, 'phase': phase,
                  'backup_sha256': backup_sha256 if backup_sha256 is not None else before['backup_sha256'] if before else None}
        self.replace(self.journal, canonical(record), 0o600)

    def managed(self):
        # An interrupted atomic unit/receipt switch may contain either verified
        # identity. Accept no third variant and never execute saved commands.
        receipt = snapshot(self.old.path(RECEIPT), 16384)
        if receipt == canonical(self.old.identity):
            active = self.old
        elif receipt == canonical(self.new.identity):
            active = self.new
        else:
            raise ValueError('Installed receipt differs from both reviewed identities.')
        view = copy.copy(active)
        view.output = {k: v for k, v in self.old.output.items() if k.startswith(self.old.release + '/')}
        view.output.update({k: v for k, v in self.new.output.items() if k.startswith(self.new.release + '/')})
        view.output.update({k: v for k, v in active.output.items() if not k.startswith(active.release + '/')})
        unit = snapshot(self.old.path(UNIT), 65536)
        if unit not in (self.old.output[UNIT][0], self.new.output[UNIT][0]):
            raise ValueError('Platform unit differs from both reviewed identities.')
        view.output[UNIT] = (unit, 0o644)
        view.dirs = self.old.dirs | self.new.dirs
        result = view.preflight()
        for name, (content, mode) in self.old.output.items():
            if name != UNIT and not self.old.existing_file(name, content, mode):
                raise ValueError('Previous installation is incomplete; inspect before upgrading.')
        if active is self.new:
            for name, (content, mode) in self.new.output.items():
                if name != UNIT and not self.new.existing_file(name, content, mode):
                    raise ValueError('Active candidate installation is incomplete; inspect before continuing.')
        return active, result

    def preflight(self, starting=False):
        record = self.record()
        if starting and record is None:
            if self.old.path(RECEIPT).read_bytes() != canonical(self.old.identity):
                raise ValueError('Previous reservation does not identify the installed version.')
        active, result = self.managed()
        if not active.owner():
            raise ValueError('An enrolled owner is required before upgrade.')
        self.base_profile()

        for path in self.roots.values():
            if path.stat().st_dev != self.old.path(STATE).stat().st_dev:
                raise ValueError('Recovery requires platform/proxy state on the installer filesystem; separate mounts are unsupported.')
        if starting and record is None:
            if active is not self.old or self.old.path(UNIT).read_bytes() != self.old.output[UNIT][0]:
                raise ValueError('Fresh upgrade requires the complete previous installation.')
            for unit in ('devoption-public.service', 'caddy-devoption.service'):
                if not self.host.active(unit):
                    raise ValueError('Start and verify the existing platform/proxy before upgrading.')
            self.host.healthy(self.old.hostname)
            self.host.public_health(self.old.hostname)
        records = inventory(self.roots, LIMIT, skip_locks=False)
        total = sum(item.get('size', 0) for item in records.values())
        self.host.preflight(self.root, 1024**3 + 3 * total + 2 * sum(len(v[0]) for v in self.new.output.values()))
        return {'transaction': self.id, 'phase': record['phase'] if record else 'not-started',
                'installed_version': active.version, 'candidate_version': self.new.version,
                'backup_bytes': total, 'publication_eligible': False, 'app_upgrades_supported': False,
                'missing_files': result['missing_files']}

    def base_profile(self):
        self.host.no_app_services()
        for name in ('/etc/devoption', '/etc/systemd/system/devoption-build.service',
                     '/etc/systemd/system/devoption-app.service', '/etc/systemd/system/devoption-app-validate.service'):
            if self.old.path(name).exists() or self.old.path(name).is_symlink():
                raise ValueError('App/build opt-ins need their coordinated upgrade path; this path supports the base bundle installation only.')

    def initialize(self):
        parent = self.old.path(STATE + '/upgrades')
        for directory in (parent,):
            self.old.safe(directory / 'record')
            if not directory.exists():
                directory.mkdir(mode=0o700)
                sync_directory(directory.parent)
            info = directory.lstat()
            if not stat.S_ISDIR(info.st_mode) or info.st_uid != self.host.root_uid or stat.S_IMODE(info.st_mode) != 0o700:
                raise ValueError('Upgrade directories must be private and administrator-owned.')
        entries = list(parent.iterdir())
        if len(entries) > 100:
            raise ValueError('Upgrade history limit reached; archive verified terminal transactions manually.')
        for directory in entries:
            if directory == self.directory:
                continue
            self.old.safe(directory / 'transaction.json')
            if not re.fullmatch('[0-9a-f]{64}', directory.name) or not directory.is_dir() or directory.is_symlink():
                raise ValueError('Unknown upgrade state; inspect before continuing.')
            other = json.loads(self.private(directory / 'transaction.json'), object_pairs_hook=unique_object)
            if other.get('phase') not in TERMINAL:
                raise ValueError('Another upgrade requires recovery before starting a new attempt.')
        if not self.directory.exists():
            self.directory.mkdir(mode=0o700)
            sync_directory(parent)
        info = self.directory.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != self.host.root_uid or stat.S_IMODE(info.st_mode) != 0o700:
            raise ValueError('Unsafe upgrade transaction directory.')
        self.phase('prepared')

    def capture(self):
        # Both writers are confirmed stopped. Never snapshot a live data tree.
        records = inventory(self.roots, LIMIT, skip_locks=False)
        self.backup.mkdir(mode=0o700)
        copy_records(self.roots, self.backup, records)
        copied = {name: self.backup / name for name in self.roots}
        if inventory(copied, LIMIT, skip_locks=False) != records or inventory(self.roots, LIMIT, skip_locks=False) != records:
            raise ValueError('Recovery point differs from stopped live state.')
        config = self.backup / 'configuration'
        config.mkdir(mode=0o700)
        for path in (UNIT, '/etc/systemd/system/caddy-devoption.service', '/etc/caddy/Caddyfile'):
            content, mode = self.old.output[path]
            if not self.old.existing_file(path, content, mode):
                raise ValueError('Configuration changed before the backup was sealed.')
        if not self.old.existing_file(RECEIPT, canonical(self.old.identity), 0o600):
            raise ValueError('Installation receipt disappeared before backup sealing.')
        for name, content in self.configuration.items():
            self.replace(config / name, content, 0o600)
        for path in sorted(self.backup.rglob('*'), reverse=True):
            if path.is_dir():
                sync_directory(path)
        sync_directory(self.backup)
        raw = canonical({'plan': self.plan, 'records': records,
                         'configuration': {name: hashlib.sha256(content).hexdigest()
                                           for name, content in self.configuration.items()}})
        self.replace(self.directory / 'backup.json', raw, 0o600)
        self.phase('backed-up', hashlib.sha256(raw).hexdigest())

    def verified_backup(self):
        raw = self.private(self.directory / 'backup.json')
        record = self.record()
        data = json.loads(raw, object_pairs_hook=unique_object)
        if (set(data) != {'plan', 'records', 'configuration'} or data['plan'] != self.plan or
                (record['backup_sha256'] is not None and hashlib.sha256(raw).hexdigest() != record['backup_sha256'])):
            raise ValueError('Backup identity differs; do not attempt binary-only rollback.')
        expected = {name: hashlib.sha256(content).hexdigest() for name, content in self.configuration.items()}
        if data['configuration'] != expected or any(
                self.private(self.backup / 'configuration' / name) != content for name, content in self.configuration.items()):
            raise ValueError('Backup configuration differs; preserve current state.')
        roots = {name: self.backup / name for name in self.roots}
        records = inventory(roots, LIMIT, skip_locks=False)
        if records != data['records']:
            raise ValueError('Backup contents/permissions differ; do not restore corrupt state.')
        return roots, records

    def switch(self, installer):
        self.replace(self.old.path(UNIT), installer.output[UNIT][0], 0o644,
                     [self.old.output[UNIT][0], self.new.output[UNIT][0]])
        self.replace(self.old.path(RECEIPT), canonical(installer.identity), 0o600,
                     [canonical(self.old.identity), canonical(self.new.identity)])
        self.host.validate(installer.binary, installer.caddy, installer.hostname)
        self.host.reload()

    def apply(self):
        self.preflight(starting=True)
        record = self.record()
        if record:
            if record['phase'] == 'healthy':
                self.host.start()
                self.check(self.new)
                self.host.enable()
                return self.preflight()
            raise ValueError('This attempt already exists; recover it, then select a new --attempt for a fresh backup.')
        self.initialize()
        try:
            self.host.stop()
            self.capture()
            for name, (mode, who) in sorted(self.new.dirs.items(), key=lambda item: len(Path(item[0]).parts)):
                if name.startswith(self.new.release) and not self.new.path(name).exists():
                    self.new.path(name).mkdir(mode=mode)
                    self.new.path(name).chmod(mode)
                    sync_directory(self.new.path(name).parent)
            for name, (content, mode) in self.new.output.items():
                if name.startswith(self.new.release + '/'):
                    self.new.write(name, content, mode)
            self.host.probe(self.new.binary, self.new.version)
            self.switch(self.new)
            self.phase('activated')
            self.host.start()
            self.check(self.new)
            self.phase('healthy')
            self.host.enable()
            return self.preflight()
        except Exception:
            # Never guess that a failed new process is harmless. Recovery is an
            # explicit, offline full-state restore, not an unverified binary swap.
            try:
                self.host.stop()
            finally:
                self.phase('failed')
            raise

    def check(self, installer):
        self.host.probe(installer.binary, installer.version)
        self.host.healthy(installer.hostname)
        self.host.public_health(installer.hostname)

    def recover(self, confirm=False):
        if not confirm:
            raise ValueError('Recovery discards post-backup writes. Select --confirm-discard-post-backup-data after reviewing downtime/data loss.')
        record = self.record()
        if not record:
            raise ValueError('No matching upgrade attempt exists.')
        self.managed()
        self.base_profile()
        if record['phase'] == 'restored':
            self.restart_previous()
            return self.preflight()
        self.host.stop()
        # Before sealing a backup there were no candidate writes to live state.
        if not (self.directory / 'backup.json').exists():
            if record['phase'] not in ('prepared', 'failed') or self.old.path(UNIT).read_bytes() != self.old.output[UNIT][0] or self.old.path(RECEIPT).read_bytes() != canonical(self.old.identity):
                raise ValueError('No sealed backup for changed live state; manual recovery is required.')
            self.switch(self.old)
        else:
            roots, records = self.verified_backup()
            self.host.preflight(self.root, 1024**3 + 2 * sum(item.get('size', 0) for item in records.values()))
            attempts = list(self.directory.glob('restore-*'))
            if len(attempts) >= 4:
                raise ValueError('Recovery retry limit reached; preserve backups and displaced state for manual inspection.')
            self.phase('restoring')
            stage = self.directory / ('restore-' + uuid.uuid4().hex)
            stage.mkdir(mode=0o700)
            copy_records(roots, stage, records)
            restored = {name: stage / name for name in roots}
            if inventory(restored, LIMIT, skip_locks=False) != records:
                raise ValueError('Staged restore differs from the verified backup.')
            for path in sorted(stage.rglob('*'), reverse=True):
                if path.is_dir():
                    sync_directory(path)
            sync_directory(stage)
            for name, destination in self.roots.items():
                self.old.safe(destination)
                # Preserve the displaced tree, including any post-backup writes.
                # Recovery retries never delete unknown partial state.
                if destination.exists() or destination.is_symlink():
                    if not destination.is_dir() or destination.is_symlink():
                        raise ValueError('Unsafe live state destination; manual recovery required.')
                    os.rename(destination, stage / ('displaced-' + name))
                    sync_directory(destination.parent)
                    sync_directory(stage)
                os.rename(stage / name, destination)
                sync_directory(destination.parent)
                sync_directory(stage)
            self.switch(self.old)
        try:
            self.host.start()
            self.check(self.old)
        except Exception:
            self.host.stop()
            raise
        self.phase('restored')
        self.host.enable()
        return self.preflight()

    def restart_previous(self):
        try:
            self.host.start()
            self.check(self.old)
            self.host.enable()
        except Exception:
            self.host.stop()
            raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('plan', 'apply', 'status', 'recover'))
    for label in ('previous', 'candidate'):
        parser.add_argument('--' + label + '-reservation', type=Path, required=True)
        parser.add_argument('--' + label + '-packet', type=Path, required=True)
    parser.add_argument('--gh', type=Path, required=True)
    parser.add_argument('--caddy-archive', type=Path, required=True)
    parser.add_argument('--hostname', required=True)
    parser.add_argument('--attempt', type=int, default=1)
    parser.add_argument('--allow-validation-candidate', action='store_true')
    parser.add_argument('--confirm-discard-post-backup-data', action='store_true')
    args = parser.parse_args()
    try:
        host = UpgradeHost()
        installers = []
        for label in ('previous', 'candidate'):
            record = load(snapshot(getattr(args, label + '_reservation'), 8192))
            manifest, files = verified_files(getattr(args, label + '_packet'), record, args.gh, args.allow_validation_candidate)
            installers.append(Installer(Path('/'), host, record, manifest, files,
                                        proxy_binary(args.caddy_archive, manifest), args.hostname))
        upgrade = Upgrade(*installers, attempt=args.attempt)
        host.preflight(Path('/'), 1024**3)
        with installation_lock('/run/lock/devoption-installer.lock'):
            if args.mode == 'apply':
                result = upgrade.apply()
            elif args.mode == 'recover':
                result = upgrade.recover(args.confirm_discard_post_backup_data)
            else:
                result = upgrade.preflight(starting=args.mode == 'plan')
            print(json.dumps(result, indent=2))
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, f'Upgrade stopped: {error}\nPreserve the transaction/backup; use status and documented recovery before retrying.\n')


if __name__ == '__main__':
    main()
