//! Bounded read-only verification of the two reviewed tool files, never execution.
//! A matching hash is not toolchain provenance, version verification or isolation.

use crate::review_context::{Executable, ReviewContext};
use rustix::{
    fs::{Mode, OFlags, open, openat},
    process::geteuid,
};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    fs::{File, Metadata},
    io::ErrorKind,
    os::unix::fs::{FileExt, MetadataExt},
};

pub const MAX_EXECUTABLE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_COMPONENTS: usize = 64;
const DIRECTORY: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const FILE: OFlags = OFlags::RDONLY
    .union(OFlags::NONBLOCK)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

#[derive(PartialEq, Eq)]
struct Observation {
    device: u64,
    inode: u64,
    uid: u32,
    gid: u32,
    mode: u32,
    links: u64,
    bytes: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl Observation {
    fn read(file: &File, uid: u32, limit: u64) -> Result<Self, Error> {
        let metadata = file.metadata().map_err(|_| Error::Unavailable)?;
        let required_execute_bit = if metadata.uid() == uid { 0o100 } else { 0o001 };
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.len() == 0
            || (metadata.uid() != uid && metadata.uid() != 0)
            || metadata.mode() & 0o7022 != 0
            || metadata.mode() & required_execute_bit == 0
        {
            return Err(Error::UnsafeFile);
        }
        if metadata.len() > limit {
            return Err(Error::TooLarge);
        }
        Ok(Self::from_metadata(metadata))
    }
    fn from_metadata(m: Metadata) -> Self {
        Self {
            device: m.dev(),
            inode: m.ino(),
            uid: m.uid(),
            gid: m.gid(),
            mode: m.mode(),
            links: m.nlink(),
            bytes: m.len(),
            modified: (m.mtime(), m.mtime_nsec()),
            changed: (m.ctime(), m.ctime_nsec()),
        }
    }
}
struct Pinned {
    expected: Executable,
    file: File,
    observed: Observation,
}

/// Retains private read handles for Cargo and rustc. No descriptor export,
/// serialization, process launch or automatic approval/admission integration.
pub struct ReviewedExecutables {
    cargo: Pinned,
    rustc: Pinned,
    uid: u32,
}
impl ReviewedExecutables {
    pub fn open(context: &ReviewContext) -> Result<Self, Error> {
        let uid = geteuid().as_raw();
        if uid == 0 || context.details().execution_uid != uid {
            return Err(Error::UnsupportedOwner);
        }
        let cargo = pin(&context.details().cargo, uid)?;
        let rustc = pin(&context.details().rustc, uid)?;
        if (cargo.observed.device, cargo.observed.inode)
            == (rustc.observed.device, rustc.observed.inode)
        {
            return Err(Error::UnsafeFile);
        }
        let result = Self { cargo, rustc, uid };
        result.revalidate()?;
        Ok(result)
    }

    /// Rehash the retained bytes and recheck path bindings. Cannot prevent a
    /// trusted same-user writer changing bytes after the check has completed.
    pub fn revalidate(&self) -> Result<(), Error> {
        if geteuid().as_raw() != self.uid {
            return Err(Error::UnsupportedOwner);
        }
        for pinned in [&self.cargo, &self.rustc] {
            verify(pinned, self.uid)?;
        }
        // Catch metadata/name changes to the first tool while checking the second.
        for pinned in [&self.cargo, &self.rustc] {
            binding(pinned, self.uid)?;
        }
        Ok(())
    }
}

fn pin(expected: &Executable, uid: u32) -> Result<Pinned, Error> {
    let file = open_file(&expected.path)?;
    let observed = Observation::read(&file, uid, MAX_EXECUTABLE_BYTES)?;
    let pinned = Pinned {
        expected: expected.clone(),
        file,
        observed,
    };
    verify(&pinned, uid)?;
    Ok(pinned)
}
fn verify(pinned: &Pinned, uid: u32) -> Result<(), Error> {
    binding(pinned, uid)?;
    let digest = hash(&pinned.file, uid, MAX_EXECUTABLE_BYTES, || {})?;
    if &digest != pinned.expected.sha256.bytes() {
        return Err(Error::HashMismatch);
    }
    binding(pinned, uid)
}
fn binding(pinned: &Pinned, uid: u32) -> Result<(), Error> {
    if Observation::read(&pinned.file, uid, MAX_EXECUTABLE_BYTES)? != pinned.observed
        || Observation::read(
            &open_file(&pinned.expected.path)?,
            uid,
            MAX_EXECUTABLE_BYTES,
        )? != pinned.observed
    {
        return Err(Error::Changed);
    }
    Ok(())
}
fn hash(
    file: &File,
    uid: u32,
    limit: u64,
    mut after_read: impl FnMut(),
) -> Result<[u8; 32], Error> {
    let before = Observation::read(file, uid, limit)?;
    let mut hasher = Sha256::new();
    let mut offset = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let available = buffer.len().min((limit + 1 - offset) as usize);
        let read = match file.read_at(&mut buffer[..available], offset) {
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            result => result.map_err(|_| Error::Unavailable)?,
        };
        if read == 0 {
            break;
        }
        offset += read as u64;
        if offset > limit {
            return Err(Error::TooLarge);
        }
        hasher.update(&buffer[..read]);
        after_read();
    }
    if offset != before.bytes || before != Observation::read(file, uid, limit)? {
        return Err(Error::Changed);
    }
    Ok(hasher.finalize().into())
}
fn open_file(path: &str) -> Result<File, Error> {
    if !crate::absolute_path(path) || path.split('/').skip(1).count() > MAX_COMPONENTS {
        return Err(Error::InvalidPath);
    }
    let (parent, name) = path.rsplit_once('/').ok_or(Error::InvalidPath)?;
    let mut directory =
        File::from(open("/", DIRECTORY, Mode::empty()).map_err(|_| Error::Unavailable)?);
    for component in parent.split('/').filter(|part| !part.is_empty()) {
        directory = File::from(
            openat(&directory, component, DIRECTORY, Mode::empty())
                .map_err(|_| Error::Unavailable)?,
        );
    }
    Ok(File::from(
        openat(&directory, name, FILE, Mode::empty()).map_err(|_| Error::Unavailable)?,
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    UnsupportedOwner,
    InvalidPath,
    UnsafeFile,
    TooLarge,
    Changed,
    HashMismatch,
    Unavailable,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnsupportedOwner => "tool verification requires the current unprivileged account",
            Self::InvalidPath => "reviewed tool path is unsupported",
            Self::UnsafeFile => "reviewed tool file metadata is unsafe",
            Self::TooLarge => "reviewed tool exceeds the byte limit",
            Self::Changed => "reviewed tool identity or metadata changed",
            Self::HashMismatch => "reviewed tool hash does not match",
            Self::Unavailable => "reviewed tool is unavailable",
        })
    }
}
impl std::error::Error for Error {}
#[cfg(test)]
mod tests;
