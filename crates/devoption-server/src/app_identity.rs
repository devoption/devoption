//! Native application accounts, separate from platform owners and HTTP transport.
//! The embedding host must independently authorize all operator methods. Never
//! expose them as guest imports or public enrollment endpoints. No service starts
//! automatically, and constructing an ID or role grants no authenticated access.

mod store;

use crate::password;
use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use store::{Account, Store};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;

const MAX_ACCOUNTS: usize = 1000;
const MAX_ORGANIZATIONS: usize = 64;
const MAX_SESSIONS: usize = 256;
const MAX_ATTEMPTS: usize = 12;
const ATTEMPT_WINDOW: Duration = Duration::from_secs(60);
const SESSION_LIFETIME: Duration = Duration::from_secs(3600);

/// Deliberately small errors with no credential, account or filesystem details.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Exists,
    NotFound,
    Busy,
    RateLimited,
    Capacity,
    Unavailable,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "invalid application identity input",
            Self::Exists => "application account already exists",
            Self::NotFound => "application account not found",
            Self::Busy => "application identity worker busy",
            Self::RateLimited => "application sign-in attempt limit reached",
            Self::Capacity => "application identity capacity reached",
            Self::Unavailable => "application identity service unavailable",
        })
    }
}
impl std::error::Error for Error {}

fn valid_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Operator-selected application namespace, independent of platform owner IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppId(String);
impl AppId {
    pub fn parse(value: &str) -> Result<Self, Error> {
        valid_id(value)
            .then(|| Self(value.into()))
            .ok_or(Error::Invalid)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrganizationId(String);
impl OrganizationId {
    pub fn parse(value: &str) -> Result<Self, Error> {
        valid_id(value)
            .then(|| Self(value.into()))
            .ok_or(Error::Invalid)
    }
}

/// Only provisioning generates account IDs; no owner-ID conversion exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountId(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppRole {
    Admin,
    Member,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrganizationRole {
    Admin,
    Member,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Membership {
    pub organization: OrganizationId,
    pub role: OrganizationRole,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Permission {
    AccessPortal,
    AccessAdmin,
    ReadOrganization(OrganizationId),
    ManageOrganization(OrganizationId),
}

/// A point-in-time authorization result. Do not cache it across requests.
#[derive(Debug, PartialEq, Eq)]
pub struct Principal {
    pub account: AccountId,
    pub role: AppRole,
}

/// Secret raw credentials for a future host-only cookie/explicit CSRF transport.
/// Intentionally neither Debug nor serializable. No URL or wire encoding here.
pub struct Credentials {
    token: [u8; 32],
    csrf: [u8; 32],
}
impl Credentials {
    pub fn session_token(&self) -> &[u8; 32] {
        &self.token
    }
    pub fn csrf_token(&self) -> &[u8; 32] {
        &self.csrf
    }
}

struct Session {
    account: AccountId,
    revision: u64,
    token: [u8; 32],
    csrf: [u8; 32],
    expires: Instant,
}

struct State {
    store: Store,
    worker: Arc<Semaphore>,
    attempts: Mutex<VecDeque<Instant>>,
    sessions: Mutex<Vec<Session>>,
    dummy_hash: String,
}

/// Clone shares authoritative account/session state. Reopening invalidates all
/// sessions; an exclusive namespace lock refuses concurrent service processes.
#[derive(Clone)]
pub struct AppIdentityService {
    state: Arc<State>,
}

fn random<const N: usize>() -> Result<[u8; N], Error> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
    Ok(bytes)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn email(value: &str) -> Result<String, Error> {
    // Explicit ASCII, case-insensitive login identifier policy; no provider rules.
    let value = value.trim();
    let (local, domain) = value.split_once('@').ok_or(Error::Invalid)?;
    let valid = value.len() <= 254
        && !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&b))
        && domain.contains('.')
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    valid
        .then(|| value.to_ascii_lowercase())
        .ok_or(Error::Invalid)
}
fn enrollment_password(value: &str) -> bool {
    value.chars().count() >= 15 && password::valid_length(value)
}
fn memberships_valid(values: &[Membership]) -> bool {
    values.len() <= MAX_ORGANIZATIONS
        && values.iter().enumerate().all(|(i, m)| {
            !values[..i]
                .iter()
                .any(|other| other.organization == m.organization)
        })
}

impl AppIdentityService {
    /// Blocking startup only. Uses an existing absolute private directory;
    /// creates only namespace files, never an OS user, owner or application user.
    pub fn open(directory: &Path, app: AppId) -> Result<Self, Error> {
        let store = Store::open(directory, app)?;
        let dummy_hash = password::hash_password(&hex(&random::<32>()?), &random()?)
            .map_err(|_| Error::Unavailable)?;
        Ok(Self {
            state: Arc::new(State {
                store,
                worker: Arc::new(Semaphore::new(1)),
                attempts: Mutex::new(VecDeque::new()),
                sessions: Mutex::new(Vec::new()),
                dummy_hash,
            }),
        })
    }

    async fn work<T: Send + 'static>(
        &self,
        task: impl FnOnce(&State) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let permit = self
            .state
            .worker
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            // Cancellation must not release admission before hashing actually ends.
            let _permit = permit;
            task(&state)
        })
        .await
        .map_err(|_| Error::Unavailable)?
    }

    /// Operator API: caller must authorize provisioning independently. There is
    /// no first-user elevation, self-registration or role field on sign-in.
    pub async fn provision(
        &self,
        address: String,
        secret: String,
        role: AppRole,
    ) -> Result<AccountId, Error> {
        let address = email(&address)?;
        if !enrollment_password(&secret) {
            return Err(Error::Invalid);
        }
        self.work(move |state| {
            let mut data = state.store.data()?;
            if data.accounts.iter().any(|a| a.email == address) {
                return Err(Error::Exists);
            }
            if data.accounts.len() >= MAX_ACCOUNTS {
                return Err(Error::Capacity);
            }
            let id = AccountId(hex(&random::<16>()?));
            if data.accounts.iter().any(|a| a.id == id) {
                return Err(Error::Unavailable);
            }
            let hash =
                password::hash_password(&secret, &random()?).map_err(|_| Error::Unavailable)?;
            let mut next = data.accounts.clone();
            next.push(Account {
                id: id.clone(),
                email: address,
                hash,
                role,
                enabled: true,
                revision: 1,
                memberships: vec![],
            });
            state.store.commit(&mut data, next)?;
            Ok(id)
        })
        .await
    }

    /// Generic failure (`None`) for unknown, disabled and incorrect credentials.
    /// Unknown addresses still run one bounded Argon2 verification. Limits are
    /// shared by this app's clones; they are not a distributed abuse-control API.
    pub async fn sign_in(
        &self,
        address: String,
        secret: String,
    ) -> Result<Option<Credentials>, Error> {
        let Ok(address) = email(&address) else {
            return Ok(None);
        };
        if !password::valid_length(&secret) {
            return Ok(None);
        }
        {
            let mut attempts = self.state.attempts.lock().map_err(|_| Error::Unavailable)?;
            let now = Instant::now();
            attempts.retain(|t| now.duration_since(*t) < ATTEMPT_WINDOW);
            if attempts.len() >= MAX_ATTEMPTS {
                return Err(Error::RateLimited);
            }
            attempts.push_back(now);
        }
        self.work(move |state| {
            // Serialize account updates with verification and activation. A
            // disable/reset cannot race in between these operations.
            let data = state.store.data()?;
            let account = data.accounts.iter().find(|a| a.email == address);
            let hash = account.map_or(state.dummy_hash.as_str(), |a| a.hash.as_str());
            let verified = password::verify_password(&secret, hash);
            let Some(account) = account.filter(|a| verified && a.enabled) else {
                return Ok(None);
            };
            let credentials = Credentials {
                token: random()?,
                csrf: random()?,
            };
            let mut sessions = state.sessions.lock().map_err(|_| Error::Unavailable)?;
            let now = Instant::now();
            sessions.retain(|s| s.expires > now);
            if sessions.len() >= MAX_SESSIONS {
                return Err(Error::Capacity);
            }
            sessions.push(Session {
                account: account.id.clone(),
                revision: account.revision,
                token: credentials.token,
                csrf: credentials.csrf,
                expires: now + SESSION_LIFETIME,
            });
            Ok(Some(credentials))
        })
        .await
    }

    /// Operator API: replace complete authorization configuration. The host
    /// must authorize this action; an organization admin must never call it.
    /// Every change invalidates existing sessions, including role downgrades.
    pub async fn configure(
        &self,
        id: AccountId,
        role: AppRole,
        enabled: bool,
        memberships: Vec<Membership>,
    ) -> Result<(), Error> {
        if !memberships_valid(&memberships) {
            return Err(Error::Invalid);
        }
        self.update(id, move |account| {
            account.role = role;
            account.enabled = enabled;
            account.memberships = memberships;
            Ok(())
        })
        .await
    }

    /// Operator-authorized credential update, not a public password-reset flow.
    pub async fn replace_password(&self, id: AccountId, secret: String) -> Result<(), Error> {
        if !enrollment_password(&secret) {
            return Err(Error::Invalid);
        }
        self.update(id, move |account| {
            account.hash =
                password::hash_password(&secret, &random()?).map_err(|_| Error::Unavailable)?;
            Ok(())
        })
        .await
    }

    async fn update(
        &self,
        id: AccountId,
        update: impl FnOnce(&mut Account) -> Result<(), Error> + Send + 'static,
    ) -> Result<(), Error> {
        self.work(move |state| {
            let mut data = state.store.data()?;
            let mut next = data.accounts.clone();
            let account = next
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or(Error::NotFound)?;
            update(account)?;
            account.revision = account.revision.checked_add(1).ok_or(Error::Unavailable)?;
            state.store.commit(&mut data, next)?;
            // Store lock then sessions lock is the shared order in every path.
            state
                .sessions
                .lock()
                .map_err(|_| Error::Unavailable)?
                .retain(|s| s.account != id);
            Ok(())
        })
        .await
    }

    pub fn authorize_read(
        &self,
        token: &[u8],
        permission: Permission,
    ) -> Result<Option<Principal>, Error> {
        self.authorize(token, None, permission, false)
    }

    /// Host must additionally enforce exact HTTPS origin and method policy.
    /// CSRF must be explicitly submitted, never obtained from a second cookie.
    pub fn authorize_mutation(
        &self,
        token: &[u8],
        csrf: &[u8],
        permission: Permission,
    ) -> Result<Option<Principal>, Error> {
        self.authorize(token, Some(csrf), permission, false)
    }

    /// Authenticated, CSRF-protected logout. Cannot revoke another token pair.
    pub fn logout(&self, token: &[u8], csrf: &[u8]) -> Result<bool, Error> {
        Ok(self
            .authorize(token, Some(csrf), Permission::AccessPortal, true)?
            .is_some())
    }

    fn authorize(
        &self,
        token: &[u8],
        csrf: Option<&[u8]>,
        permission: Permission,
        logout: bool,
    ) -> Result<Option<Principal>, Error> {
        let data = self.state.store.data()?;
        let mut sessions = self.state.sessions.lock().map_err(|_| Error::Unavailable)?;
        let now = Instant::now();
        sessions.retain(|s| s.expires > now);
        let Some(index) = sessions
            .iter()
            .position(|s| bool::from(s.token.as_slice().ct_eq(token)))
        else {
            return Ok(None);
        };
        let session = &sessions[index];
        if csrf.is_some_and(|candidate| !bool::from(session.csrf.as_slice().ct_eq(candidate))) {
            return Ok(None);
        }
        let Some(account) = data
            .accounts
            .iter()
            .find(|a| a.id == session.account && a.enabled && a.revision == session.revision)
        else {
            return Ok(None);
        };
        let allowed = match permission {
            Permission::AccessPortal => true,
            Permission::AccessAdmin => account.role == AppRole::Admin,
            Permission::ReadOrganization(ref org) => {
                account.memberships.iter().any(|m| &m.organization == org)
            }
            Permission::ManageOrganization(ref org) => account
                .memberships
                .iter()
                .any(|m| &m.organization == org && m.role == OrganizationRole::Admin),
        };
        if !allowed {
            return Ok(None);
        }
        let principal = Principal {
            account: account.id.clone(),
            role: account.role,
        };
        if logout {
            sessions.remove(index);
        }
        Ok(Some(principal))
    }
}

#[cfg(test)]
mod tests;
