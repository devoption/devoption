"""Bounded Unix file handoff for disposable rehearsal fixtures, not a runner.

The trusted caller must first terminate the runner's entire cgroup, prevent any
restart/writer, and retain the output filesystem. This module cannot establish
termination or grant build authority. It accepts pinned sibling directories under
one private supervisor-owned parent, accounts for every source hard link, and
copies bytes into a new private inode. It never executes or exports that inode.
"""

from contextlib import contextmanager, ExitStack
from dataclasses import dataclass
import hashlib
import os
import stat

from rehearsal_artifact_metadata import MAX_ARTIFACT_BYTES, describe_artifact_metadata


ARTIFACT_NAME = "artifact"
CHUNK_BYTES = 64 * 1024
MAX_COMPONENTS = 16
MAX_PATH_BYTES = 1024
_DIRECTORY_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
_READ_FLAGS = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC


class HandoffError(Exception):
    """Fixed diagnostics exclude private paths and underlying OS error text."""


def _require(condition, message):
    if not condition:
        raise HandoffError(message)


def _parts(path):
    _require(isinstance(path, str) and path.isascii() and 0 < len(path) <= MAX_PATH_BYTES,
             "invalid relative artifact path")
    parts = path.split("/")
    _require(len(parts) <= MAX_COMPONENTS and all(
        part not in ("", ".", "..") and len(os.fsencode(part)) <= 255
        and not any(ord(char) < 32 or ord(char) == 127 for char in part)
        for part in parts), "invalid relative artifact path")
    return parts


def _identity(metadata):
    return metadata.st_dev, metadata.st_ino


def _binding(metadata):
    return (*_identity(metadata), metadata.st_mode, metadata.st_uid, metadata.st_gid)


def _version(metadata):
    return (*_binding(metadata), metadata.st_nlink, metadata.st_size,
            metadata.st_mtime_ns, metadata.st_ctime_ns)


def _private_directory(metadata, uid, writable=False):
    mode = stat.S_IMODE(metadata.st_mode)
    _require(stat.S_ISDIR(metadata.st_mode) and metadata.st_nlink > 0
             and metadata.st_uid == uid and mode in ((0o700,) if writable else (0o500, 0o700)),
             "directory is not private and owned as expected")


class PinnedDirectory:
    """Pinned child and parent descriptors; not a proof of runner termination.

    Pin output and staging before handoff; replacements after pinning are errors.
    The caller supplies a trusted, already-open private parent directory handle.
    Use as a context manager. Ownership of the supplied parent handle is retained
    by the caller. No directory is created or repaired here.
    """

    def __init__(self, parent_fd, name, owner_uid):
        _require(len(_parts(name)) == 1, "invalid directory name")
        self.name, self.owner_uid = name, owner_uid
        self.fd = self.parent_fd = None
        try:
            self.parent_fd = os.dup(parent_fd)
            parent = os.fstat(self.parent_fd)
            _private_directory(parent, os.geteuid(), writable=True)
            self.parent_binding = _binding(parent)
            before = os.stat(name, dir_fd=self.parent_fd, follow_symlinks=False)
            _private_directory(before, owner_uid)
            self.fd = os.open(name, _DIRECTORY_FLAGS, dir_fd=self.parent_fd)
            opened = os.fstat(self.fd)
            _require(_version(opened) == _version(before), "directory changed while opening")
            _require(opened.st_dev == parent.st_dev, "directory crosses a filesystem boundary")
            self.binding = _binding(opened)
        except BaseException as error:
            self.close()
            if isinstance(error, OSError):
                raise HandoffError("cannot pin directory") from None
            raise

    def verify(self):
        parent = os.fstat(self.parent_fd)
        _private_directory(parent, os.geteuid(), writable=True)
        _require(_binding(parent) == self.parent_binding, "parent directory changed")
        opened = os.fstat(self.fd)
        named = os.stat(self.name, dir_fd=self.parent_fd, follow_symlinks=False)
        _private_directory(opened, self.owner_uid)
        _require(_binding(opened) == self.binding == _binding(named), "pinned directory replaced")

    def close(self):
        for name in ("fd", "parent_fd"):
            fd = getattr(self, name)
            if fd is not None:
                os.close(fd)
                setattr(self, name, None)

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def _open_source(root, parts, stack, guards):
    parent = root.fd
    for index, name in enumerate(parts):
        before = os.stat(name, dir_fd=parent, follow_symlinks=False)
        leaf = index == len(parts) - 1
        if leaf:
            _require(stat.S_ISREG(before.st_mode) and before.st_uid == root.owner_uid
                     and stat.S_IMODE(before.st_mode) & 0o7077 == 0
                     and 0 < before.st_size <= MAX_ARTIFACT_BYTES,
                     "source is not a bounded private regular file")
        else:
            _private_directory(before, root.owner_uid)
        _require(before.st_dev == root.binding[0], "source crosses a filesystem boundary")
        fd = os.open(name, _READ_FLAGS if leaf else _DIRECTORY_FLAGS, dir_fd=parent)
        stack.callback(os.close, fd)
        _require(_version(os.fstat(fd)) == _version(before), "source changed while opening")
        guards.append((fd, parent, name, _version(before)))
        if leaf:
            return fd, before, (*_identity(os.fstat(parent)), name)
        parent = fd


def _verify_sources(root, root_version, guards):
    root.verify()
    _require(_version(os.fstat(root.fd)) == root_version, "source root changed")
    for fd, parent, name, version in guards:
        _require(_version(os.fstat(fd)) == version and _version(os.stat(
            name, dir_fd=parent, follow_symlinks=False)) == version, "source path or bytes changed")


def _destination(staging, fd, identity, size, mode):
    staging.verify()
    _private_directory(os.fstat(staging.fd), os.geteuid(), writable=True)
    metadata = os.fstat(fd)
    _require(_identity(metadata) == identity and metadata.st_uid == os.geteuid()
             and stat.S_IMODE(metadata.st_mode) == mode and metadata.st_size == size
             and describe_artifact_metadata(metadata)["metadata_accepted"],
             "destination failed private single-link validation")
    named = os.stat(ARTIFACT_NAME, dir_fd=staging.fd, follow_symlinks=False)
    _require(_version(named) == _version(metadata), "destination replaced")
    with os.scandir(staging.fd) as entries:
        first, extra = next(entries, None), next(entries, None)
        _require(first is not None and first.name == ARTIFACT_NAME and extra is None,
                 "staging directory changed")
    return metadata


def _write_all(fd, data):
    view = memoryview(data)
    while view:
        written = os.write(fd, view)
        _require(written > 0, "destination write made no progress")
        view = view[written:]


@dataclass(frozen=True)
class HandoffReceipt:
    bytes: int
    sha256: str
    source_links: int


def _remove_staged(staging, output):
    try:
        named = os.stat(ARTIFACT_NAME, dir_fd=staging.fd, follow_symlinks=False)
        _require(_identity(named) == _identity(os.fstat(output)),
                 "staging cleanup conflict; quarantine required")
        os.unlink(ARTIFACT_NAME, dir_fd=staging.fd)
    except OSError:
        raise HandoffError("staging cleanup failed; quarantine required") from None


@contextmanager
def handoff_stopped_artifact(source, source_paths, staging):
    """Yield a bounded hash receipt while the private staged file exists.

    Preconditions: all runner writers are terminated and excluded; source and
    staging are pinned siblings within the same quota-controlled filesystem.
    Supply one or two exact, independently selected source names. They must name
    the same inode and account for ALL hard links; no traversal/glob discovery.
    The destination remains regular, private and single-linked. It is removed on
    exit. An observed destination replacement is preserved: the caller must
    quarantine its owned staging tree after a cleanup conflict. Exclusive trusted
    staging access is required throughout, including named-entry cleanup.
    No receipt authorizes a job.
    """
    with ExitStack() as stack:
        try:
            source.verify()
            staging.verify()
            _require(source.parent_binding == staging.parent_binding
                     and source.binding[:2] != staging.binding[:2], "directories must be distinct siblings")
            _require(staging.owner_uid == os.geteuid(), "staging owner is not the supervisor")
            _private_directory(os.fstat(staging.fd), os.geteuid(), writable=True)
            _require(isinstance(source_paths, tuple) and 1 <= len(source_paths) <= 2,
                     "expected one or two exact source names")
            paths = [_parts(path) for path in source_paths]
            with os.scandir(staging.fd) as entries:
                _require(next(entries, None) is None, "staging directory is not empty")
            root_version = _version(os.fstat(source.fd))
            guards = []
            files = [_open_source(source, path, stack, guards) for path in paths]
            first_fd, before, _ = files[0]
            _require(len({entry for _, _, entry in files}) == len(files)
                     and before.st_nlink == len(files)
                     and all(_version(meta) == _version(before) for _, meta, _ in files),
                     "source names do not account for every hard link")
            _verify_sources(source, root_version, guards)
            output = os.open(ARTIFACT_NAME, os.O_CREAT | os.O_EXCL | os.O_RDWR
                             | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC,
                             0o600, dir_fd=staging.fd)
            # LIFO cleanup keeps the created inode pinned until its name is
            # checked/removed, and always closes it even on cleanup failure.
            stack.callback(os.close, output)
            stack.callback(_remove_staged, staging, output)
            created = _identity(os.fstat(output))
            _require(created != _identity(before), "destination is not independent")
            os.fchmod(output, 0o600)
            copied = 0
            while copied < before.st_size:
                block = os.read(first_fd, min(CHUNK_BYTES, before.st_size - copied))
                _require(bool(block), "source shortened during copy")
                _write_all(output, block)
                copied += len(block)
            _require(not os.read(first_fd, 1), "source grew during copy")
            _verify_sources(source, root_version, guards)
            _destination(staging, output, created, copied, 0o600)
            os.fchmod(output, 0o400)
            os.fsync(output)
            ready = _destination(staging, output, created, copied, 0o400)
            # Hash only the validated destination, through its existing handle.
            os.lseek(output, 0, os.SEEK_SET)
            digest, hashed = hashlib.sha256(), 0
            while hashed < copied:
                block = os.read(output, min(CHUNK_BYTES, copied - hashed))
                _require(bool(block), "destination shortened while hashing")
                digest.update(block)
                hashed += len(block)
            _require(not os.read(output, 1), "destination grew while hashing")
            _require(_version(_destination(staging, output, created, copied, 0o400))
                     == _version(ready), "destination changed while hashing")
            _verify_sources(source, root_version, guards)
            receipt = HandoffReceipt(copied, digest.hexdigest(), before.st_nlink)
        except OSError:
            raise HandoffError("filesystem handoff failed") from None
        # Caller exceptions are not helper filesystem failures. ExitStack still
        # performs ownership-checked cleanup and closes every retained handle.
        yield receipt
