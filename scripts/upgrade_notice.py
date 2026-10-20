"""Public, root-owned upgrade observations. Never store inputs, credentials or commands."""

import os
import json
from pathlib import Path
import stat
import tempfile
import time

from recovery_snapshot import sync_directory
from release_bundle import canonical
from verify_bundle_provenance import snapshot, unique_object

NOTICE = '/var/lib/devoption-updates/notice.json'
PHASES = {'not-started', 'prepared', 'backed-up', 'activated', 'healthy', 'failed', 'restoring', 'restored'}


def publish(installer, candidate, transaction, attempt, phase):
    if phase not in PHASES:
        raise ValueError('Unsupported upgrade notice phase.')
    path = installer.path(NOTICE)
    installer.safe(path)
    if not path.parent.exists():
        path.parent.mkdir(mode=0o755)
        path.parent.chmod(0o755)
        sync_directory(path.parent.parent)
    info = path.parent.lstat()
    if (not stat.S_ISDIR(info.st_mode) or info.st_uid != installer.host.root_uid or
            stat.S_IMODE(info.st_mode) != 0o755):
        raise ValueError('Upgrade notices require an administrator-owned directory with mode 0755.')
    if path.exists() or path.is_symlink():
        info = path.lstat()
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != installer.host.root_uid or
                info.st_nlink != 1 or stat.S_IMODE(info.st_mode) != 0o644):
            raise ValueError('Unsafe upgrade notice; preserve it for inspection.')
        # Do not clobber an unrelated file in the reserved location.
        raw = snapshot(path, 8192)
        before = json.loads(raw, object_pairs_hook=unique_object)
        keys = {'schema_version', 'previous_version', 'candidate_version', 'candidate_archive_sha256',
                'candidate_source_commit', 'transaction', 'attempt', 'phase', 'checked_at', 'publication_eligible'}
        if (not isinstance(before, dict) or set(before) != keys or canonical(before) != raw or
                type(before.get('schema_version')) is not int or before['schema_version'] != 1 or
                before.get('publication_eligible') is not False or before.get('phase') not in PHASES):
            raise ValueError('Unrecognized upgrade notice; preserve it for inspection.')
    value = {'schema_version': 1, 'previous_version': installer.version,
             'candidate_version': candidate.version,
             'candidate_archive_sha256': candidate.record['candidate']['archive_sha256'],
             'candidate_source_commit': candidate.record['candidate']['source_commit'],
             'transaction': transaction, 'attempt': attempt, 'phase': phase,
             'checked_at': int(time.time()), 'publication_eligible': False}
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix='.notice-', delete=False) as stream:
        temporary = Path(stream.name)
        try:
            stream.write(canonical(value))
            stream.flush()
            os.fchmod(stream.fileno(), 0o644)
            os.fsync(stream.fileno())
            os.replace(temporary, path)
            sync_directory(path.parent)
        finally:
            temporary.unlink(missing_ok=True)
