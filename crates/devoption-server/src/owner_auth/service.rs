//! Bounded async access to owner setup and password authentication.
//!
//! Retain one service for all owner authentication routes; clones share its state.
//! One operation may be in flight, and at most five operations are admitted per
//! rolling minute, including successes and failures. Busy requests are rejected
//! immediately. Limits are in memory: a new service or process starts fresh.
//!
//! Existing-owner HTTP sign-in uses this service with cookie transport and
//! pre-authentication CSRF adapters. Enrollment authorization/policy and browser
//! forms remain unfinished. This grants no permissions.

use std::{
    collections::VecDeque,
    error::Error,
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};

use crate::{
    identity::IdentityId,
    owner_auth::{self, SetupError},
    owner_store::{OwnerStore, StoreError},
    session::{
        CSRF_TOKEN_BYTES, MAX_LIFETIME, Session, TOKEN_BYTES,
        slot::{ActivationError, SessionSlot},
    },
};

const MAX_ATTEMPTS: usize = 5;
const ATTEMPT_WINDOW: Duration = Duration::from_secs(60);

pub mod enrollment;

/// A fresh session with its two credentials; intentionally neither Debug nor Clone.
/// Issuance alone does not persist, print, or activate either credential.
pub struct IssuedSession {
    session: Session,
    token: [u8; TOKEN_BYTES],
    csrf_token: [u8; CSRF_TOKEN_BYTES],
}

/// Credentials released only after successful activation; neither Debug nor Clone.
/// Transport must keep the session credential in an HttpOnly cookie and
/// send the separate CSRF token in a protected response, then require it explicitly
/// on mutations. Never place either token in logs, URLs, or browser-local storage.
pub struct BrowserCredentials {
    session_token: [u8; TOKEN_BYTES],
    csrf_token: [u8; CSRF_TOKEN_BYTES],
}

impl BrowserCredentials {
    pub fn session_token(&self) -> &[u8; TOKEN_BYTES] {
        &self.session_token
    }

    pub fn csrf_token(&self) -> &[u8; CSRF_TOKEN_BYTES] {
        &self.csrf_token
    }
}

impl IssuedSession {
    /// Access the session and bearer credential without returning its CSRF token.
    /// Prefer [`Self::activate_browser`] for browser transport. Never log credentials.
    pub fn into_parts(self) -> (Session, [u8; TOKEN_BYTES]) {
        (self.session, self.token)
    }

    /// Consume this grant into the authoritative slot before returning its credential.
    /// Replaces the previous session only if this grant is still valid; never renews it.
    /// Browser callers must use `activate_browser` to retain the separate CSRF token.
    pub fn activate(self, slot: &SessionSlot) -> Result<[u8; TOKEN_BYTES], ActivationError> {
        let (session, token) = self.into_parts();
        slot.activate(session, &token)?;
        Ok(token)
    }

    /// Activate the grant before releasing its session and synchronizer CSRF tokens.
    /// HTTP enrollment/sign-in protection remains separate from the transport adapter.
    pub fn activate_browser(
        self,
        slot: &SessionSlot,
    ) -> Result<BrowserCredentials, ActivationError> {
        slot.activate(self.session, &self.token)?;
        Ok(BrowserCredentials {
            session_token: self.token,
            csrf_token: self.csrf_token,
        })
    }
}

/// Failures contain no password, salt, hash, or worker panic details.
#[derive(Debug)]
pub enum ServiceError {
    Busy,
    RateLimited { retry_after: Duration },
    Unavailable,
    Setup(SetupError),
    Store(StoreError),
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => formatter.write_str("owner authentication worker is busy"),
            Self::RateLimited { .. } => {
                formatter.write_str("owner authentication attempt limit reached")
            }
            Self::Unavailable => formatter.write_str("owner authentication service unavailable"),
            Self::Setup(error) => error.fmt(formatter),
            Self::Store(error) => error.fmt(formatter),
        }
    }
}

impl Error for ServiceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Setup(error) => Some(error),
            Self::Store(error) => Some(error),
            _ => None,
        }
    }
}

struct State {
    store: OwnerStore,
    worker: Arc<Semaphore>,
    attempts: Mutex<VecDeque<Instant>>,
}

/// A shared admission boundary, to be used from a Tokio runtime.
///
/// Cancelling an async caller does not stop its already-submitted blocking work.
/// The worker slot remains held until that work exits; the attempt stays charged.
/// Setup may still publish after cancellation. Inspect storage before retrying;
/// never delete or reset the owner record to recover a cancelled request.
#[derive(Clone)]
pub struct OwnerAuthService {
    state: Arc<State>,
}

impl OwnerAuthService {
    pub fn new(store: OwnerStore) -> Self {
        Self {
            state: Arc::new(State {
                store,
                worker: Arc::new(Semaphore::new(1)),
                attempts: Mutex::new(VecDeque::with_capacity(MAX_ATTEMPTS)),
            }),
        }
    }

    /// Run independently authorized setup off the runtime thread.
    /// Passwords are owned so the worker can finish safely after cancellation.
    pub async fn setup(&self, password: String) -> Result<IdentityId, ServiceError> {
        self.run(move |store| owner_auth::setup(store, &password).map_err(ServiceError::Setup))
            .await
    }

    /// Verify the password off the runtime thread; success grants no session.
    pub async fn authenticate(&self, password: String) -> Result<Option<IdentityId>, ServiceError> {
        self.run(move |store| {
            owner_auth::authenticate(store, &password).map_err(ServiceError::Store)
        })
        .await
    }

    /// Verify the password, then issue independent session and CSRF credentials
    /// bound to one session with a one-hour absolute lifetime.
    /// The caller supplies no identity, token, or lifetime. HTTP transport is separate.
    pub async fn sign_in(&self, password: String) -> Result<Option<IssuedSession>, ServiceError> {
        self.run(move |store| {
            sign_in_with_entropy(store, &password, |bytes| {
                getrandom::fill(bytes).map_err(|_| ServiceError::Unavailable)
            })
        })
        .await
    }

    async fn run<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&OwnerStore) -> Result<T, ServiceError> + Send + 'static,
    ) -> Result<T, ServiceError> {
        let permit = self.admit_at(Instant::now())?;
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || {
            // Move the permit into the actual worker, not the cancellable caller.
            let _permit = permit;
            operation(&state.store)
        })
        .await
        .map_err(|_| ServiceError::Unavailable)?
    }

    fn admit_at(&self, now: Instant) -> Result<OwnedSemaphorePermit, ServiceError> {
        let permit =
            Arc::clone(&self.state.worker)
                .try_acquire_owned()
                .map_err(|error| match error {
                    TryAcquireError::NoPermits => ServiceError::Busy,
                    TryAcquireError::Closed => ServiceError::Unavailable,
                })?;
        let mut attempts = self
            .state
            .attempts
            .lock()
            .map_err(|_| ServiceError::Unavailable)?;
        while attempts
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= ATTEMPT_WINDOW)
        {
            attempts.pop_front();
        }
        if attempts.len() >= MAX_ATTEMPTS {
            let oldest = *attempts.front().expect("a full attempt window is nonempty");
            return Err(ServiceError::RateLimited {
                retry_after: ATTEMPT_WINDOW.saturating_sub(now.saturating_duration_since(oldest)),
            });
        }
        attempts.push_back(now);
        Ok(permit)
    }
}

fn sign_in_with_entropy(
    store: &OwnerStore,
    password: &str,
    mut fill: impl FnMut(&mut [u8]) -> Result<(), ServiceError>,
) -> Result<Option<IssuedSession>, ServiceError> {
    let Some(identity) = owner_auth::authenticate(store, password).map_err(ServiceError::Store)?
    else {
        return Ok(None);
    };
    let mut token = [0; TOKEN_BYTES];
    fill(&mut token)?;
    let mut csrf_token = [0; CSRF_TOKEN_BYTES];
    fill(&mut csrf_token)?;
    let session = Session::new(identity, token, MAX_LIFETIME)
        .map_err(|_| ServiceError::Unavailable)?
        .with_csrf(csrf_token);
    Ok(Some(IssuedSession {
        session,
        token,
        csrf_token,
    }))
}

#[cfg(test)]
mod tests;
