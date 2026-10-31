//! Durable account approvals and invitations. The existing owner is never migrated.
//! A retained advisory writer lock excludes a second server; all operations also
//! share the authentication service's worker permit. Secrets never enter views.
use crate::{identity::IdentityId, owner_store::OwnerStore, password};
use rustix::{
    fs::{AtFlags, FlockOperation, Mode, OFlags, flock, openat, renameat, unlinkat},
    io::Errno,
    process::geteuid,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
    time::{SystemTime, UNIX_EPOCH},
};

const LIMIT: usize = 128;
const MAX_BYTES: u64 = 128 * 1024;
const RECORD: &str = "users.json";

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct Accounts {
    version: u8,
    pub registration: bool,
    pub users: Vec<User>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct User {
    pub id: [u8; 16],
    pub email: String,
    pub status: Status,
    hash: String,
    expires: u64,
}
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Status {
    Invited,
    Pending,
    Active,
    Disabled,
}

pub(crate) struct Store {
    directory: File,
    _lock: File,
}
impl Drop for Store {
    fn drop(&mut self) {
        // Explicit unlock also releases a descriptor briefly inherited by a
        // concurrently forked child before its CLOEXEC takes effect.
        let _ = flock(&self._lock, FlockOperation::Unlock);
    }
}
fn invalid() -> io::Error {
    io::Error::other("account storage unavailable")
}
fn private(file: &File, directory: bool) -> io::Result<()> {
    let meta = file.metadata()?;
    if meta.uid() != geteuid().as_raw()
        || meta.mode() & 0o077 != 0
        || (directory && (!meta.is_dir() || meta.nlink() == 0))
        || (!directory && (!meta.is_file() || meta.nlink() != 1))
    {
        return Err(invalid());
    }
    Ok(())
}
pub(crate) fn email(value: &str) -> io::Result<String> {
    if value.len() > 254
        || !value.is_ascii()
        || value
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
    {
        return Err(invalid());
    }
    let (local, domain) = value.split_once('@').ok_or_else(invalid)?;
    if local.is_empty() || domain.is_empty() || domain.contains('@') || !domain.contains('.') {
        return Err(invalid());
    }
    Ok(value.to_ascii_lowercase())
}
fn now() -> io::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid())?
        .as_secs())
}
fn random<const N: usize>() -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| invalid())?;
    Ok(bytes)
}
fn hash(secret: &str) -> io::Result<String> {
    password::hash_password(secret, &random()?).map_err(|_| invalid())
}
impl Store {
    pub fn new(owner: &OwnerStore) -> io::Result<Self> {
        let directory = owner.duplicate_directory()?;
        private(&directory, true)?;
        let lock = File::from(openat(
            &directory,
            ".",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        private(&lock, true)?;
        flock(&lock, FlockOperation::NonBlockingLockExclusive)?;
        let store = Self {
            directory,
            _lock: lock,
        };
        store.load()?;
        Ok(store)
    }
    pub fn load(&self) -> io::Result<Accounts> {
        private(&self.directory, true)?;
        let file = match openat(
            &self.directory,
            RECORD,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => File::from(fd),
            Err(Errno::NOENT) => {
                return Ok(Accounts {
                    version: 1,
                    ..Accounts::default()
                });
            }
            Err(e) => return Err(e.into()),
        };
        private(&file, false)?;
        if file.metadata()?.len() > MAX_BYTES {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid());
        }
        let accounts: Accounts = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if accounts.version != 1 || accounts.users.len() > LIMIT {
            return Err(invalid());
        }
        for (i, user) in accounts.users.iter().enumerate() {
            if email(&user.email)? != user.email
                || password::supported_record(&user.hash).is_none()
                || accounts.users[..i]
                    .iter()
                    .any(|previous| previous.email == user.email || previous.id == user.id)
            {
                return Err(invalid());
            }
        }
        Ok(accounts)
    }
    pub fn save(&self, accounts: &Accounts) -> io::Result<()> {
        self.load()?;
        let bytes = serde_json::to_vec(accounts).map_err(|_| invalid())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid());
        }
        let name = format!(
            ".users-{}.tmp",
            random::<16>()?
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect::<String>()
        );
        let mut file = File::from(openat(
            &self.directory,
            name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )?);
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            private(&self.directory, true)?;
            renameat(&self.directory, name.as_str(), &self.directory, RECORD)?;
            self.directory.sync_all()
        })();
        let _ = unlinkat(&self.directory, name.as_str(), AtFlags::empty());
        result
    }
    pub fn invite(&self, address: &str) -> io::Result<String> {
        let address = email(address)?;
        let mut accounts = self.load()?;
        if accounts.users.len() >= LIMIT || accounts.users.iter().any(|u| u.email == address) {
            return Err(invalid());
        }
        let token = random::<32>()?
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>();
        accounts.users.push(User {
            id: random()?,
            email: address,
            status: Status::Invited,
            hash: hash(&token)?,
            expires: now()?.checked_add(86400).ok_or_else(invalid)?,
        });
        self.save(&accounts)?;
        Ok(token)
    }
    pub fn enroll(&self, address: &str, secret: &str, invitation: &str) -> io::Result<bool> {
        if secret.chars().count() < crate::owner_auth::local_setup::MIN_PASSWORD_CHARACTERS
            || !password::valid_length(secret)
            || secret.chars().any(char::is_control)
        {
            return Err(invalid());
        }
        let address = email(address)?;
        let mut accounts = self.load()?;
        if invitation.is_empty() {
            if !accounts.registration
                || accounts.users.len() >= LIMIT
                || accounts.users.iter().any(|u| u.email == address)
            {
                return Ok(false);
            }
            accounts.users.push(User {
                id: random()?,
                email: address,
                status: Status::Pending,
                hash: hash(secret)?,
                expires: 0,
            });
        } else {
            if invitation.len() != 64 || !invitation.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Ok(false);
            }
            let Some(user) = accounts.users.iter_mut().find(|u| {
                u.email == address
                    && u.status == Status::Invited
                    && now().is_ok_and(|now| now < u.expires)
            }) else {
                return Ok(false);
            };
            if !password::verify_password(invitation, &user.hash) {
                return Ok(false);
            }
            user.hash = hash(secret)?;
            user.status = Status::Active;
            user.expires = 0;
        }
        self.save(&accounts)?;
        Ok(true)
    }
    pub fn authenticate(&self, address: &str, secret: &str) -> io::Result<Option<IdentityId>> {
        let address = match email(address) {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        let accounts = self.load()?;
        let user = accounts.users.iter().find(|u| u.email == address);
        // Unknown/pending accounts still perform the same bounded password work.
        let verified = match user {
            Some(u) => password::verify_password(secret, &u.hash),
            None => {
                hash(secret)?;
                false
            }
        };
        Ok(user
            .filter(|u| verified && u.status == Status::Active)
            .map(|u| IdentityId::from_bytes(u.id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    const PASSWORD: &str = "public test fixture password";
    const EMAIL: &str = "member@example.test";
    fn fixture() -> (tempfile::TempDir, OwnerStore, Store) {
        let directory = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let owner = OwnerStore::open(directory.path()).unwrap();
        let store = Store::new(&owner).unwrap();
        (directory, owner, store)
    }
    #[test]
    fn invitations_expire_cannot_be_reused_and_persist_without_plaintext_credentials() {
        let (directory, owner, store) = fixture();
        let token = store.invite(EMAIL).unwrap();
        assert!(
            !store
                .enroll("other@example.test", PASSWORD, &token)
                .unwrap()
        );
        assert!(!store.enroll(EMAIL, PASSWORD, &"0".repeat(64)).unwrap());
        let mut accounts = store.load().unwrap();
        accounts.users[0].expires = now().unwrap(); // Exact deadline is expired.
        store.save(&accounts).unwrap();
        assert!(!store.enroll(EMAIL, PASSWORD, &token).unwrap());
        accounts.users[0].expires = now().unwrap() + 86400;
        store.save(&accounts).unwrap();
        drop(store);
        let store = Store::new(&owner).unwrap();
        assert!(store.enroll(EMAIL, PASSWORD, &token).unwrap());
        let id = store.authenticate(EMAIL, PASSWORD).unwrap().unwrap();
        assert!(
            !store
                .enroll(EMAIL, "changed public fixture password", &token)
                .unwrap()
        );
        drop(store);
        let store = Store::new(&owner).unwrap();
        assert_eq!(store.authenticate(EMAIL, PASSWORD).unwrap(), Some(id));
        let contents = fs::read_to_string(directory.path().join(RECORD)).unwrap();
        assert!(!contents.contains(PASSWORD));
        assert!(!contents.contains(&token));
        assert_eq!(
            fs::metadata(directory.path().join(RECORD)).unwrap().mode() & 0o777,
            0o600
        );
    }
    #[test]
    fn unsafe_corrupt_and_unsupported_storage_is_never_overwritten_or_reset() {
        let (directory, owner, store) = fixture();
        assert!(Store::new(&owner).is_err()); // Another writer cannot lose approvals.
        store.invite(EMAIL).unwrap();
        let accounts = store.load().unwrap();
        let record = directory.path().join(RECORD);
        let valid = fs::read(&record).unwrap();
        for contents in [
            b"{}".as_slice(),
            b"{\"version\":2,\"registration\":false,\"users\":[]}",
            b"not json",
        ] {
            fs::write(&record, contents).unwrap();
            assert!(store.load().is_err());
            assert!(store.save(&accounts).is_err());
            assert_eq!(fs::read(&record).unwrap(), contents);
        }
        fs::write(&record, &valid).unwrap();
        fs::set_permissions(&record, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.load().is_err());
        fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
        let alias = directory.path().join("alias");
        fs::hard_link(&record, &alias).unwrap();
        assert!(store.load().is_err());
        fs::remove_file(&alias).unwrap();
        fs::remove_file(&record).unwrap();
        symlink(&alias, &record).unwrap();
        assert!(store.load().is_err());
        assert!(store.save(&accounts).is_err());
    }
    #[test]
    fn registration_and_account_limits_are_durable_and_fail_closed() {
        let (_directory, owner, store) = fixture();
        assert!(!store.enroll(EMAIL, PASSWORD, "").unwrap());
        let mut accounts = store.load().unwrap();
        accounts.registration = true;
        store.save(&accounts).unwrap();
        assert!(store.enroll("MEMBER@example.test", PASSWORD, "").unwrap());
        assert!(!store.enroll(EMAIL, PASSWORD, "").unwrap());
        assert_eq!(store.authenticate(EMAIL, PASSWORD).unwrap(), None);
        assert!(store.enroll("short@example.test", "too short", "").is_err());
        assert!(
            store
                .enroll("control@example.test", "public fixture password\n", "")
                .is_err()
        );
        let mut accounts = store.load().unwrap();
        accounts.users[0].status = Status::Active;
        store.save(&accounts).unwrap();
        drop(store);
        let store = Store::new(&owner).unwrap();
        assert!(store.authenticate(EMAIL, PASSWORD).unwrap().is_some());
        let mut accounts = store.load().unwrap();
        for number in 1..LIMIT {
            accounts.users.push(User {
                id: [number as u8; 16],
                email: format!("member{number}@example.test"),
                status: Status::Pending,
                hash: accounts.users[0].hash.clone(),
                expires: 0,
            });
        }
        store.save(&accounts).unwrap();
        assert!(store.invite("extra@example.test").is_err());
        assert!(!store.enroll("extra@example.test", PASSWORD, "").unwrap());
    }
}
