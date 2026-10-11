//! Bounded async access to owner setup and password authentication.
//!
//! Retain one service for all owner authentication routes; clones share its state.
//! One operation may be in flight, and at most five operations are admitted per
//! rolling minute, including successes and failures. Busy requests are rejected
//! immediately. Limits are in memory: a new service or process starts fresh.
//!
//! Only temporary fixtures use this service today. HTTP integration still needs
//! enrollment authorization/policy, bounded request bodies, data-directory setup,
//! authoritative session retention, cookies, and CSRF handling. This grants no permissions.

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
    session::{MAX_LIFETIME, Session, TOKEN_BYTES},
};

const MAX_ATTEMPTS: usize = 5;
const ATTEMPT_WINDOW: Duration = Duration::from_secs(60);

/// A fresh session paired with its credential; intentionally neither Debug nor Clone.
/// No credential is persisted, printed, or installed in HTTP state by this grant.
pub struct IssuedSession {
    session: Session,
    token: [u8; TOKEN_BYTES],
}

impl IssuedSession {
    /// Retain the validator as authoritative server state and deliver the credential
    /// through the future browser cookie transport. Never log or display the token.
    /// Keeping multiple validators alive requires an explicit session lifecycle policy.
    pub fn into_parts(self) -> (Session, [u8; TOKEN_BYTES]) {
        (self.session, self.token)
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

    /// Verify the password, then issue a fresh session with a one-hour absolute lifetime.
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
    fill: impl FnOnce(&mut [u8]) -> Result<(), ServiceError>,
) -> Result<Option<IssuedSession>, ServiceError> {
    let Some(identity) = owner_auth::authenticate(store, password).map_err(ServiceError::Store)?
    else {
        return Ok(None);
    };
    let mut token = [0; TOKEN_BYTES];
    fill(&mut token)?;
    let session =
        Session::new(identity, token, MAX_LIFETIME).map_err(|_| ServiceError::Unavailable)?;
    Ok(Some(IssuedSession { session, token }))
}

#[cfg(test)]
mod tests;
