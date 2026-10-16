//! Private bounded history, never restored execution or artifact authority.
use crate::{
    BuildPlan, absolute_path,
    jobs::{BuildJob, JobId, State},
};
use rustix::{
    fs::{FlockOperation, Mode, OFlags, flock, open, openat, renameat},
    process::geteuid,
};
use std::{
    fmt,
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
};
mod codec;
const RECORD: &str = "build-history.bin";
const LOCK: &str = "build-history.lock";
const PENDING: &str = ".build-history.pending";
pub const MAX_JOBS: usize = 10;
const DIRECTORY: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const READ: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

/// Historical claims only; no plan, approval, path or executable artifact handle.
#[derive(Clone, PartialEq, Eq)]
pub struct Record {
    id: JobId,
    project: [u8; 16],
    repository: [u8; 16],
    commit: String,
    source: [u8; 32],
    state: State,
}
impl Record {
    fn from_job(job: &BuildJob) -> Self {
        let plan = job.plan().details();
        Self {
            id: job.id(),
            project: plan.project.to_bytes(),
            repository: plan.repository.to_bytes(),
            commit: plan.commit.clone(),
            source: *plan.source.sha256().bytes(),
            state: job.state().clone(),
        }
    }
    pub fn id(&self) -> JobId {
        self.id
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
    pub fn source_sha256(&self) -> &[u8; 32] {
        &self.source
    }
    pub fn state(&self) -> &State {
        &self.state
    }
    fn same_identity(&self, other: &Self) -> bool {
        self.id == other.id
            && self.project == other.project
            && self.repository == other.repository
            && self.commit == other.commit
            && self.source == other.source
    }
}

/// One exclusive writer. Existing private directory required; no repair or cleanup.
pub struct History {
    path: String,
    directory: File,
    lock: File,
    uid: u32,
    poisoned: bool,
}
impl History {
    pub fn open(path: &str) -> Result<Self, Error> {
        let uid = geteuid().as_raw();
        if uid == 0 {
            return Err(Error::Unsafe);
        }
        let directory = directory(path)?;
        check_directory(&directory, uid)?;
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
        check_file(&lock, uid, 0)?;
        flock(&lock, FlockOperation::NonBlockingLockExclusive).map_err(|e| {
            if e == rustix::io::Errno::WOULDBLOCK {
                Error::Busy
            } else {
                Error::Unavailable
            }
        })?;
        let result = Self {
            path: path.into(),
            directory,
            lock,
            uid,
            poisoned: false,
        };
        result.load()?;
        Ok(result)
    }
    fn check(&self) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Unavailable);
        }
        self.check_storage()
    }
    fn check_storage(&self) -> Result<(), Error> {
        if geteuid().as_raw() != self.uid {
            return Err(Error::Unavailable);
        }
        check_directory(&self.directory, self.uid)?;
        let current = directory(&self.path)?;
        check_directory(&current, self.uid)?;
        if identity(&current)? != identity(&self.directory)? {
            return Err(Error::Changed);
        }
        check_file(&self.lock, self.uid, 0)?;
        let current_lock = File::from(
            openat(&self.directory, LOCK, READ, Mode::empty()).map_err(|_| Error::Unavailable)?,
        );
        check_file(&current_lock, self.uid, 0)?;
        if identity(&current_lock)? != identity(&self.lock)? {
            return Err(Error::Changed);
        }
        Ok(())
    }
    pub fn load(&self) -> Result<Vec<Record>, Error> {
        self.check()?;
        let file = match openat(&self.directory, RECORD, READ, Mode::empty()) {
            Ok(fd) => File::from(fd),
            Err(rustix::io::Errno::NOENT) => return Ok(Vec::new()),
            Err(_) => return Err(Error::Unavailable),
        };
        check_file(&file, self.uid, codec::MAX_BYTES as u64)?;
        let before = observation(&file)?;
        let mut bytes = Vec::new();
        (&file)
            .take((codec::MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Unavailable)?;
        let current = File::from(
            openat(&self.directory, RECORD, READ, Mode::empty()).map_err(|_| Error::Changed)?,
        );
        check_file(&current, self.uid, codec::MAX_BYTES as u64)?;
        if before != observation(&file)? || before != observation(&current)? {
            return Err(Error::Changed);
        }
        self.check()?;
        codec::decode(&bytes)
    }
    /// Allocate and durably save a fresh queued attempt before returning it.
    /// This does not approve a plan, reserve a runner or establish child cleanup.
    pub fn queue(&mut self, plan: BuildPlan) -> Result<BuildJob, Error> {
        self.queue_using(plan, |bytes| {
            getrandom::fill(bytes).map_err(|_| Error::Randomness)
        })
    }
    fn queue_using(
        &mut self,
        plan: BuildPlan,
        mut random: impl FnMut(&mut [u8; 16]) -> Result<(), Error>,
    ) -> Result<BuildJob, Error> {
        let mut records = self.load()?;
        if records.len() == MAX_JOBS {
            return Err(Error::Capacity);
        }
        // Bounded even if the entropy source repeatedly returns a retained ID.
        for _ in 0..16 {
            let mut bytes = [0; 16];
            random(&mut bytes)?;
            let id = JobId::from_bytes(bytes);
            if records.iter().any(|record| record.id == id) {
                continue;
            }
            let job = BuildJob::queued(id, plan);
            records.push(Record::from_job(&job));
            self.publish(&records)?;
            return Ok(job);
        }
        Err(Error::Randomness)
    }
    /// Update an already persisted attempt. Exact duplicate observations are
    /// harmless; other updates need matching identity and a legal lifecycle edge.
    pub fn record(&mut self, job: &BuildJob) -> Result<(), Error> {
        let mut records = self.load()?;
        let next = Record::from_job(job);
        let old = records
            .iter_mut()
            .find(|r| r.id == next.id)
            .ok_or(Error::UnknownAttempt)?;
        if !old.same_identity(&next) {
            return Err(Error::Changed);
        }
        if *old == next {
            return Ok(());
        }
        if !transition(&old.state, &next.state) {
            return Err(Error::Transition);
        }
        *old = next;
        self.publish(&records)
    }
    /// Persist interruption only. Never resumes work or establishes child cleanup.
    pub fn recover_interrupted(&mut self) -> Result<usize, Error> {
        let mut records = self.load()?;
        let mut count = 0;
        for record in &mut records {
            if matches!(record.state, State::Queued | State::Running) {
                record.state = State::Interrupted;
                count += 1;
            }
        }
        if count > 0 {
            self.publish(&records)?;
        }
        Ok(count)
    }
    fn publish(&mut self, records: &[Record]) -> Result<(), Error> {
        self.publish_using(records, |file, bytes| file.write_all(bytes))
    }
    fn publish_using(
        &mut self,
        records: &[Record],
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<(), Error> {
        let bytes = codec::encode(records)?;
        self.check()?;
        self.poisoned = true;
        // Leave an uncertain pending file for explicit inspection, never silently
        // delete/adopt it. A post-rename sync failure may have published new data.
        let mut file = File::from(
            openat(
                &self.directory,
                PENDING,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|_| Error::Unavailable)?,
        );
        check_file(&file, self.uid, 0)?;
        write(&mut file, &bytes).map_err(|_| Error::Unavailable)?;
        file.sync_all().map_err(|_| Error::Unavailable)?;
        check_file(&file, self.uid, codec::MAX_BYTES as u64)?;
        let current = File::from(
            openat(&self.directory, PENDING, READ, Mode::empty()).map_err(|_| Error::Changed)?,
        );
        if observation(&current)? != observation(&file)? {
            return Err(Error::Changed);
        }
        // Recheck directory/lock binding immediately before publication.
        self.check_storage()?;
        renameat(&self.directory, PENDING, &self.directory, RECORD)
            .map_err(|_| Error::Unavailable)?;
        self.directory.sync_all().map_err(|_| Error::Unavailable)?;
        self.poisoned = false;
        Ok(())
    }
}
fn transition(old: &State, new: &State) -> bool {
    use crate::jobs::Failure;
    matches!(
        (old, new),
        (
            State::Queued,
            State::Running
                | State::Cancelled
                | State::Interrupted
                | State::Failed(Failure::Preparation)
        ) | (
            State::Running,
            State::Succeeded(_) | State::Failed(_) | State::Cancelled | State::Interrupted
        )
    )
}
fn directory(path: &str) -> Result<File, Error> {
    if !absolute_path(path) || path.split('/').skip(1).count() > 64 {
        return Err(Error::Unsafe);
    }
    let mut file = File::from(open("/", DIRECTORY, Mode::empty()).map_err(|_| Error::Unsafe)?);
    for part in path.split('/').skip(1) {
        file =
            File::from(openat(&file, part, DIRECTORY, Mode::empty()).map_err(|_| Error::Unsafe)?);
    }
    Ok(file)
}
fn check_directory(file: &File, uid: u32) -> Result<(), Error> {
    let m = file.metadata().map_err(|_| Error::Unavailable)?;
    if !m.is_dir() || m.uid() != uid || m.nlink() == 0 || m.mode() & 0o7777 != 0o700 {
        return Err(Error::Unsafe);
    }
    Ok(())
}
fn check_file(file: &File, uid: u32, max: u64) -> Result<(), Error> {
    let m = file.metadata().map_err(|_| Error::Unavailable)?;
    if !m.is_file()
        || m.uid() != uid
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
type Observation = (u64, u64, u64, u32, u32, u32, u64, i64, i64, i64, i64);
fn observation(file: &File) -> Result<Observation, Error> {
    let m = file.metadata().map_err(|_| Error::Unavailable)?;
    Ok((
        m.dev(),
        m.ino(),
        m.len(),
        m.uid(),
        m.gid(),
        m.mode(),
        m.nlink(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    ))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unsafe,
    Busy,
    Unavailable,
    Changed,
    Corrupt,
    Capacity,
    Transition,
    Randomness,
    UnknownAttempt,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unsafe => "unsafe build history storage",
            Self::Busy => "build history writer is busy",
            Self::Unavailable => "build history storage unavailable; inspect before retrying",
            Self::Changed => "build history identity changed",
            Self::Corrupt => "invalid build history record",
            Self::Capacity => "build history capacity reached",
            Self::Randomness => "fresh build attempt identity unavailable",
            Self::UnknownAttempt => "build attempt is not in history",
            Self::Transition => "build history transition rejected",
        })
    }
}
impl std::error::Error for Error {}
#[cfg(test)]
mod tests;
