//! Create-once owner storage for the Linux MVP and macOS development preview.
//!
//! Callers supply an existing private directory and an already-hashed password.
//! The running preview does not use this store. No identity/salt generation,
//! authentication, permission grants, reset, or migration is performed here.
//! The directory's final component and record cannot be symlinks; operations stay
//! anchored to the opened directory. This is not a boundary against its OS owner.
//! Interrupted writes may leave private temporary files, which are never loaded.

use std::{
    error::Error,
    fmt,
    fs::{File, Metadata},
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use rustix::{
    fs::{AtFlags, Mode, OFlags, linkat, open, openat, unlinkat},
    io::Errno,
    process::geteuid,
};

use crate::{identity::IdentityId, password::supported_record};

const RECORD_NAME: &str = "owner.bin";
// Format: eight-byte magic, one-byte version, 16-byte identity, UTF-8 PHC record.
const MAGIC: &[u8; 8] = b"DVOWNER\0";
const VERSION: u8 = 1;
const HEADER_BYTES: usize = 25;
const MAX_RECORD_BYTES: usize = HEADER_BYTES + 128;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Stored identity and its local password credential, without a permission grant.
/// Intentionally not Debug: password hashes are sensitive storage data.
pub struct OwnerRecord {
    identity: IdentityId,
    password_hash: String,
}

impl OwnerRecord {
    pub fn identity(&self) -> IdentityId {
        self.identity
    }

    /// For the password verifier; never send this record to a browser or log it.
    pub fn password_hash(&self) -> &str {
        &self.password_hash
    }
}

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    UnsafeDirectory,
    InvalidRecord,
    UnsupportedVersion,
    AlreadyExists,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Io(_) => "owner store I/O failed",
            Self::UnsafeDirectory => "owner directory must be private and owned by this OS user",
            Self::InvalidRecord => "owner record is invalid or not private",
            Self::UnsupportedVersion => "owner record version is unsupported",
            Self::AlreadyExists => "owner record already exists; setup cannot replace it",
        })
    }
}

impl Error for StoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<Errno> for StoreError {
    fn from(error: Errno) -> Self {
        Self::Io(error.into())
    }
}

/// An opened private directory. Opening it creates or modifies nothing.
pub struct OwnerStore {
    directory: File,
}

impl OwnerStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let directory = File::from(open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let store = Self { directory };
        store.check_directory()?;
        Ok(store)
    }

    /// Only an absent record means setup is unconfigured. All other errors fail
    /// closed; callers must never interpret them as permission to create an owner.
    pub fn load(&self) -> Result<Option<OwnerRecord>, StoreError> {
        self.check_directory()?;
        let file = match openat(
            &self.directory,
            RECORD_NAME,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => File::from(fd),
            Err(Errno::NOENT) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file() || !is_private(&metadata) {
            return Err(StoreError::InvalidRecord);
        }
        let mut bytes = Vec::new();
        file.take(MAX_RECORD_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        decode(&bytes).map(Some)
    }

    /// Atomically publish one complete record; never replace any existing entry.
    /// The caller must separately authorize first-owner setup. This library API
    /// does not expose setup over HTTP or create permissions/sessions.
    ///
    /// An I/O error after publication may mean the record exists but directory
    /// durability was not confirmed. Inspect `load`; do not delete or reset it.
    pub fn create(&self, identity: IdentityId, password_hash: &str) -> Result<(), StoreError> {
        self.check_directory()?;
        if supported_record(password_hash).is_none() {
            return Err(StoreError::InvalidRecord);
        }
        let mut bytes = Vec::with_capacity(HEADER_BYTES + password_hash.len());
        bytes.extend_from_slice(MAGIC);
        bytes.push(VERSION);
        bytes.extend_from_slice(&identity.to_bytes());
        bytes.extend_from_slice(password_hash.as_bytes());

        let (mut file, pending) = self.temporary_file()?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        // linkat publishes the fully written inode and cannot overwrite a name.
        match linkat(
            &self.directory,
            pending.name.as_str(),
            &self.directory,
            RECORD_NAME,
            AtFlags::empty(),
        ) {
            Ok(()) => {}
            Err(Errno::EXIST) => return Err(StoreError::AlreadyExists),
            Err(error) => return Err(error.into()),
        }
        drop(file);
        drop(pending);
        self.directory.sync_all()?;
        Ok(())
    }

    fn check_directory(&self) -> Result<(), StoreError> {
        let metadata = self.directory.metadata()?;
        if metadata.is_dir() && metadata.nlink() > 0 && is_private(&metadata) {
            Ok(())
        } else {
            Err(StoreError::UnsafeDirectory)
        }
    }

    fn temporary_file(&self) -> Result<(File, Pending<'_>), StoreError> {
        for _ in 0..32 {
            // Only the OS owner can access this directory. Names need uniqueness,
            // not secrecy; exclusive creation also rejects preexisting symlinks.
            let name = format!(
                ".owner-{}-{}.tmp",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            );
            match openat(
                &self.directory,
                name.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(fd) => {
                    return Ok((
                        File::from(fd),
                        Pending {
                            directory: &self.directory,
                            name,
                        },
                    ));
                }
                Err(Errno::EXIST) => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(Errno::EXIST.into())
    }
}

fn is_private(metadata: &Metadata) -> bool {
    metadata.uid() == geteuid().as_raw() && metadata.mode() & 0o077 == 0
}

struct Pending<'a> {
    directory: &'a File,
    name: String,
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        let _ = unlinkat(self.directory, self.name.as_str(), AtFlags::empty());
    }
}

fn decode(bytes: &[u8]) -> Result<OwnerRecord, StoreError> {
    if bytes.len() < HEADER_BYTES || bytes.len() > MAX_RECORD_BYTES || &bytes[..8] != MAGIC {
        return Err(StoreError::InvalidRecord);
    }
    if bytes[8] != VERSION {
        return Err(StoreError::UnsupportedVersion);
    }
    let mut identity = [0; 16];
    identity.copy_from_slice(&bytes[9..HEADER_BYTES]);
    let password_hash =
        std::str::from_utf8(&bytes[HEADER_BYTES..]).map_err(|_| StoreError::InvalidRecord)?;
    if supported_record(password_hash).is_none() {
        return Err(StoreError::InvalidRecord);
    }
    Ok(OwnerRecord {
        identity: IdentityId::from_bytes(identity),
        password_hash: password_hash.to_owned(),
    })
}

#[cfg(test)]
mod tests;
