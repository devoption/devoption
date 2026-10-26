//! Opt-in owner-managed encrypted snapshots. No guest/build secret delivery.
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use devoption_projects::ProjectId;
use rustix::{
    fs::{AtFlags, FlockOperation, Mode, OFlags, flock, open, openat, renameat, unlinkat},
    io::Errno,
    process::geteuid,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::fs::{FileExt, MetadataExt},
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

pub(crate) const MAX_VALUE: usize = 8192;
const MAX_FILE: usize = 1024 * 1024;
const MAX_ENTRIES: usize = 64;
const MAX_EVENTS: usize = 100;
const MAGIC: &[u8] = b"DevOption secrets v1\0";
const LOCK: &str = ".project-secrets.lock";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Environment {
    Development,
    Preview,
    Production,
}
impl Environment {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "development" => Some(Self::Development),
            "preview" => Some(Self::Preview),
            "production" => Some(Self::Production),
            _ => None,
        }
    }
    pub(crate) fn segment(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Preview => "preview",
            Self::Production => "production",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Development => "Development",
            Self::Preview => "Preview",
            Self::Production => "Production",
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Invalid,
    Conflict,
    Capacity,
    Unavailable,
    Uncertain,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "Secret input is invalid.",
            Self::Conflict => "Secret changed. Reload before trying again.",
            Self::Capacity => "Environment secret limit reached.",
            Self::Unavailable => "Secret storage is unavailable.",
            Self::Uncertain => {
                "Save could not be confirmed. Reload to check saved state before retrying."
            }
        })
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::Unavailable
    }
}
impl From<Errno> for Error {
    fn from(_: Errno) -> Self {
        Self::Unavailable
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    value: Zeroizing<String>,
    revision: u64,
    updated: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Event {
    pub(crate) name: String,
    pub(crate) action: String,
    pub(crate) revision: u64,
    pub(crate) updated: u64,
}
#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct State {
    sequence: u64,
    entries: Vec<Entry>,
    events: Vec<Event>,
}
pub(crate) struct Metadata {
    pub(crate) name: String,
    pub(crate) revision: u64,
    pub(crate) updated: u64,
}
pub(crate) struct Listing {
    pub(crate) entries: Vec<Metadata>,
    pub(crate) events: Vec<Event>,
}

/// Never Debug/Serialize: this capability retains a key and directory, not a path.
pub(crate) struct Store {
    directory: File,
    key_file: File,
    key: Zeroizing<[u8; 32]>,
    lock: File,
    owner: [u8; 16],
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = flock(&self.lock, FlockOperation::Unlock);
    }
}
impl Store {
    pub(crate) fn open(
        directory: File,
        key_path: &Path,
        owner: [u8; 16],
        forbidden: &[&Path],
    ) -> Result<Self, Error> {
        directory_valid(&directory)?;
        let parent = key_path.parent().ok_or(Error::Unavailable)?;
        let key_parent = open_directory(parent)?;
        if contains(&directory, &key_parent)? {
            return Err(Error::Unavailable);
        }
        for path in forbidden {
            if contains(&open_directory(path)?, &key_parent)? {
                return Err(Error::Unavailable);
            }
        }
        let name = key_path.file_name().ok_or(Error::Unavailable)?;
        let key_file = File::from(openat(&key_parent, name, read_flags(), Mode::empty())?);
        file_valid(&key_file)?;
        if key_file.metadata()?.len() != 32 {
            return Err(Error::Unavailable);
        }
        let mut key = Zeroizing::new([0; 32]);
        key_file.read_exact_at(key.as_mut(), 0)?;
        let lock = File::from(openat(
            &directory,
            LOCK,
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )?);
        file_valid(&lock)?;
        flock(&lock, FlockOperation::NonBlockingLockExclusive)?;
        let store = Self {
            directory,
            key_file,
            key,
            lock,
            owner,
        };
        store.check()?;
        Ok(store)
    }
    fn check(&self) -> Result<(), Error> {
        directory_valid(&self.directory)?;
        file_valid(&self.key_file)?;
        file_valid(&self.lock)?;
        let current = File::from(openat(&self.directory, LOCK, read_flags(), Mode::empty())?);
        if identity(&current)? != identity(&self.lock)? || self.key_file.metadata()?.len() != 32 {
            return Err(Error::Unavailable);
        }
        let mut bytes = Zeroizing::new([0; 32]);
        self.key_file.read_exact_at(bytes.as_mut(), 0)?;
        use subtle::ConstantTimeEq;
        if !bool::from(bytes.as_slice().ct_eq(self.key.as_slice())) {
            return Err(Error::Unavailable);
        }
        Ok(())
    }
    fn filename(project: ProjectId, env: Environment) -> String {
        format!(
            "project-secrets-{}-{}.bin",
            hex(&project.to_bytes()),
            env.segment()
        )
    }
    fn aad(&self, project: ProjectId, env: Environment) -> Vec<u8> {
        let mut aad = MAGIC.to_vec();
        aad.extend(self.owner);
        aad.extend(project.to_bytes());
        aad.extend(env.segment().as_bytes());
        aad
    }
    fn load(&self, project: ProjectId, env: Environment) -> Result<State, Error> {
        self.check()?;
        let file = match openat(
            &self.directory,
            Self::filename(project, env).as_str(),
            read_flags(),
            Mode::empty(),
        ) {
            Ok(fd) => File::from(fd),
            Err(Errno::NOENT) => return Ok(State::default()),
            Err(_) => return Err(Error::Unavailable),
        };
        file_valid(&file)?;
        if file.metadata()?.len() > MAX_FILE as u64 {
            return Err(Error::Unavailable);
        }
        let mut bytes = Vec::new();
        file.take((MAX_FILE + 1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_FILE
            || bytes.len() < MAGIC.len() + 24 + 16
            || !bytes.starts_with(MAGIC)
        {
            return Err(Error::Unavailable);
        }
        let nonce = XNonce::try_from(&bytes[MAGIC.len()..MAGIC.len() + 24])
            .map_err(|_| Error::Unavailable)?;
        let cipher = XChaCha20Poly1305::new_from_slice(self.key.as_slice())
            .map_err(|_| Error::Unavailable)?;
        let plain = Zeroizing::new(
            cipher
                .decrypt(
                    &nonce,
                    Payload {
                        msg: &bytes[MAGIC.len() + 24..],
                        aad: &self.aad(project, env),
                    },
                )
                .map_err(|_| Error::Unavailable)?,
        );
        let state: State = serde_json::from_slice(&plain).map_err(|_| Error::Unavailable)?;
        validate_state(&state)?;
        Ok(state)
    }
    fn save(&self, project: ProjectId, env: Environment, state: &State) -> Result<(), Error> {
        self.save_with_sync(project, env, state, |directory| directory.sync_all())
    }
    fn save_with_sync(
        &self,
        project: ProjectId,
        env: Environment,
        state: &State,
        sync: impl FnOnce(&File) -> std::io::Result<()>,
    ) -> Result<(), Error> {
        self.check()?;
        validate_state(state)?;
        let plain = Zeroizing::new(serde_json::to_vec(state).map_err(|_| Error::Unavailable)?);
        let mut nonce = [0; 24];
        getrandom::fill(&mut nonce).map_err(|_| Error::Unavailable)?;
        let cipher = XChaCha20Poly1305::new_from_slice(self.key.as_slice())
            .map_err(|_| Error::Unavailable)?;
        let encrypted = cipher
            .encrypt(
                &nonce.into(),
                Payload {
                    msg: &plain,
                    aad: &self.aad(project, env),
                },
            )
            .map_err(|_| Error::Unavailable)?;
        let mut bytes = MAGIC.to_vec();
        bytes.extend(nonce);
        bytes.extend(encrypted);
        if bytes.len() > MAX_FILE {
            return Err(Error::Capacity);
        }
        let mut random = [0; 16];
        getrandom::fill(&mut random).map_err(|_| Error::Unavailable)?;
        let temp = format!(".project-secrets-{}.tmp", hex(&random));
        let result = (|| {
            let mut file = File::from(openat(
                &self.directory,
                temp.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )?);
            file_valid(&file)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            self.check()?;
            renameat(
                &self.directory,
                temp.as_str(),
                &self.directory,
                Self::filename(project, env).as_str(),
            )?;
            sync(&self.directory).map_err(|_| Error::Uncertain)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = unlinkat(&self.directory, temp.as_str(), AtFlags::empty());
        }
        result
    }
    pub(crate) fn list(&self, project: ProjectId, env: Environment) -> Result<Listing, Error> {
        let state = self.load(project, env)?;
        Ok(Listing {
            entries: state
                .entries
                .iter()
                .map(|e| Metadata {
                    name: e.name.clone(),
                    revision: e.revision,
                    updated: e.updated,
                })
                .collect(),
            events: state.events,
        })
    }
    /// Compare-and-set prevents an old browser tab from overwriting a replacement.
    pub(crate) fn set(
        &self,
        project: ProjectId,
        env: Environment,
        name: &str,
        value: Zeroizing<String>,
        expected: u64,
    ) -> Result<(), Error> {
        if !valid_name(name) || !valid_value(&value) {
            return Err(Error::Invalid);
        }
        let mut state = self.load(project, env)?;
        let found = state.entries.iter().position(|e| e.name == name);
        if found.map_or(0, |i| state.entries[i].revision) != expected {
            return Err(Error::Conflict);
        }
        if found.is_none() && state.entries.len() >= MAX_ENTRIES {
            return Err(Error::Capacity);
        }
        let revision = state.sequence.checked_add(1).ok_or(Error::Capacity)?;
        let updated = now()?;
        let entry = Entry {
            name: name.into(),
            value,
            revision,
            updated,
        };
        if let Some(i) = found {
            state.entries[i] = entry
        } else {
            state.entries.push(entry)
        }
        state.entries.sort_by(|a, b| a.name.cmp(&b.name));
        record(
            &mut state,
            name,
            if found.is_some() { "replaced" } else { "added" },
            revision,
            updated,
        );
        self.save(project, env, &state)
    }
    pub(crate) fn remove(
        &self,
        project: ProjectId,
        env: Environment,
        name: &str,
        expected: u64,
    ) -> Result<(), Error> {
        if !valid_name(name) || expected == 0 {
            return Err(Error::Invalid);
        }
        let mut state = self.load(project, env)?;
        let index = state
            .entries
            .iter()
            .position(|e| e.name == name)
            .ok_or(Error::Conflict)?;
        if state.entries[index].revision != expected {
            return Err(Error::Conflict);
        }
        state.entries.remove(index);
        let revision = state.sequence.checked_add(1).ok_or(Error::Capacity)?;
        record(&mut state, name, "removed", revision, now()?);
        self.save(project, env, &state)
    }
}
fn record(state: &mut State, name: &str, action: &str, revision: u64, updated: u64) {
    state.sequence = revision;
    state.events.push(Event {
        name: name.into(),
        action: action.into(),
        revision,
        updated,
    });
    if state.events.len() > MAX_EVENTS {
        state.events.remove(0);
    }
}
pub(crate) fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .enumerate()
            .all(|(i, c)| c.is_ascii_uppercase() || c == b'_' || (i > 0 && c.is_ascii_digit()))
}
fn valid_value(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_VALUE && !s.contains('\0')
}
fn validate_state(s: &State) -> Result<(), Error> {
    if s.entries.len() > MAX_ENTRIES || s.events.len() > MAX_EVENTS {
        return Err(Error::Unavailable);
    }
    let mut last = "";
    for e in &s.entries {
        if !valid_name(&e.name)
            || !valid_value(&e.value)
            || e.revision == 0
            || e.revision > s.sequence
            || e.name.as_str() <= last
        {
            return Err(Error::Unavailable);
        }
        last = &e.name;
    }
    let mut previous = 0;
    for e in &s.events {
        if !valid_name(&e.name)
            || !matches!(e.action.as_str(), "added" | "replaced" | "removed")
            || e.revision <= previous
            || e.revision > s.sequence
        {
            return Err(Error::Unavailable);
        }
        previous = e.revision;
    }
    if previous != s.sequence {
        return Err(Error::Unavailable);
    }
    Ok(())
}
fn now() -> Result<u64, Error> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|t| t.as_secs())
        .map_err(|_| Error::Unavailable)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn read_flags() -> OFlags {
    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC
}
fn identity(f: &File) -> Result<(u64, u64), Error> {
    let m = f.metadata()?;
    Ok((m.dev(), m.ino()))
}
fn file_valid(f: &File) -> Result<(), Error> {
    let m = f.metadata()?;
    if !m.is_file()
        || m.nlink() != 1
        || m.uid() != geteuid().as_raw()
        || !matches!(m.mode() & 0o7777, 0o400 | 0o600)
    {
        return Err(Error::Unavailable);
    }
    Ok(())
}
fn directory_valid(f: &File) -> Result<(), Error> {
    let m = f.metadata()?;
    if !m.is_dir() || m.uid() != geteuid().as_raw() || m.mode() & 0o7077 != 0 {
        return Err(Error::Unavailable);
    }
    Ok(())
}
fn open_directory(p: &Path) -> Result<File, Error> {
    if !p.is_absolute()
        || p.components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(Error::Unavailable);
    }
    let flags = read_flags() | OFlags::DIRECTORY;
    let mut f = File::from(open("/", flags, Mode::empty())?);
    for c in p.components() {
        if let Component::Normal(n) = c {
            f = File::from(openat(&f, n, flags, Mode::empty())?)
        }
    }
    Ok(f)
}
fn contains(ancestor: &File, directory: &File) -> Result<bool, Error> {
    let mut current = directory.try_clone()?;
    for _ in 0..512 {
        let id = identity(&current)?;
        if id == identity(ancestor)? {
            return Ok(true);
        }
        let parent = File::from(openat(
            &current,
            "..",
            read_flags() | OFlags::DIRECTORY,
            Mode::empty(),
        )?);
        if identity(&parent)? == id {
            return Ok(false);
        }
        current = parent;
    }
    Err(Error::Unavailable)
}
#[cfg(test)]
mod tests;
