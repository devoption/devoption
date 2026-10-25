//! Private, bounded snapshots. One service process owns a namespace lock.
//! Not a boundary against the OS user that owns the directory. Sessions are
//! deliberately volatile; only account IDs, hashes and authorization persist.
use super::*;
use crate::owner_store::OwnerStore;
use rustix::{
    fs::{AtFlags, FlockOperation, Mode, OFlags, flock, openat, renameat, unlinkat},
    io::Errno,
    process::geteuid,
};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    sync::{
        MutexGuard, TryLockError,
        atomic::{AtomicU64, Ordering},
    },
};

const MAX_BYTES: usize = 4 * 1024 * 1024;
static TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub(super) struct Account {
    pub id: AccountId,
    pub email: String,
    pub hash: String,
    pub role: AppRole,
    pub enabled: bool,
    pub revision: u64,
    pub memberships: Vec<Membership>,
}
pub(super) struct Data {
    pub accounts: Vec<Account>,
    persisted: bool,
    broken: bool,
}
pub(super) struct Store {
    directory: File,
    lock: File,
    app: AppId,
    record: String,
    data: Mutex<Data>,
}
fn unavailable<T>(_: T) -> Error {
    Error::Unavailable
}
fn private(file: &File, directory: bool) -> Result<(), Error> {
    let m = file.metadata().map_err(unavailable)?;
    if m.uid() != geteuid().as_raw()
        || m.mode() & 0o077 != 0
        || (directory && (!m.is_dir() || m.nlink() == 0))
        || (!directory && (!m.is_file() || m.nlink() != 1 || m.len() > MAX_BYTES as u64))
    {
        return Err(Error::Unavailable);
    }
    Ok(())
}
impl Store {
    pub fn open(path: &Path, app: AppId) -> Result<Self, Error> {
        // Reuse only the descriptor opener; never read or create an owner record.
        let directory = OwnerStore::open_absolute(path)
            .map_err(unavailable)?
            .duplicate_directory()
            .map_err(unavailable)?;
        let name = format!("app-{}.lock", app.0);
        let lock = File::from(
            openat(
                &directory,
                name.as_str(),
                OFlags::RDWR
                    | OFlags::CREATE
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(unavailable)?,
        );
        private(&lock, false)?;
        flock(&lock, FlockOperation::NonBlockingLockExclusive).map_err(|_| Error::Busy)?;
        let record = format!("app-{}.json", app.0);
        let mut store = Self {
            directory,
            lock,
            app,
            record,
            data: Mutex::new(Data {
                accounts: vec![],
                persisted: false,
                broken: false,
            }),
        };
        let accounts = store.load()?;
        let data = store.data.get_mut().map_err(unavailable)?;
        data.persisted = accounts.is_some();
        data.accounts = accounts.unwrap_or_default();
        Ok(store)
    }

    fn load(&self) -> Result<Option<Vec<Account>>, Error> {
        let file = match openat(
            &self.directory,
            self.record.as_str(),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => File::from(fd),
            Err(Errno::NOENT) => return Ok(None),
            Err(_) => return Err(Error::Unavailable),
        };
        private(&file, false)?;
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(unavailable)?;
        if bytes.len() > MAX_BYTES {
            return Err(Error::Unavailable);
        }
        decode(&bytes, &self.app).map(Some)
    }

    pub fn data(&self) -> Result<MutexGuard<'_, Data>, Error> {
        // A synchronous authorization guard must not block an async host thread
        // behind the password worker or a durable snapshot write.
        let data = self.data.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => Error::Busy,
            TryLockError::Poisoned(_) => Error::Unavailable,
        })?;
        if data.broken {
            return Err(Error::Unavailable);
        }
        private(&self.directory, true)?;
        private(&self.lock, false)?;
        // Refuse removed/unsafe storage rather than silently recreating accounts.
        if data.persisted {
            let file = File::from(
                openat(
                    &self.directory,
                    self.record.as_str(),
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(unavailable)?,
            );
            private(&file, false)?;
        }
        Ok(data)
    }

    pub fn commit(&self, data: &mut Data, accounts: Vec<Account>) -> Result<(), Error> {
        let result = self.save(&accounts);
        if result.is_err() {
            // A rename may have succeeded before fsync failed. Fail closed until
            // an explicit reopen; never keep authorizing the old in-memory roles.
            data.broken = true;
            return Err(Error::Unavailable);
        }
        data.accounts = accounts;
        data.persisted = true;
        Ok(())
    }
    fn save(&self, accounts: &[Account]) -> Result<(), Error> {
        let records: Vec<_> = accounts
            .iter()
            .map(|a| {
                json!([
                    a.id.0,
                    a.email,
                    a.hash,
                    if a.role == AppRole::Admin {
                        "admin"
                    } else {
                        "member"
                    },
                    a.enabled,
                    a.revision,
                    a.memberships
                        .iter()
                        .map(|m| json!([
                            m.organization.0,
                            if m.role == OrganizationRole::Admin {
                                "admin"
                            } else {
                                "member"
                            }
                        ]))
                        .collect::<Vec<_>>()
                ])
            })
            .collect();
        let bytes = serde_json::to_vec(&json!([1, self.app.0, records])).map_err(unavailable)?;
        if bytes.len() > MAX_BYTES {
            return Err(Error::Capacity);
        }
        for _ in 0..32 {
            let name = format!(
                ".app-{}-{}-{}.tmp",
                self.app.0,
                std::process::id(),
                TEMP.fetch_add(1, Ordering::Relaxed)
            );
            let fd = match openat(
                &self.directory,
                name.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(fd) => fd,
                Err(Errno::EXIST) => continue,
                Err(_) => return Err(Error::Unavailable),
            };
            let pending = Pending {
                directory: &self.directory,
                name,
            };
            let mut file = File::from(fd);
            file.write_all(&bytes).map_err(unavailable)?;
            file.sync_all().map_err(unavailable)?;
            private(&self.directory, true)?;
            private(&self.lock, false)?;
            renameat(
                &self.directory,
                pending.name.as_str(),
                &self.directory,
                self.record.as_str(),
            )
            .map_err(unavailable)?;
            self.directory.sync_all().map_err(unavailable)?;
            return Ok(());
        }
        Err(Error::Unavailable)
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

// Fixed-shape arrays avoid JSON duplicate-object-key ambiguities. Every field
// and length is checked; unknown versions, extra fields and roles fail closed.
fn decode(bytes: &[u8], app: &AppId) -> Result<Vec<Account>, Error> {
    fn array(value: &Value, count: usize) -> Result<&[Value], Error> {
        value
            .as_array()
            .filter(|a| a.len() == count)
            .map(Vec::as_slice)
            .ok_or(Error::Unavailable)
    }
    fn text(value: &Value) -> Result<&str, Error> {
        value.as_str().ok_or(Error::Unavailable)
    }
    let v: Value = serde_json::from_slice(bytes).map_err(unavailable)?;
    let root = array(&v, 3)?;
    if root[0].as_u64() != Some(1) || text(&root[1])? != app.0 {
        return Err(Error::Unavailable);
    }
    let entries = root[2]
        .as_array()
        .filter(|a| a.len() <= MAX_ACCOUNTS)
        .ok_or(Error::Unavailable)?;
    let mut accounts: Vec<Account> = Vec::new();
    for entry in entries {
        let fields = array(entry, 7)?;
        let id = text(&fields[0])?;
        let address = text(&fields[1])?;
        let hash = text(&fields[2])?;
        let revision = fields[5]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or(Error::Unavailable)?;
        if !valid_id(id)
            || email(address).as_deref() != Ok(address)
            || password::supported_record(hash).is_none()
            || accounts.iter().any(|a| a.id.0 == id || a.email == address)
        {
            return Err(Error::Unavailable);
        }
        let role = match text(&fields[3])? {
            "admin" => AppRole::Admin,
            "member" => AppRole::Member,
            _ => return Err(Error::Unavailable),
        };
        let enabled = fields[4].as_bool().ok_or(Error::Unavailable)?;
        let values = fields[6]
            .as_array()
            .filter(|a| a.len() <= MAX_ORGANIZATIONS)
            .ok_or(Error::Unavailable)?;
        let mut memberships = Vec::new();
        for value in values {
            let m = array(value, 2)?;
            let organization = OrganizationId::parse(text(&m[0])?).map_err(unavailable)?;
            let role = match text(&m[1])? {
                "admin" => OrganizationRole::Admin,
                "member" => OrganizationRole::Member,
                _ => return Err(Error::Unavailable),
            };
            memberships.push(Membership { organization, role });
        }
        if !memberships_valid(&memberships) {
            return Err(Error::Unavailable);
        }
        accounts.push(Account {
            id: AccountId(id.into()),
            email: address.into(),
            hash: hash.into(),
            role,
            enabled,
            revision,
            memberships,
        });
    }
    Ok(accounts)
}
