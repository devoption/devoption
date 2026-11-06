//! Private immutable source staging; no publication, Cargo index or access grant.
use super::{MAX_PACKAGE_BYTES, source_package};
use crate::BuildPlan;
use devoption_registry::{
    CargoDependencyRequest, CargoOrigin, HostedRegistry, LockedPackage, RegistryId, Sha256,
};
use rustix::{
    fs::{AtFlags, Dir, FlockOperation, Mode, OFlags, flock, linkat, openat, unlinkat},
    io::Errno,
    process::geteuid,
};
use std::{
    fmt,
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
};

const MAGIC: &[u8] = b"DVSTAGE1";
const LOCK: &str = ".source-stage.lock";
const PENDING: &str = ".source-stage.pending";
const MAX_RECORD_BYTES: usize = MAX_PACKAGE_BYTES + 4096;
pub const MAX_STAGED_PACKAGES: usize = 128;
const READ: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

/// Logical destination and exact source version. Names differing only in case or
/// hyphen/underscore, and versions differing only in build metadata, share a slot.
#[derive(Clone, PartialEq, Eq)]
pub struct Coordinates {
    registry: HostedRegistry,
    name: String,
    version: String,
}
impl Coordinates {
    pub fn new(registry: HostedRegistry, name: &str, version: &str) -> Result<Self, Error> {
        CargoDependencyRequest::new(
            registry.id().clone(),
            Sha256::digest(b""),
            Sha256::digest(b""),
            vec![LockedPackage {
                origin: CargoOrigin::DevOption(registry.id().clone()),
                name: name.into(),
                version: version.into(),
                archive_sha256: Sha256::digest(b""),
            }],
        )
        .map_err(|_| Error::Invalid)?;
        Ok(Self {
            registry,
            name: name.into(),
            version: version.into(),
        })
    }
    pub fn registry(&self) -> &HostedRegistry {
        &self.registry
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    fn slot(&self) -> String {
        let mut bytes = Vec::new();
        for value in [
            self.registry.id().as_str(),
            &self.name.to_ascii_lowercase().replace('_', "-"),
            self.version.split('+').next().unwrap(),
        ] {
            string(&mut bytes, value);
        }
        format!("{}.source", hex(Sha256::digest(&bytes)))
    }
}

/// Exact canonical ustar source generated from a structurally valid library plan.
/// Provenance is a source-plan binding, not proof of a successful/approved build.
/// No Debug implementation exposes private coordinates, revision or bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct StagedSource {
    coordinates: Coordinates,
    project: [u8; 16],
    repository: [u8; 16],
    commit: String,
    source: Sha256,
    archive_sha256: Sha256,
    archive: Vec<u8>,
}
impl StagedSource {
    pub fn from_plan(registry: HostedRegistry, plan: &BuildPlan) -> Result<Self, Error> {
        let input = plan.details();
        let source = &input.source;
        let archive = source_package(source).map_err(|_| Error::Invalid)?;
        Ok(Self {
            coordinates: Coordinates::new(registry, source.package(), source.version())?,
            project: input.project.to_bytes(),
            repository: input.repository.to_bytes(),
            commit: source.commit().into(),
            source: source.sha256(),
            archive_sha256: Sha256::digest(&archive),
            archive,
        })
    }
    pub fn coordinates(&self) -> &Coordinates {
        &self.coordinates
    }
    pub fn project(&self) -> [u8; 16] {
        self.project
    }
    pub fn repository(&self) -> [u8; 16] {
        self.repository
    }
    pub fn commit(&self) -> &str {
        &self.commit
    }
    pub fn source_sha256(&self) -> Sha256 {
        self.source
    }
    pub fn archive_sha256(&self) -> Sha256 {
        self.archive_sha256
    }
    pub fn archive(&self) -> &[u8] {
        &self.archive
    }
    pub fn archive_bytes(&self) -> u64 {
        self.archive.len() as u64
    }
    fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        for value in [
            self.coordinates.registry.id().as_str(),
            self.coordinates.registry.index(),
            &self.coordinates.name,
            &self.coordinates.version,
            &self.commit,
        ] {
            string(&mut out, value);
        }
        out.extend(self.project);
        out.extend(self.repository);
        out.extend(self.source.bytes());
        out.extend(self.archive_sha256.bytes());
        out.extend((self.archive.len() as u64).to_be_bytes());
        out.extend(&self.archive);
        out.extend(Sha256::digest(&out).bytes());
        out
    }
    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < 40 || bytes.len() > MAX_RECORD_BYTES {
            return Err(Error::Invalid);
        }
        let (body, checksum) = bytes.split_at(bytes.len() - 32);
        if Sha256::digest(body).bytes() != checksum {
            return Err(Error::Integrity);
        }
        let mut reader = Reader(body);
        if reader.take(8)? != MAGIC {
            return Err(Error::Invalid);
        }
        let id = RegistryId::new(reader.string()?).map_err(|_| Error::Invalid)?;
        let registry = HostedRegistry::new(id, reader.string()?).map_err(|_| Error::Invalid)?;
        let coordinates = Coordinates::new(registry, reader.string()?, reader.string()?)?;
        let commit = reader.string()?.to_owned();
        if commit.len() != 40
            || !commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::Invalid);
        }
        let project = reader.take(16)?.try_into().map_err(|_| Error::Invalid)?;
        let repository = reader.take(16)?.try_into().map_err(|_| Error::Invalid)?;
        let source = Sha256::from_hex(
            &reader
                .take(32)?
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
        )
        .map_err(|_| Error::Invalid)?;
        let archive_sha256 = Sha256::from_hex(
            &reader
                .take(32)?
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
        )
        .map_err(|_| Error::Invalid)?;
        let size = u64::from_be_bytes(reader.take(8)?.try_into().map_err(|_| Error::Invalid)?);
        if size == 0 || size > MAX_PACKAGE_BYTES as u64 {
            return Err(Error::Invalid);
        }
        let archive = reader.take(size as usize)?.to_vec();
        if !reader.0.is_empty() || Sha256::digest(&archive) != archive_sha256 {
            return Err(Error::Integrity);
        }
        Ok(Self {
            coordinates,
            project,
            repository,
            commit,
            source,
            archive_sha256,
            archive,
        })
    }
}
fn hex(value: Sha256) -> String {
    value.bytes().iter().map(|b| format!("{b:02x}")).collect()
}
fn string(out: &mut Vec<u8>, value: &str) {
    out.extend((value.len() as u32).to_be_bytes());
    out.extend(value.as_bytes());
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if n > self.0.len() {
            return Err(Error::Invalid);
        }
        let (value, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(value)
    }
    fn string(&mut self) -> Result<&'a str, Error> {
        let n = u32::from_be_bytes(self.take(4)?.try_into().map_err(|_| Error::Invalid)?) as usize;
        if n > 2048 {
            return Err(Error::Invalid);
        }
        std::str::from_utf8(self.take(n)?).map_err(|_| Error::Invalid)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageOutcome {
    Stored,
    AlreadyPresent,
}

/// Exclusive descriptor-anchored staging store in an existing private directory.
/// The OS owner and local filesystem are trusted. No directory creation, repair,
/// deletion API, publication, registry serving, project grants or network I/O.
pub struct SourceStore {
    directory: File,
    lock: File,
    poisoned: bool,
}
impl Drop for SourceStore {
    fn drop(&mut self) {
        let _ = flock(&self.lock, FlockOperation::Unlock);
    }
}
impl SourceStore {
    pub fn open(path: &str) -> Result<Self, Error> {
        let directory = crate::history::directory(path).map_err(|_| Error::Unsafe)?;
        directory_valid(&directory)?;
        let lock = File::from(
            openat(
                &directory,
                LOCK,
                OFlags::RDWR
                    | OFlags::CREATE
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|_| Error::Unavailable)?,
        );
        file_valid(&lock, 0)?;
        flock(&lock, FlockOperation::NonBlockingLockExclusive).map_err(|e| {
            if e == Errno::WOULDBLOCK {
                Error::Busy
            } else {
                Error::Unavailable
            }
        })?;
        let store = Self {
            directory,
            lock,
            poisoned: false,
        };
        store.inventory()?;
        Ok(store)
    }
    fn check(&self) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Uncertain);
        }
        self.check_storage()
    }
    fn check_storage(&self) -> Result<(), Error> {
        directory_valid(&self.directory)?;
        file_valid(&self.lock, 0)?;
        let current = File::from(
            openat(&self.directory, LOCK, READ, Mode::empty()).map_err(|_| Error::Unsafe)?,
        );
        file_valid(&current, 0)?;
        if identity(&current)? != identity(&self.lock)? {
            return Err(Error::Unsafe);
        }
        Ok(())
    }
    fn inventory(&self) -> Result<Vec<String>, Error> {
        self.check()?;
        let mut names = Vec::new();
        for entry in Dir::read_from(&self.directory).map_err(|_| Error::Unavailable)? {
            let entry = entry.map_err(|_| Error::Unavailable)?;
            let name = entry.file_name().to_str().map_err(|_| Error::Unsafe)?;
            if matches!(name, "." | "..") || name == LOCK {
                continue;
            }
            if name == PENDING {
                return Err(Error::Uncertain);
            }
            if name.len() != 71
                || !name.ends_with(".source")
                || !name.as_bytes()[..64]
                    .iter()
                    .copied()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(Error::Unsafe);
            }
            if names.len() >= MAX_STAGED_PACKAGES {
                return Err(Error::Capacity);
            }
            let package = self.load(name)?.ok_or(Error::Integrity)?;
            if package.coordinates.slot() != name {
                return Err(Error::Integrity);
            }
            names.push(name.to_owned());
        }
        Ok(names)
    }
    fn load(&self, name: &str) -> Result<Option<StagedSource>, Error> {
        let mut file = match openat(&self.directory, name, READ, Mode::empty()) {
            Ok(fd) => File::from(fd),
            Err(Errno::NOENT) => return Ok(None),
            Err(_) => return Err(Error::Unsafe),
        };
        file_valid(&file, MAX_RECORD_BYTES as u64)?;
        let before = file.metadata().map_err(|_| Error::Unavailable)?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_RECORD_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Unavailable)?;
        file_valid(&file, MAX_RECORD_BYTES as u64)?;
        let after = file.metadata().map_err(|_| Error::Unavailable)?;
        if before.len() != after.len()
            || before.mtime() != after.mtime()
            || before.mtime_nsec() != after.mtime_nsec()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
            || bytes.len() as u64 != after.len()
        {
            return Err(Error::Integrity);
        }
        StagedSource::decode(&bytes).map(Some)
    }
    /// None means only that these coordinates are absent. Rebound index identity,
    /// unsafe storage and corrupt bytes fail closed without a fallback lookup.
    pub fn read(&self, coordinates: &Coordinates) -> Result<Option<StagedSource>, Error> {
        self.inventory()?;
        let source = self.load(&coordinates.slot())?;
        if source
            .as_ref()
            .is_some_and(|p| p.coordinates != *coordinates)
        {
            return Err(Error::Conflict);
        }
        Ok(source)
    }
    /// Identical retry is idempotent; every changed byte or provenance at an
    /// occupied version conflicts. An interrupted write blocks further use.
    pub fn stage(&mut self, source: &StagedSource) -> Result<StageOutcome, Error> {
        self.stage_using(source, |file, bytes| file.write_all(bytes))
    }
    fn stage_using(
        &mut self,
        source: &StagedSource,
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<StageOutcome, Error> {
        let inventory = self.inventory()?;
        let name = source.coordinates.slot();
        if let Some(existing) = self.load(&name)? {
            return if existing == *source {
                Ok(StageOutcome::AlreadyPresent)
            } else {
                Err(Error::Conflict)
            };
        }
        if inventory.len() >= MAX_STAGED_PACKAGES {
            return Err(Error::Capacity);
        }
        let bytes = source.encode();
        // Revalidate even internally generated records before the first mutation.
        if StagedSource::decode(&bytes)? != *source {
            return Err(Error::Invalid);
        }
        let mut file = File::from(
            openat(
                &self.directory,
                PENDING,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|_| Error::Uncertain)?,
        );
        self.poisoned = true;
        // Preserve the pending record on every failure. No implicit repair/retry.
        write(&mut file, &bytes).map_err(|_| Error::Uncertain)?;
        file.sync_all().map_err(|_| Error::Uncertain)?;
        file_valid(&file, MAX_RECORD_BYTES as u64).map_err(|_| Error::Uncertain)?;
        self.check_storage().map_err(|_| Error::Uncertain)?;
        if self.load(PENDING).map_err(|_| Error::Uncertain)?.as_ref() != Some(source) {
            return Err(Error::Uncertain);
        }
        let pending = File::from(
            openat(&self.directory, PENDING, READ, Mode::empty()).map_err(|_| Error::Uncertain)?,
        );
        if identity(&pending).map_err(|_| Error::Uncertain)?
            != identity(&file).map_err(|_| Error::Uncertain)?
        {
            return Err(Error::Uncertain);
        }
        // Hard-link creation is atomic and never replaces an occupied destination.
        linkat(
            &self.directory,
            PENDING,
            &self.directory,
            name.as_str(),
            AtFlags::empty(),
        )
        .map_err(|_| Error::Uncertain)?;
        self.directory.sync_all().map_err(|_| Error::Uncertain)?;
        unlinkat(&self.directory, PENDING, AtFlags::empty()).map_err(|_| Error::Uncertain)?;
        self.directory.sync_all().map_err(|_| Error::Uncertain)?;
        self.check_storage().map_err(|_| Error::Uncertain)?;
        if self.load(&name).map_err(|_| Error::Uncertain)?.as_ref() != Some(source) {
            return Err(Error::Uncertain);
        }
        self.poisoned = false;
        Ok(StageOutcome::Stored)
    }
}
fn directory_valid(file: &File) -> Result<(), Error> {
    let m = file.metadata().map_err(|_| Error::Unavailable)?;
    if geteuid().as_raw() == 0
        || !m.is_dir()
        || m.uid() != geteuid().as_raw()
        || m.mode() & 0o7777 != 0o700
        || m.nlink() == 0
    {
        return Err(Error::Unsafe);
    }
    Ok(())
}
fn file_valid(file: &File, max: u64) -> Result<(), Error> {
    let m = file.metadata().map_err(|_| Error::Unavailable)?;
    if !m.is_file()
        || m.uid() != geteuid().as_raw()
        || m.nlink() != 1
        || m.mode() & 0o7777 != 0o600
        || m.len() > max
    {
        return Err(Error::Unsafe);
    }
    Ok(())
}
fn identity(file: &File) -> Result<(u64, u64), Error> {
    let m = file.metadata().map_err(|_| Error::Unavailable)?;
    Ok((m.dev(), m.ino()))
}

/// Fixed diagnostics never include a package, registry URL, path or source bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unsafe,
    Busy,
    Unavailable,
    Integrity,
    Conflict,
    Capacity,
    Uncertain,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "unsupported staged source record",
            Self::Unsafe => "source staging storage is unsafe",
            Self::Busy => "source staging storage is busy",
            Self::Unavailable => "source staging storage is unavailable",
            Self::Integrity => "staged source integrity check failed",
            Self::Conflict => "staged source version is already bound differently",
            Self::Capacity => "source staging capacity exceeded",
            Self::Uncertain => "source staging requires explicit interrupted-write inspection",
        })
    }
}
impl std::error::Error for Error {}

#[cfg(test)]
mod tests;
