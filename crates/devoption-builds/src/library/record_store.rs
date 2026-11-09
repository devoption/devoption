//! Shared descriptor-anchored immutable record storage. No publication/access grants.
use super::staging::{Error, StageOutcome};
use rustix::{
    fs::{AtFlags, Dir, FlockOperation, Mode, OFlags, flock, linkat, openat, unlinkat},
    io::Errno,
    process::geteuid,
};
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
};
pub(crate) const MAX_RECORDS: usize = 128;
const READ: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);
pub(crate) trait Record: PartialEq + Sized {
    type Key: PartialEq;
    const LOCK: &'static str;
    const PENDING: &'static str;
    const SUFFIX: &'static str;
    const MAX_BYTES: usize;
    fn coordinates(&self) -> &Self::Key;
    fn encode(&self) -> Vec<u8>;
    fn decode(bytes: &[u8]) -> Result<Self, Error>;
    fn key_for(coordinates: &Self::Key) -> String;
    fn key(&self) -> String {
        Self::key_for(self.coordinates())
    }
}
/// Exclusive descriptor-anchored staging store in an existing private directory.
/// The OS owner and local filesystem are trusted. No directory creation, repair,
/// deletion API, publication, registry serving, project grants or network I/O.
pub(crate) struct RecordStore<T: Record> {
    marker: std::marker::PhantomData<T>,
    directory: File,
    lock: File,
    poisoned: bool,
}
impl<T: Record> Drop for RecordStore<T> {
    fn drop(&mut self) {
        let _ = flock(&self.lock, FlockOperation::Unlock);
    }
}
impl<T: Record> RecordStore<T> {
    pub fn open(path: &str) -> Result<Self, Error> {
        let store = Self::open_locked(path)?;
        store.inventory()?;
        Ok(store)
    }
    fn open_locked(path: &str) -> Result<Self, Error> {
        let directory = crate::history::directory(path).map_err(|_| Error::Unsafe)?;
        directory_valid(&directory)?;
        let lock = File::from(
            openat(
                &directory,
                T::LOCK,
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
            marker: std::marker::PhantomData,
        };
        Ok(store)
    }
    pub(crate) fn binding(&self) -> Result<[(u64, u64); 2], Error> {
        self.check()?;
        Ok([identity(&self.directory)?, identity(&self.lock)?])
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
            openat(&self.directory, T::LOCK, READ, Mode::empty()).map_err(|_| Error::Unsafe)?,
        );
        file_valid(&current, 0)?;
        if identity(&current)? != identity(&self.lock)? {
            return Err(Error::Unsafe);
        }
        Ok(())
    }
    pub(crate) fn inventory(&self) -> Result<Vec<String>, Error> {
        self.inventory_with(None)
    }
    fn inventory_with(&self, recovery: Option<(&str, u64)>) -> Result<Vec<String>, Error> {
        self.check()?;
        let mut names = Vec::new();
        for entry in Dir::read_from(&self.directory).map_err(|_| Error::Unavailable)? {
            let entry = entry.map_err(|_| Error::Unavailable)?;
            let name = entry.file_name().to_str().map_err(|_| Error::Unsafe)?;
            if matches!(name, "." | "..") || name == T::LOCK {
                continue;
            }
            if name == T::PENDING {
                if recovery.is_some() {
                    continue;
                }
                return Err(Error::Uncertain);
            }
            if name.len() != 64 + T::SUFFIX.len()
                || !name.ends_with(T::SUFFIX)
                || !name.as_bytes()[..64]
                    .iter()
                    .copied()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(Error::Unsafe);
            }
            if names.len() >= MAX_RECORDS {
                return Err(Error::Capacity);
            }
            let links = recovery
                .filter(|(key, _)| *key == name)
                .map_or(1, |(_, links)| links);
            let package = self.load_links(name, links)?.ok_or(Error::Integrity)?;
            if package.key() != name {
                return Err(Error::Integrity);
            }
            names.push(name.to_owned());
        }
        Ok(names)
    }
    pub(crate) fn records(&self) -> Result<Vec<T>, Error> {
        self.inventory()?
            .iter()
            .map(|name| self.load(name)?.ok_or(Error::Integrity))
            .collect()
    }
    fn load(&self, name: &str) -> Result<Option<T>, Error> {
        self.load_links(name, 1)
    }
    fn load_links(&self, name: &str, links: u64) -> Result<Option<T>, Error> {
        let mut file = match openat(&self.directory, name, READ, Mode::empty()) {
            Ok(fd) => File::from(fd),
            Err(Errno::NOENT) => return Ok(None),
            Err(_) => return Err(Error::Unsafe),
        };
        file_valid_links(&file, T::MAX_BYTES as u64, links)?;
        let before = file.metadata().map_err(|_| Error::Unavailable)?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(T::MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Unavailable)?;
        file_valid_links(&file, T::MAX_BYTES as u64, links)?;
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
        T::decode(&bytes).map(Some)
    }
    /// Explicit recovery accepts only the exact complete candidate. No partial
    /// record deletion, conflict replacement or new candidate creation occurs.
    pub(crate) fn recover(path: &str, expected: &T) -> Result<StageOutcome, Error> {
        // Validate the supplied internally constructed candidate before opening.
        if T::decode(&expected.encode())? != *expected {
            return Err(Error::Invalid);
        }
        let store = Self::open_locked(path)?;
        store.check_storage()?;
        let key = expected.key();
        let pending = match openat(&store.directory, T::PENDING, READ, Mode::empty()) {
            Ok(fd) => File::from(fd),
            Err(Errno::NOENT) => {
                store.inventory()?;
                return match store.load(&key)? {
                    Some(record) if record == *expected => Ok(StageOutcome::AlreadyPresent),
                    Some(_) => Err(Error::Conflict),
                    None => Err(Error::Uncertain),
                };
            }
            Err(_) => return Err(Error::Unsafe),
        };
        let links = pending.metadata().map_err(|_| Error::Unavailable)?.nlink();
        if ![1, 2].contains(&links) {
            return Err(Error::Unsafe);
        }
        if store.load_links(T::PENDING, links)?.as_ref() != Some(expected) {
            return Err(Error::Conflict);
        }
        let inventory = store.inventory_with(Some((&key, links)))?;
        if links == 1 {
            if store.load(&key)?.is_some() {
                return Err(Error::Conflict);
            }
            if inventory.len() >= MAX_RECORDS {
                return Err(Error::Capacity);
            }
        } else {
            let final_file = File::from(
                openat(&store.directory, key.as_str(), READ, Mode::empty())
                    .map_err(|_| Error::Uncertain)?,
            );
            if identity(&final_file)? != identity(&pending)?
                || store.load_links(&key, 2)?.as_ref() != Some(expected)
            {
                return Err(Error::Integrity);
            }
        }
        // Recheck pending inode and lock immediately before completing the write.
        store.check_storage()?;
        let current = File::from(
            openat(&store.directory, T::PENDING, READ, Mode::empty())
                .map_err(|_| Error::Uncertain)?,
        );
        if identity(&current)? != identity(&pending)? {
            return Err(Error::Unsafe);
        }
        pending.sync_all().map_err(|_| Error::Uncertain)?;
        if links == 1 {
            linkat(
                &store.directory,
                T::PENDING,
                &store.directory,
                key.as_str(),
                AtFlags::empty(),
            )
            .map_err(|_| Error::Uncertain)?;
        }
        store.directory.sync_all().map_err(|_| Error::Uncertain)?;
        unlinkat(&store.directory, T::PENDING, AtFlags::empty()).map_err(|_| Error::Uncertain)?;
        store.directory.sync_all().map_err(|_| Error::Uncertain)?;
        store.inventory()?;
        if store.load(&key)?.as_ref() != Some(expected) {
            return Err(Error::Uncertain);
        }
        Ok(StageOutcome::Stored)
    }
    /// None means only that these coordinates are absent. Rebound index identity,
    /// unsafe storage and corrupt bytes fail closed without a fallback lookup.
    pub fn read(&self, coordinates: &T::Key) -> Result<Option<T>, Error> {
        self.inventory()?;
        let source = self.load(&T::key_for(coordinates))?;
        if source
            .as_ref()
            .is_some_and(|p| p.coordinates() != coordinates)
        {
            return Err(Error::Conflict);
        }
        Ok(source)
    }
    /// Identical retry is idempotent; every changed byte or provenance at an
    /// occupied version conflicts. An interrupted write blocks further use.
    pub fn stage(&mut self, source: &T) -> Result<StageOutcome, Error> {
        self.stage_using(source, |file, bytes| file.write_all(bytes))
    }
    pub(crate) fn stage_using(
        &mut self,
        source: &T,
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<StageOutcome, Error> {
        let inventory = self.inventory()?;
        let name = source.key();
        if let Some(existing) = self.load(&name)? {
            return if existing == *source {
                Ok(StageOutcome::AlreadyPresent)
            } else {
                Err(Error::Conflict)
            };
        }
        if inventory.len() >= MAX_RECORDS {
            return Err(Error::Capacity);
        }
        let bytes = source.encode();
        // Revalidate even internally generated records before the first mutation.
        if T::decode(&bytes)? != *source {
            return Err(Error::Invalid);
        }
        let mut file = File::from(
            openat(
                &self.directory,
                T::PENDING,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|_| Error::Uncertain)?,
        );
        self.poisoned = true;
        // Preserve the pending record on every failure. No implicit repair/retry.
        write(&mut file, &bytes).map_err(|_| Error::Uncertain)?;
        file.sync_all().map_err(|_| Error::Uncertain)?;
        file_valid(&file, T::MAX_BYTES as u64).map_err(|_| Error::Uncertain)?;
        self.check_storage().map_err(|_| Error::Uncertain)?;
        if self
            .load(T::PENDING)
            .map_err(|_| Error::Uncertain)?
            .as_ref()
            != Some(source)
        {
            return Err(Error::Uncertain);
        }
        let pending = File::from(
            openat(&self.directory, T::PENDING, READ, Mode::empty())
                .map_err(|_| Error::Uncertain)?,
        );
        if identity(&pending).map_err(|_| Error::Uncertain)?
            != identity(&file).map_err(|_| Error::Uncertain)?
        {
            return Err(Error::Uncertain);
        }
        // Hard-link creation is atomic and never replaces an occupied destination.
        linkat(
            &self.directory,
            T::PENDING,
            &self.directory,
            name.as_str(),
            AtFlags::empty(),
        )
        .map_err(|_| Error::Uncertain)?;
        self.directory.sync_all().map_err(|_| Error::Uncertain)?;
        unlinkat(&self.directory, T::PENDING, AtFlags::empty()).map_err(|_| Error::Uncertain)?;
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
    file_valid_links(file, max, 1)
}
fn file_valid_links(file: &File, max: u64, links: u64) -> Result<(), Error> {
    let m = file.metadata().map_err(|_| Error::Unavailable)?;
    if !m.is_file()
        || m.uid() != geteuid().as_raw()
        || m.nlink() != links
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
