#!/usr/bin/env python3
"""Bounded offline record snapshots for same-installation recovery; never installs or starts."""
import argparse
from contextlib import ExitStack
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tomllib
from deployment_broker import deployment_slots

LIMIT = 256 * 1024 * 1024
ENTRIES = 20000
EXTRA_LOCKS = {'owner': ('.project-secrets.lock',)}
LOCKS = {'owner': '.projects.lock', 'history': 'build-history.lock', 'jobs': 'execution.lock', 'releases': 'broker.lock'}


def canonical(path):
    path = Path(path)
    if not path.is_absolute() or path.resolve(strict=True) != path:
        raise ValueError('existing canonical absolute path required')
    return path


def metadata(path):
    info = path.lstat()
    if not (stat.S_ISREG(info.st_mode) or stat.S_ISDIR(info.st_mode)) or (stat.S_ISREG(info.st_mode) and info.st_nlink != 1):
        raise ValueError('links and special files are unsupported')
    mode = stat.S_IMODE(info.st_mode)
    if mode & 0o7000:
        raise ValueError('special permission bits are unsupported')
    return dict(kind='directory' if stat.S_ISDIR(info.st_mode) else 'file', mode=mode, uid=info.st_uid, gid=info.st_gid)


def digest(path, limit=LIMIT):
    before = path.stat()
    if before.st_size > limit:
        raise ValueError('file exceeds snapshot limit')
    result = hashlib.sha256()
    count = 0
    with path.open('rb') as stream:
        while chunk := stream.read(min(1024 * 1024, limit + 1 - count)):
            count += len(chunk)
            if count > limit:
                raise ValueError('file grew beyond snapshot limit')
            result.update(chunk)
    after = path.stat()
    if (before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
        raise ValueError('file changed during snapshot')
    return dict(size=after.st_size, sha256=result.hexdigest())


def inventory(roots, limit, skip_locks=True):
    records = {}
    total = 0
    def visit(path, name):
        nonlocal total
        if len(records) >= ENTRIES:
            raise ValueError('snapshot entry limit exceeded')
        record = metadata(path)
        if '.pending' in path.name or path.name.endswith('.tmp'):
            raise ValueError('pending state requires operator inspection')
        if record['kind'] == 'file':
            record.update(digest(path, limit))
            total += record['size']
            if total > limit:
                raise ValueError('snapshot byte limit exceeded')
        records[name] = record
        if record['kind'] == 'directory':
            for child in sorted(path.iterdir()):
                if skip_locks and name in LOCKS and child.name in (LOCKS[name], *EXTRA_LOCKS.get(name, ())):
                    metadata(child)
                    continue
                visit(child, name + '/' + child.name)
    for name, path in roots.items():
        if metadata(path)['kind'] != 'directory':
            raise ValueError('snapshot roots must be directories')
        visit(path, name)
    return records


def stopped(unit):
    if not re.fullmatch(r'[A-Za-z0-9_-]+\.service', unit):
        raise ValueError('explicit system service name required')
    result = subprocess.run(['/usr/bin/systemctl', 'show', unit, '--property=ActiveState,ControlGroup'],
                            check=True, capture_output=True, text=True, timeout=5)
    values = dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)
    if values.get('ActiveState') not in ('inactive', 'failed') or values.get('ControlGroup') != '':
        raise ValueError('stop platform and all workers before snapshot: ' + unit)


def assert_stopped(unit, deployment=None):
    names = [unit, 'devoption-build.service', 'devoption-app.service', 'devoption-app-validate.service']
    if deployment and 'apps' in deployment:
        deployment_slots(deployment)
        for app in deployment['apps']:
            names.extend('devoption-app-'+app['project']+suffix+'.service' for suffix in ('', '-validate'))
    for name in names:
        stopped(name)


def references(paths):
    result = {}
    for name, path in paths.items():
        path = canonical(path)
        record = metadata(path)
        record['path'] = str(path)
        if record['kind'] == 'file':
            record.update(digest(path))
        else:
            info = path.stat()
            record.update(device=info.st_dev, inode=info.st_ino)
        result[name] = record
    return result


def copy_records(roots, output, records):
    # Output is a new private directory. No archive extraction or caller paths
    # from a manifest are used: records are regenerated from the verified tree.
    for name, record in records.items():
        label, _, relative = name.partition('/')
        source = roots[label] / relative if relative else roots[label]
        target = output / name
        if record['kind'] == 'directory':
            target.mkdir(mode=0o700)
        else:
            with source.open('rb') as src, target.open('xb') as dst:
                remaining = record['size']
                while remaining:
                    chunk = src.read(min(1024 * 1024, remaining))
                    if not chunk:
                        raise ValueError('source shrank during copy')
                    dst.write(chunk); remaining -= len(chunk)
                if src.read(1):
                    raise ValueError('source grew during copy')
                dst.flush(); os.fsync(dst.fileno())
        if os.geteuid() == 0:
            os.chown(target, record['uid'], record['gid'])
        os.chmod(target, record['mode'])


def sync_directory(path):
    fd = os.open(path, os.O_RDONLY)
    try: os.fsync(fd)
    finally: os.close(fd)


def create(build_path, deployment_path, binary, output, unit, revision, limit):
    if not re.fullmatch(r'[0-9a-f]{40}', revision):
        raise ValueError('record the reviewed full source commit')
    if not 1 <= limit <= LIMIT:
        raise ValueError('byte limit must be between 1 and 256 MiB')
    build_path, deployment_path = canonical(build_path), canonical(deployment_path)
    for path in (build_path, deployment_path):
        if metadata(path)['kind'] != 'file':
            raise ValueError('regular configuration files required')
        digest(path, 65536)
    build = tomllib.loads(build_path.read_text())
    deployment = tomllib.loads(deployment_path.read_text())
    for key in ('uid', 'owner_data', 'history', 'repositories'):
        if build[key] != deployment[key]:
            raise ValueError('build/deployment configuration mismatch')
    if build['work'] != deployment['build_work']:
        raise ValueError('build/deployment work mismatch')
    roots = dict(owner=canonical(build['owner_data']), history=canonical(build['history']),
                 jobs=canonical(build['work']), releases=canonical(deployment['state']))
    if type(build['uid']) is not int or build['uid'] <= 0:
        raise ValueError('configured nonroot platform UID required')
    for name, path in roots.items():
        info = metadata(path)
        expected_uid = os.geteuid() if name == 'releases' else build['uid']
        if info != dict(kind='directory', mode=0o700, uid=expected_uid, gid=info['gid']):
            raise ValueError('snapshot roots must retain private installed ownership')
    paths = list(roots.values())
    if any(a == b or a in b.parents or b in a.parents for i, a in enumerate(paths) for b in paths[i+1:]):
        raise ValueError('snapshot roots must be disjoint')
    output = Path(output).absolute()
    if output.name in ('.', '..') or output.parent.resolve(strict=True) != output.parent:
        raise ValueError('output requires an existing canonical parent')
    if any(output == p or p in output.parents or output in p.parents for p in paths):
        raise ValueError('snapshot output must be separate from source state')
    external = dict(binary=binary, build_config=build_path, deployment_config=deployment_path,
                    worker=deployment['worker'], repositories=build['repositories'], toolchain=build['toolchain'], vendor=build['vendor'],
                    build_broker='/usr/local/libexec/devoption-build-broker', build_worker='/usr/local/libexec/devoption-build-worker',
                    deployment_broker='/usr/local/libexec/devoption-deployment-broker')
    if 'ca_file' in deployment.get('ingress', {}):
        external['rehearsal_ca'] = deployment['ingress']['ca_file']
    slots = deployment_slots(deployment) if 'apps' in deployment else [deployment]
    for slot in slots:
        if 'apps' in deployment and 'ca_file' in slot['ingress']:
            external['rehearsal_ca_'+slot['ingress']['project']] = slot['ingress']['ca_file']
    assert_stopped(unit, deployment)
    with ExitStack() as stack:
        for name, root in roots.items():
            for filename in (LOCKS[name], *EXTRA_LOCKS.get(name, ())):
                lock = root / filename
                if lock.exists() or lock.is_symlink():
                    if metadata(lock)['kind'] != 'file':
                        raise ValueError('invalid writer lock')
                    stream = stack.enter_context(lock.open('rb'))
                    fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if not (roots['owner'] / 'owner.bin').is_file():
            raise ValueError('existing owner required')
        for slot in slots:
            intent = Path(slot['state']) / 'desired.json'
            if intent.exists() or intent.is_symlink():
                metadata(intent); digest(intent, 16384)
            if intent.exists() and json.loads(intent.read_text()).get('intent') != 'stopped':
                raise ValueError('use Stop app before snapshot; running intent is not backed up')
        before = inventory(roots, limit)
        refs = references(external)
        if any(output == Path(ref['path']) or Path(ref['path']) in output.parents for ref in refs.values()):
            raise ValueError('snapshot must be outside referenced installation paths')
        output.mkdir(mode=0o700)
        payload = output / 'payload'; payload.mkdir(mode=0o700)
        copy_records(roots, payload, before)
        if inventory(roots, limit) != before or inventory({k: payload/k for k in roots}, limit) != before or references(external) != refs:
            raise ValueError('state changed or copy verification failed; retain incomplete snapshot for inspection')
        assert_stopped(unit, deployment)
        manifest = dict(version=1, revision=revision, limit=limit, roots={k: str(v) for k,v in roots.items()}, references=refs, records=before)
        for directory in [p for p in payload.rglob('*') if p.is_dir()] + [payload]:
            sync_directory(directory)
        encoded = json.dumps(manifest, sort_keys=True).encode()
        if len(encoded) > 8 * 1024 * 1024:
            raise ValueError('snapshot manifest exceeds metadata limit')
        with (output/'manifest.json').open('xb') as stream:
            stream.write(encoded); stream.flush(); os.fsync(stream.fileno())
        sync_directory(output)
        sync_directory(output.parent)


def verify(snapshot):
    snapshot = canonical(snapshot)
    info = snapshot.stat()
    if info.st_uid != os.geteuid() or stat.S_IMODE(info.st_mode) != 0o700:
        raise ValueError('snapshot must be private and owned by the operator')
    if set(p.name for p in snapshot.iterdir()) != {'manifest.json', 'payload'}:
        raise ValueError('incomplete or unexpected snapshot contents')
    manifest_path = snapshot/'manifest.json'
    metadata(manifest_path)
    if manifest_path.stat().st_size > 8 * 1024 * 1024:
        raise ValueError('oversized snapshot manifest')
    manifest = json.loads(manifest_path.read_text())
    if manifest['version'] != 1 or set(manifest['roots']) != set(LOCKS) or type(manifest['limit']) is not int or not 1 <= manifest['limit'] <= LIMIT:
        raise ValueError('unsupported snapshot')
    payload = canonical(snapshot/'payload')
    if set(p.name for p in payload.iterdir()) != set(LOCKS):
        raise ValueError('incomplete snapshot roots')
    roots = {name: payload/name for name in LOCKS}
    records = inventory(roots, manifest['limit'], skip_locks=False)
    if records != manifest['records']:
        raise ValueError('snapshot checksum or metadata mismatch')
    return manifest, roots, records


def stage(snapshot, output):
    manifest, roots, records = verify(snapshot)
    refs = manifest['references']
    if references({name: record['path'] for name, record in refs.items()}) != refs:
        raise ValueError('original installation changed; inspect before same-node recovery')
    output = Path(output).absolute()
    if output.name in ('.', '..') or output.parent.resolve(strict=True) != output.parent:
        raise ValueError('output requires an existing canonical parent')
    source = canonical(snapshot)
    original_roots = [Path(p) for p in manifest['roots'].values()]
    original_roots += [Path(ref['path']) for ref in refs.values()]
    if any(output == p or p in output.parents or output in p.parents for p in [source, *original_roots]):
        raise ValueError('restore must be separate from snapshot and original state')
    output.mkdir(mode=0o700)
    copy_records(roots, output, records)
    if inventory({k: output/k for k in roots}, manifest['limit']) != records:
        raise ValueError('staged restore verification failed')
    for directory in [p for p in output.rglob('*') if p.is_dir()] + [output]:
        sync_directory(directory)
    sync_directory(output.parent)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='operation', required=True)
    create_parser = sub.add_parser('create')
    for arg in ('build-config', 'deployment-config', 'binary', 'output', 'platform-unit', 'source-commit'):
        create_parser.add_argument('--'+arg, required=True)
    create_parser.add_argument('--max-bytes', type=int, default=64*1024*1024)
    for name in ('verify', 'stage'):
        child = sub.add_parser(name); child.add_argument('--snapshot', type=Path, required=True)
        if name == 'stage': child.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if os.geteuid() != 0:
        parser.error('run as the node administrator; snapshots include root-owned release state')
    os.umask(0o077)
    try:
        if args.operation == 'create':
            create(args.build_config, args.deployment_config, args.binary, args.output, args.platform_unit, args.source_commit, args.max_bytes)
        elif args.operation == 'verify': verify(args.snapshot)
        else: stage(args.snapshot, args.output)
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, f'Recovery operation failed: {error}\n')
    print('Snapshot operation verified. No installed state changed and no service started.')


if __name__ == '__main__': main()
