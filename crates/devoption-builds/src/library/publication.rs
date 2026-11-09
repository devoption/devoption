//! Owner-only installation-local publication policy for a trusted adapter.
//! Caller IDs are not authentication. No HTTP, credentials, upstream or execution.
use super::{
    cargo_package::CargoPackage,
    package_store::PackageStore,
    record_store::{Record, RecordStore},
    staging::{Coordinates, Error as StorageError, StageOutcome},
};
use crate::approvals::OwnerId;
use devoption_projects::{Catalog, ProjectId, RepositoryId};
use devoption_registry::{HostedRegistry, RegistryId, Sha256};
use serde_json::{Value, json};
use std::{
    fmt,
    time::{Duration, Instant},
};

pub const MAX_POLICY_EVENTS: usize = super::record_store::MAX_RECORDS;
pub const REVIEW_LIFETIME: Duration = Duration::from_secs(300);
const MAX_EVENT_BYTES: usize = 16384;

/// Explicit owner actions. The source project always retains access. Changes
/// affect future reads; they cannot retract bytes already acquired by a consumer.
#[derive(Clone, PartialEq, Eq)]
pub enum Change {
    Publish { readers: Vec<ProjectId> },
    Readers { readers: Vec<ProjectId> },
    Yank { yanked: bool },
}
/// Process-local, non-cloneable single-use review; never a credential. A trusted
/// adapter must authenticate the approval action and supply fresh catalog state.
/// ```compile_fail
/// use devoption_builds::library::publication::Review;
/// fn forged() -> Review { Review {} }
/// ```
pub struct Review {
    event: Event,
    catalog: Catalog,
    owner: OwnerId,
    issued: Instant,
    ledger: [(u64, u64); 2],
    package: CargoPackage,
}
impl Review {
    /// Private reviewed bytes/metadata for a trusted owner adapter. Do not render
    /// source archives into browser responses merely to show a review summary.
    pub fn candidate(&self) -> &CargoPackage {
        &self.package
    }
    pub fn coordinates(&self) -> &Coordinates {
        &self.event.coordinates
    }
    pub fn change(&self) -> &Change {
        &self.event.change
    }
    /// Complete candidate identity includes archive/index/source/provenance bytes.
    pub fn candidate_sha256(&self) -> Sha256 {
        self.event.candidate
    }
    pub fn sequence(&self) -> u64 {
        self.event.sequence
    }
    /// Remaining event slots after this action. Zero disables artifact reads
    /// until separately reviewed ledger maintenance, preserving revocability.
    pub fn remaining_capacity(&self) -> usize {
        MAX_POLICY_EVENTS.saturating_sub(self.event.sequence as usize + 1)
    }
}
/// Owner-visible observations only; none of these fields is read authority.
/// No Debug implementation exposes private coordinates or project identities.
pub struct Status {
    pub coordinates: Coordinates,
    pub candidate_sha256: Sha256,
    pub source: ProjectId,
    pub repository: RepositoryId,
    pub commit: String,
    pub source_sha256: Sha256,
    pub archive_sha256: Sha256,
    pub archive_bytes: u64,
    pub readers: Vec<ProjectId>,
    pub yanked: bool,
    pub events_used: usize,
    pub event_capacity: usize,
}
#[derive(Clone, PartialEq, Eq)]
struct Event {
    sequence: u64,
    previous: Sha256,
    coordinates: Coordinates,
    candidate: Sha256,
    owner: OwnerId,
    source: ProjectId,
    repository: RepositoryId,
    change: Change,
}
#[derive(Clone)]
struct Version {
    coordinates: Coordinates,
    candidate: Sha256,
    source: ProjectId,
    repository: RepositoryId,
    readers: Vec<ProjectId>,
    yanked: bool,
}
struct State {
    events: Vec<Event>,
    versions: Vec<Version>,
}
impl State {
    fn head(&self) -> Sha256 {
        self.events
            .last()
            .map_or(Sha256::digest(b"devoption.registry-policy.v1"), |e| {
                Sha256::digest(&e.encode())
            })
    }
    fn find(&self, coordinates: &Coordinates) -> Option<&Version> {
        let slot = coordinates.slot();
        self.versions.iter().find(|v| v.coordinates.slot() == slot)
    }
}

/// Separate private append-only ledger. OS owner and local filesystem are trusted.
/// No service opens it automatically. An exact configured owner is required for
/// every operation; adapters must never accept that identity from client input.
pub struct Publications {
    store: RecordStore<Event>,
    owner: OwnerId,
}
impl Publications {
    pub fn open(path: &str, owner: OwnerId) -> Result<Self, Error> {
        let result = Self {
            store: RecordStore::open(path)?,
            owner,
        };
        result.state()?;
        Ok(result)
    }
    fn actor(&self, actor: OwnerId) -> Result<(), Error> {
        if actor == self.owner {
            Ok(())
        } else {
            Err(Error::Denied)
        }
    }
    fn state(&self) -> Result<State, Error> {
        state_from(self.store.records()?, self.owner)
    }
    /// Preparation reads the exact retained candidate and checks current catalog
    /// scope; it does not publish. A review binds the entire supplied catalog,
    /// current ledger head, candidate digest, operation and full reader set.
    pub fn review(
        &self,
        actor: OwnerId,
        catalog: &Catalog,
        candidates: &PackageStore,
        coordinates: &Coordinates,
        change: Change,
    ) -> Result<Review, Error> {
        self.actor(actor)?;
        let state = self.state()?;
        let package = candidates.read(coordinates)?.ok_or(Error::Unavailable)?;
        let source = ProjectId::from_bytes(package.source().project());
        let repository = RepositoryId::from_bytes(package.source().repository());
        check_source(catalog, source, repository)?;
        let change = normalize(change, catalog, source)?;
        let event = Event {
            sequence: state.events.len() as u64,
            previous: state.head(),
            coordinates: coordinates.clone(),
            candidate: Sha256::digest(&package.encode()),
            owner: self.owner,
            source,
            repository,
            change,
        };
        let mut versions = state.versions;
        apply(&mut versions, &event)?;
        if event.sequence >= super::record_store::MAX_RECORDS as u64 {
            return Err(Error::Capacity);
        }
        Ok(Review {
            event,
            catalog: catalog.clone(),
            owner: actor,
            issued: Instant::now(),
            ledger: self.store.binding()?,
            package,
        })
    }
    /// Explicit approval consumes the review by value, including on failure.
    /// The authenticated actor and all supplied current observations are checked
    /// again immediately before the immutable event append.
    pub fn approve(
        &mut self,
        actor: OwnerId,
        catalog: &Catalog,
        candidates: &PackageStore,
        review: Review,
    ) -> Result<(), Error> {
        self.approve_at(actor, catalog, candidates, review, Instant::now())
    }
    fn approve_at(
        &mut self,
        actor: OwnerId,
        catalog: &Catalog,
        candidates: &PackageStore,
        review: Review,
        now: Instant,
    ) -> Result<(), Error> {
        self.actor(actor)?;
        if review.ledger != self.store.binding()?
            || review.owner != actor
            || now
                .checked_duration_since(review.issued)
                .is_none_or(|age| age >= REVIEW_LIFETIME)
            || review.catalog != *catalog
        {
            return Err(Error::Changed);
        }
        let state = self.state()?;
        if state.head() != review.event.previous
            || state.events.len() as u64 != review.event.sequence
        {
            return Err(Error::Changed);
        }
        let fresh = self.review(
            actor,
            catalog,
            candidates,
            &review.event.coordinates,
            review.event.change.clone(),
        )?;
        if fresh.event != review.event {
            return Err(Error::Changed);
        }
        if self.store.stage(&review.event)? != StageOutcome::Stored {
            return Err(Error::Changed);
        }
        Ok(())
    }
    /// Inspect the exact durable decision and verified candidate without making
    /// a read grant. Remains available at the event limit so an operator can see
    /// why acquisition is disabled; missing/corrupt state still fails closed.
    pub fn status(
        &self,
        actor: OwnerId,
        catalog: &Catalog,
        candidates: &PackageStore,
        coordinates: &Coordinates,
    ) -> Result<Status, Error> {
        self.actor(actor)?;
        let state = self.state()?;
        let version = state.find(coordinates).ok_or(Error::Denied)?;
        if version.coordinates != *coordinates {
            return Err(Error::Denied);
        }
        check_source(catalog, version.source, version.repository)?;
        let package = checked_candidate(candidates, version)?;
        Ok(Status {
            coordinates: coordinates.clone(),
            candidate_sha256: version.candidate,
            source: version.source,
            repository: version.repository,
            commit: package.source().commit().into(),
            source_sha256: package.source().source_sha256(),
            archive_sha256: package.archive_sha256(),
            archive_bytes: package.archive().len() as u64,
            readers: version.readers.clone(),
            yanked: version.yanked,
            events_used: state.events.len(),
            event_capacity: MAX_POLICY_EVENTS,
        })
    }
    fn allowed(
        &self,
        actor: OwnerId,
        project: ProjectId,
        catalog: &Catalog,
        coordinates: &Coordinates,
    ) -> Result<Version, Error> {
        self.actor(actor)?;
        let state = self.state()?;
        if state.events.len() >= MAX_POLICY_EVENTS {
            return Err(Error::Capacity);
        }
        let version = state.find(coordinates).ok_or(Error::Denied)?;
        // Aliases/rebound URLs cannot disclose a private version's metadata.
        if version.coordinates != *coordinates
            || !version.readers.contains(&project)
            || !catalog.projects().iter().any(|p| p.id() == project)
        {
            return Err(Error::Denied);
        }
        check_source(catalog, version.source, version.repository)?;
        Ok(version.clone())
    }
    /// Authorized index metadata reflects yank state. No archive bytes escape
    /// here and unauthorized/unknown coordinates share the same denial.
    pub fn index_line(
        &self,
        actor: OwnerId,
        project: ProjectId,
        catalog: &Catalog,
        candidates: &PackageStore,
        coordinates: &Coordinates,
    ) -> Result<Vec<u8>, Error> {
        let version = self.allowed(actor, project, catalog, coordinates)?;
        let package = checked_candidate(candidates, &version)?;
        let mut line: Value =
            serde_json::from_slice(package.index_line()).map_err(|_| Error::Integrity)?;
        line["yanked"] = json!(version.yanked);
        let mut bytes = serde_json::to_vec(&line).map_err(|_| Error::Integrity)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
    /// Exact-checksum locked acquisition remains possible after a yank while
    /// access is still granted. A yank changes new resolution, not archive bytes.
    /// Recheck authorization at acquisition: earlier index access grants no token.
    pub fn archive(
        &self,
        actor: OwnerId,
        project: ProjectId,
        catalog: &Catalog,
        candidates: &PackageStore,
        coordinates: &Coordinates,
        expected: Sha256,
    ) -> Result<Vec<u8>, Error> {
        let version = self.allowed(actor, project, catalog, coordinates)?;
        let package = checked_candidate(candidates, &version)?;
        if package.archive_sha256() != expected {
            return Err(Error::Integrity);
        }
        Ok(package.archive().to_vec())
    }
}
fn state_from(events: Vec<Event>, owner: OwnerId) -> Result<State, Error> {
    let mut events = events;
    events.sort_by_key(|e| e.sequence);
    let mut state = State {
        events: Vec::new(),
        versions: Vec::new(),
    };
    for event in events {
        if event.sequence != state.events.len() as u64
            || event.previous != state.head()
            || event.owner != owner
        {
            return Err(Error::Integrity);
        }
        apply(&mut state.versions, &event).map_err(|_| Error::Integrity)?;
        state.events.push(event);
    }
    Ok(state)
}

fn checked_candidate(candidates: &PackageStore, version: &Version) -> Result<CargoPackage, Error> {
    let package = candidates
        .read(&version.coordinates)?
        .ok_or(Error::Unavailable)?;
    if Sha256::digest(&package.encode()) != version.candidate
        || package.source().project() != version.source.to_bytes()
        || package.source().repository() != version.repository.to_bytes()
    {
        return Err(Error::Integrity);
    }
    Ok(package)
}
fn check_source(
    catalog: &Catalog,
    source: ProjectId,
    repository: RepositoryId,
) -> Result<(), Error> {
    if catalog
        .projects()
        .iter()
        .any(|p| p.id() == source && p.repositories().contains(&repository))
    {
        Ok(())
    } else {
        Err(Error::Denied)
    }
}
fn normalize(change: Change, catalog: &Catalog, source: ProjectId) -> Result<Change, Error> {
    let (publish, mut readers) = match change {
        Change::Publish { readers } => (true, readers),
        Change::Readers { readers } => (false, readers),
        Change::Yank { .. } => return Ok(change),
    };
    if readers.len() > devoption_projects::MAX_PROJECTS {
        return Err(Error::Invalid);
    }
    readers.sort_by_key(|id| id.to_bytes());
    if readers.windows(2).any(|w| w[0] == w[1])
        || readers
            .iter()
            .any(|id| !catalog.projects().iter().any(|p| p.id() == *id))
    {
        return Err(Error::Denied);
    }
    if !readers.contains(&source) {
        readers.push(source);
        readers.sort_by_key(|id| id.to_bytes());
    }
    Ok(if publish {
        Change::Publish { readers }
    } else {
        Change::Readers { readers }
    })
}
fn apply(versions: &mut Vec<Version>, event: &Event) -> Result<(), Error> {
    for version in versions.iter() {
        if version.coordinates.registry().id() == event.coordinates.registry().id() {
            if version.coordinates.registry() != event.coordinates.registry() {
                return Err(Error::Conflict);
            }
            if version
                .coordinates
                .name()
                .to_ascii_lowercase()
                .replace('_', "-")
                == event
                    .coordinates
                    .name()
                    .to_ascii_lowercase()
                    .replace('_', "-")
                && (version.coordinates.name() != event.coordinates.name()
                    || version.source != event.source
                    || version.repository != event.repository)
            {
                return Err(Error::Conflict);
            }
        }
    }
    let slot = event.coordinates.slot();
    let existing = versions.iter_mut().find(|v| v.coordinates.slot() == slot);
    match (&event.change, existing) {
        (Change::Publish { readers }, None) => {
            versions.push(Version {
                coordinates: event.coordinates.clone(),
                candidate: event.candidate,
                source: event.source,
                repository: event.repository,
                readers: readers.clone(),
                yanked: false,
            });
            Ok(())
        }
        (Change::Publish { .. }, Some(_)) => Err(Error::Conflict),
        (_, None) => Err(Error::Conflict),
        (change, Some(version)) => {
            if version.coordinates != event.coordinates
                || version.candidate != event.candidate
                || version.source != event.source
                || version.repository != event.repository
            {
                return Err(Error::Conflict);
            }
            match change {
                Change::Readers { readers } if *readers != version.readers => {
                    version.readers = readers.clone()
                }
                Change::Yank { yanked } if *yanked != version.yanked => version.yanked = *yanked,
                _ => return Err(Error::Conflict),
            }
            Ok(())
        }
    }
}

/// Fixed errors omit coordinates, project IDs, paths and source/private bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Denied,
    Changed,
    Conflict,
    Integrity,
    Unavailable,
    Capacity,
    Storage(StorageError),
}
impl From<StorageError> for Error {
    fn from(e: StorageError) -> Self {
        Self::Storage(e)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Denied => "registry access denied",
            Self::Changed => "registry review changed or expired",
            Self::Conflict => "registry version or policy conflict",
            Self::Invalid => "invalid registry policy",
            Self::Integrity => "registry policy integrity check failed",
            Self::Capacity => "registry policy capacity exceeded",
            Self::Unavailable => "registry candidate unavailable",
            Self::Storage(_) => "registry policy storage unavailable or requires inspection",
        })
    }
}
impl std::error::Error for Error {}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn id(value: &Value) -> Result<[u8; 16], StorageError> {
    let text = value.as_str().ok_or(StorageError::Invalid)?;
    if text.len() != 32
        || text
            .bytes()
            .any(|b| !b.is_ascii_digit() && !(b'a'..=b'f').contains(&b))
    {
        return Err(StorageError::Invalid);
    }
    Ok(ProjectId::from_hex(text)
        .ok_or(StorageError::Invalid)?
        .to_bytes())
}
impl Record for Event {
    type Key = u64;
    const LOCK: &'static str = ".publication-policy.lock";
    const PENDING: &'static str = ".publication-policy.pending";
    const SUFFIX: &'static str = ".policy";
    const MAX_BYTES: usize = MAX_EVENT_BYTES;
    fn coordinates(&self) -> &u64 {
        &self.sequence
    }
    fn key_for(sequence: &u64) -> String {
        format!("{sequence:064x}{}", Self::SUFFIX)
    }
    fn encode(&self) -> Vec<u8> {
        let (action, readers, yanked) = match &self.change {
            Change::Publish { readers } => ("publish", readers.clone(), false),
            Change::Readers { readers } => ("readers", readers.clone(), false),
            Change::Yank { yanked } => ("yank", Vec::new(), *yanked),
        };
        // Owner identity is bookkeeping metadata, never an authentication token.
        // Its byte accessor is crate-private for this durable codec.
        let value = json!({"schema":1,"sequence":self.sequence,"previous":hex(self.previous.bytes()),"registry":self.coordinates.registry().id().as_str(),"index":self.coordinates.registry().index(),"name":self.coordinates.name(),"version":self.coordinates.version(),"candidate":hex(self.candidate.bytes()),"owner":hex(&self.owner.bytes()),"source":hex(&self.source.to_bytes()),"repository":hex(&self.repository.to_bytes()),"action":action,"readers":readers.iter().map(|id|hex(&id.to_bytes())).collect::<Vec<_>>(),"yanked":yanked});
        let mut bytes = serde_json::to_vec(&value).expect("fixed registry policy record");
        bytes.extend(Sha256::digest(&bytes).bytes());
        bytes
    }
    fn decode(bytes: &[u8]) -> Result<Self, StorageError> {
        if bytes.len() < 32 || bytes.len() > MAX_EVENT_BYTES {
            return Err(StorageError::Invalid);
        }
        let (body, hash) = bytes.split_at(bytes.len() - 32);
        if Sha256::digest(body).bytes() != hash {
            return Err(StorageError::Integrity);
        }
        let v: Value = serde_json::from_slice(body).map_err(|_| StorageError::Invalid)?;
        if v.as_object().map(|o| o.len()) != Some(14)
            || v["schema"] != 1
            || serde_json::to_vec(&v).map_err(|_| StorageError::Invalid)? != body
        {
            return Err(StorageError::Invalid);
        }
        let string = |key: &str| v[key].as_str().ok_or(StorageError::Invalid);
        let digest = |key: &str| Sha256::from_hex(string(key)?).map_err(|_| StorageError::Invalid);
        let registry = HostedRegistry::new(
            RegistryId::new(string("registry")?).map_err(|_| StorageError::Invalid)?,
            string("index")?,
        )
        .map_err(|_| StorageError::Invalid)?;
        let coordinates = Coordinates::new(registry, string("name")?, string("version")?)?;
        let source = ProjectId::from_bytes(id(&v["source"])?);
        let repository = RepositoryId::from_bytes(id(&v["repository"])?);
        let owner = OwnerId::from_bytes(id(&v["owner"])?);
        let values = v["readers"].as_array().ok_or(StorageError::Invalid)?;
        if values.len() > devoption_projects::MAX_PROJECTS {
            return Err(StorageError::Invalid);
        }
        let readers = values
            .iter()
            .map(|v| id(v).map(ProjectId::from_bytes))
            .collect::<Result<Vec<_>, _>>()?;
        let yanked = v["yanked"].as_bool().ok_or(StorageError::Invalid)?;
        let change = match string("action")? {
            "publish" | "readers"
                if !yanked
                    && readers.contains(&source)
                    && readers
                        .windows(2)
                        .all(|w| w[0].to_bytes() < w[1].to_bytes()) =>
            {
                if string("action")? == "publish" {
                    Change::Publish { readers }
                } else {
                    Change::Readers { readers }
                }
            }
            "yank" if readers.is_empty() => Change::Yank { yanked },
            _ => return Err(StorageError::Invalid),
        };
        let sequence = v["sequence"]
            .as_u64()
            .filter(|n| *n < super::record_store::MAX_RECORDS as u64)
            .ok_or(StorageError::Invalid)?;
        Ok(Self {
            sequence,
            previous: digest("previous")?,
            coordinates,
            candidate: digest("candidate")?,
            owner,
            source,
            repository,
            change,
        })
    }
}
mod recovery;
pub use recovery::RecoveryReview;
#[cfg(test)]
mod tests;
