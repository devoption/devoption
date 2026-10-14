//! Bounded, atomic profile records anchored to the startup-selected data directory.
use super::{DisplayName, Error, MAX_NAME_BYTES};
use crate::{identity::IdentityId, owner_store::OwnerStore};
use rustix::{
    fs::{AtFlags, Mode, OFlags, openat, renameat, unlinkat},
    io::Errno,
    process::geteuid,
};
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    sync::atomic::{AtomicU64, Ordering},
};

const RECORD: &str = "profile.bin";
const MAGIC: &[u8; 8] = b"DVPROFIL";
const HEADER: usize = 25;
const MAX_READ_ATTEMPTS: usize = 8;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub(crate) struct ProfileStore {
    directory: File,
    identity: IdentityId,
}

impl ProfileStore {
    pub(crate) fn new(owner: &OwnerStore, identity: IdentityId) -> Result<Self, Error> {
        let store = Self {
            directory: owner.duplicate_directory()?,
            identity,
        };
        store.load(identity)?;
        Ok(store)
    }

    fn check(&self, identity: IdentityId) -> Result<(), Error> {
        if identity != self.identity {
            return Err(Error::InvalidRecord);
        }
        let metadata = self.directory.metadata()?;
        if !metadata.is_dir()
            || metadata.nlink() == 0
            || metadata.uid() != geteuid().as_raw()
            || metadata.mode() & 0o077 != 0
        {
            return Err(Error::UnsafeDirectory);
        }
        Ok(())
    }

    pub(crate) fn load(&self, identity: IdentityId) -> Result<DisplayName, Error> {
        self.load_after_open(identity, || {})
    }

    // Keep the open/metadata boundary testable without timing-dependent sleeps.
    fn load_after_open(
        &self,
        identity: IdentityId,
        mut after_open: impl FnMut(),
    ) -> Result<DisplayName, Error> {
        for attempt in 0..MAX_READ_ATTEMPTS {
            self.check(identity)?;
            let file = match openat(
                &self.directory,
                RECORD,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            ) {
                Ok(fd) => File::from(fd),
                Err(Errno::NOENT) if attempt == 0 => return Ok(DisplayName::default()),
                Err(error) => return Err(error.into()),
            };
            after_open();
            let metadata = file.metadata()?;
            if !metadata.is_file()
                || metadata.uid() != geteuid().as_raw()
                || metadata.mode() & 0o077 != 0
                || metadata.len() > (HEADER + MAX_NAME_BYTES) as u64
            {
                return Err(Error::InvalidRecord);
            }
            // Atomic replacement can unlink a record after open but before stat.
            // Never accept that retired handle: reopen the current path and apply
            // every check again. A disappearing replacement is not an unset name.
            if metadata.nlink() == 0 {
                continue;
            }
            if metadata.nlink() != 1 {
                return Err(Error::InvalidRecord);
            }
            let mut bytes = Vec::new();
            file.take((HEADER + MAX_NAME_BYTES + 1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes.len() < HEADER
                || bytes.len() > HEADER + MAX_NAME_BYTES
                || &bytes[..8] != MAGIC
                || bytes[8] != 1
                || bytes[9..HEADER] != identity.to_bytes()
            {
                return Err(Error::InvalidRecord);
            }
            let name = std::str::from_utf8(&bytes[HEADER..]).map_err(|_| Error::InvalidRecord)?;
            return DisplayName::new(name).map_err(|_| Error::InvalidRecord);
        }
        Err(Errno::AGAIN.into())
    }

    /// Last atomic replacement wins. An uncertain post-publication sync failure must
    /// be checked with load; never delete/reset an existing record on failure.
    pub(crate) fn save(&self, identity: IdentityId, name: &DisplayName) -> Result<(), Error> {
        self.load(identity)?; // Fail closed on an existing unsafe or invalid record.
        let mut bytes = Vec::with_capacity(HEADER + name.as_str().len());
        bytes.extend_from_slice(MAGIC);
        bytes.push(1);
        bytes.extend_from_slice(&identity.to_bytes());
        bytes.extend_from_slice(name.as_str().as_bytes());
        for _ in 0..32 {
            let name = format!(
                ".profile-{}-{}.tmp",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            );
            let fd = match openat(
                &self.directory,
                name.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(fd) => fd,
                Err(Errno::EXIST) => continue,
                Err(error) => return Err(error.into()),
            };
            let pending = Pending {
                directory: &self.directory,
                name,
            };
            let mut file = File::from(fd);
            file.write_all(&bytes)?;
            file.sync_all()?;
            self.check(identity)?;
            renameat(
                &self.directory,
                pending.name.as_str(),
                &self.directory,
                RECORD,
            )?;
            self.directory.sync_all()?;
            return Ok(());
        }
        Err(Errno::EXIST.into())
    }
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

#[cfg(test)]
mod tests;
