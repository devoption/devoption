//! Private immutable source staging; no publication, Cargo index or access grant.
use super::{MAX_PACKAGE_BYTES, source_package};
use crate::BuildPlan;
use devoption_registry::{
    CargoDependencyRequest, CargoOrigin, HostedRegistry, LockedPackage, RegistryId, Sha256,
};
use std::fmt;
#[cfg(test)]
use std::{fs::File, io::Write, os::unix::fs::MetadataExt};

const MAGIC: &[u8] = b"DVSTAGE1";
const LOCK: &str = ".source-stage.lock";
const PENDING: &str = ".source-stage.pending";
const MAX_RECORD_BYTES: usize = MAX_PACKAGE_BYTES + 4096;
pub const MAX_STAGED_PACKAGES: usize = super::record_store::MAX_RECORDS;
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
    pub(crate) fn slot(&self) -> String {
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
        Self::from_source(
            registry,
            input.project.to_bytes(),
            input.repository.to_bytes(),
            &input.source,
        )
    }
    pub(crate) fn from_source(
        registry: HostedRegistry,
        project: [u8; 16],
        repository: [u8; 16],
        source: &crate::source_snapshot::SourceSnapshot,
    ) -> Result<Self, Error> {
        let archive = source_package(source).map_err(|_| Error::Invalid)?;
        Ok(Self {
            coordinates: Coordinates::new(registry, source.package(), source.version())?,
            project,
            repository,
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
    pub(super) fn encode(&self) -> Vec<u8> {
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
    pub(super) fn decode(bytes: &[u8]) -> Result<Self, Error> {
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

/// Private source-only store. Existing source record format and refusal semantics
/// remain unchanged; Cargo candidates use a separate directory and record domain.
pub struct SourceStore(super::record_store::RecordStore<StagedSource>);
impl SourceStore {
    pub fn open(path: &str) -> Result<Self, Error> {
        super::record_store::RecordStore::open(path).map(Self)
    }
    pub fn read(&self, coordinates: &Coordinates) -> Result<Option<StagedSource>, Error> {
        self.0.read(coordinates)
    }
    pub fn stage(&mut self, source: &StagedSource) -> Result<StageOutcome, Error> {
        self.0.stage(source)
    }
    #[cfg(test)]
    fn stage_using(
        &mut self,
        source: &StagedSource,
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<StageOutcome, Error> {
        self.0.stage_using(source, write)
    }
    #[cfg(test)]
    fn inventory(&self) -> Result<Vec<String>, Error> {
        self.0.inventory()
    }
}
impl super::record_store::Record for StagedSource {
    type Key = Coordinates;
    const LOCK: &'static str = LOCK;
    const PENDING: &'static str = PENDING;
    const SUFFIX: &'static str = ".source";
    const MAX_BYTES: usize = MAX_RECORD_BYTES;
    fn key_for(coordinates: &Coordinates) -> String {
        coordinates.slot().trim_end_matches(".source").to_owned() + Self::SUFFIX
    }
    fn coordinates(&self) -> &Coordinates {
        &self.coordinates
    }
    fn encode(&self) -> Vec<u8> {
        self.encode()
    }
    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        Self::decode(bytes)
    }
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
            Self::Invalid => "unsupported staged library record",
            Self::Unsafe => "library staging storage is unsafe",
            Self::Busy => "library staging storage is busy",
            Self::Unavailable => "library staging storage is unavailable",
            Self::Integrity => "staged library integrity check failed",
            Self::Conflict => "staged library version is already bound differently",
            Self::Capacity => "library staging capacity exceeded",
            Self::Uncertain => "library staging requires explicit interrupted-write inspection",
        })
    }
}
impl std::error::Error for Error {}

#[cfg(test)]
mod tests;
