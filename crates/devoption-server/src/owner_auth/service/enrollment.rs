//! Locally armed enrollment authorization, separate from browser transport.
//!
//! Only an explicit local setup operation may call `begin_local`; HTTP reachability
//! or an ordinary pre-authentication challenge is not operator authority. This
//! component has no production caller or HTTP route. Secrets stay in memory here.
//! See docs/local-enrollment.md for the trust boundary and unfinished handoff.

use super::{OwnerAuthService, ServiceError};
use crate::{identity::IdentityId, owner_auth, owner_store::OwnerStore};
use std::{
    collections::VecDeque,
    error::Error,
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};
use subtle::ConstantTimeEq;

const LIFETIME: Duration = Duration::from_secs(300);
const AUDIT_LIMIT: usize = 16;
static NEXT_ATTEMPT: AtomicU64 = AtomicU64::new(1);

/// A one-use launch credential, intentionally not Debug or Clone. Transport must
/// not put it in logs, request URLs, referrers, or persistent browser storage.
pub struct LaunchCredential([u8; 32]);

impl LaunchCredential {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// An independent cookie/CSRF pair for one browser submission, not a login session.
/// The future HTTP adapter must keep the cookie HttpOnly and require explicit CSRF.
pub struct BrowserAuthorization {
    cookie: [u8; 32],
    csrf: [u8; 32],
}

impl BrowserAuthorization {
    pub fn cookie(&self) -> &[u8; 32] {
        &self.cookie
    }
    pub fn csrf(&self) -> &[u8; 32] {
        &self.csrf
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollmentError {
    Denied,
    Expired,
    AlreadyConfigured,
    InvalidPassword,
    Busy,
    RateLimited { retry_after: Duration },
    Unavailable,
}

impl fmt::Display for EnrollmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Denied => "enrollment authorization denied",
            Self::Expired => "enrollment attempt expired",
            Self::AlreadyConfigured => "owner already configured",
            Self::InvalidPassword => "invalid enrollment password input",
            Self::Busy => "owner authentication worker is busy",
            Self::RateLimited { .. } => "owner authentication attempt limit reached",
            Self::Unavailable => "enrollment unavailable",
        })
    }
}

impl Error for EnrollmentError {}

impl From<ServiceError> for EnrollmentError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::Busy => Self::Busy,
            ServiceError::RateLimited { retry_after } => Self::RateLimited { retry_after },
            _ => Self::Unavailable,
        }
    }
}

/// Fixed vocabulary only: no user input, tokens, passwords, hashes, or paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditKind {
    Armed,
    ClaimStarted,
    BrowserBound,
    Denied,
    Expired,
    EnrollmentAccepted,
    OwnerCreated,
    AlreadyConfigured,
    Failed,
    CompletionUncertain,
}

#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub at: SystemTime,
    pub kind: AuditKind,
}

/// Process-local evidence, not a durable or tamper-proof audit trail. The public
/// counter identifies an attempt for diagnostics and grants no authority.
#[derive(Debug, Clone)]
pub struct AuditSnapshot {
    pub attempt_id: u64,
    pub omitted_events: u64,
    pub events: Vec<AuditEvent>,
}

enum Phase {
    Waiting([u8; 32]),
    Claiming,
    Bound { cookie: [u8; 32], csrf: [u8; 32] },
    Enrolling,
    Closed,
}

struct State {
    attempt_id: u64,
    expires_at: Instant,
    phase: Phase,
    events: VecDeque<AuditEvent>,
    omitted_events: u64,
}

impl State {
    fn record(&mut self, kind: AuditKind) {
        if self.events.len() == AUDIT_LIMIT {
            self.events.pop_front();
            self.omitted_events = self.omitted_events.saturating_add(1);
        }
        self.events.push_back(AuditEvent {
            at: SystemTime::now(),
            kind,
        });
    }

    fn live(&mut self) -> Result<(), EnrollmentError> {
        if matches!(self.phase, Phase::Closed) {
            self.record(AuditKind::Denied);
            return Err(EnrollmentError::Denied);
        }
        if Instant::now() >= self.expires_at {
            self.phase = Phase::Closed;
            self.record(AuditKind::Expired);
            return Err(EnrollmentError::Expired);
        }
        Ok(())
    }
}

/// Clones share the same attempt, store, admission limits, and audit buffer.
/// One fixed five-minute deadline covers both claim and enrollment admission.
#[derive(Clone)]
pub struct EnrollmentAttempt {
    auth: OwnerAuthService,
    state: Arc<Mutex<State>>,
}

impl EnrollmentAttempt {
    pub async fn begin_local(
        auth: OwnerAuthService,
    ) -> Result<(Self, LaunchCredential), EnrollmentError> {
        Self::begin_using(auth, getrandom::fill).await
    }

    async fn begin_using(
        auth: OwnerAuthService,
        mut random: impl FnMut(&mut [u8]) -> Result<(), getrandom::Error> + Send + 'static,
    ) -> Result<(Self, LaunchCredential), EnrollmentError> {
        let result = auth
            .run(move |store| {
                Ok((|| {
                    unconfigured(store)?;
                    let mut token = [0; 32];
                    random(&mut token).map_err(|_| EnrollmentError::Unavailable)?;
                    let attempt_id = NEXT_ATTEMPT
                        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                        .map_err(|_| EnrollmentError::Unavailable)?;
                    let expires_at = Instant::now()
                        .checked_add(LIFETIME)
                        .ok_or(EnrollmentError::Unavailable)?;
                    let mut state = State {
                        attempt_id,
                        expires_at,
                        phase: Phase::Waiting(token),
                        events: VecDeque::new(),
                        omitted_events: 0,
                    };
                    state.record(AuditKind::Armed);
                    Ok::<_, EnrollmentError>((state, LaunchCredential(token)))
                })())
            })
            .await??;
        Ok((
            Self {
                auth,
                state: Arc::new(Mutex::new(result.0)),
            },
            result.1,
        ))
    }

    pub async fn claim(&self, launch: &[u8]) -> Result<BrowserAuthorization, EnrollmentError> {
        self.claim_using(launch, getrandom::fill).await
    }

    async fn claim_using(
        &self,
        launch: &[u8],
        mut random: impl FnMut(&mut [u8]) -> Result<(), getrandom::Error> + Send + 'static,
    ) -> Result<BrowserAuthorization, EnrollmentError> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| EnrollmentError::Unavailable)?;
            state.live()?;
            if !matches!(&state.phase, Phase::Waiting(expected) if bool::from(expected.as_slice().ct_eq(launch)))
            {
                state.record(AuditKind::Denied);
                return Err(EnrollmentError::Denied);
            }
            // Burn the launch credential before admission/await; a cancelled or
            // failed claim cannot reopen it or deliver it to another browser.
            state.phase = Phase::Claiming;
            state.record(AuditKind::ClaimStarted);
        }
        let shared = Arc::clone(&self.state);
        let result = self
            .auth
            .run(move |store| {
                let outcome = (|| {
                    unconfigured(store)?;
                    let mut cookie = [0; 32];
                    let mut csrf = [0; 32];
                    random(&mut cookie)
                        .and_then(|()| random(&mut csrf))
                        .map_err(|_| EnrollmentError::Unavailable)?;
                    let mut state = shared.lock().map_err(|_| EnrollmentError::Unavailable)?;
                    state.live()?;
                    if !matches!(state.phase, Phase::Claiming) {
                        return Err(EnrollmentError::Denied);
                    }
                    state.phase = Phase::Bound { cookie, csrf };
                    state.record(AuditKind::BrowserBound);
                    Ok(BrowserAuthorization { cookie, csrf })
                })();
                if let Err(error) = outcome {
                    close(&shared, error_kind(error));
                }
                Ok(outcome)
            })
            .await;
        self.finish_admission(result, AuditKind::Failed)
    }

    /// Consume the browser pair before work starts. Wrong/mixed pairs cannot
    /// consume it. Any admitted failure requires a new locally armed attempt.
    /// Input bounds apply today; enrollment password strength policy is unfinished.
    pub async fn enroll(
        &self,
        cookie: &[u8],
        csrf: &[u8],
        password: String,
    ) -> Result<IdentityId, EnrollmentError> {
        self.enroll_using(cookie, csrf, password, owner_auth::setup)
            .await
    }

    async fn enroll_using(
        &self,
        cookie: &[u8],
        csrf: &[u8],
        password: String,
        setup: impl FnOnce(&OwnerStore, &str) -> Result<IdentityId, owner_auth::SetupError>
        + Send
        + 'static,
    ) -> Result<IdentityId, EnrollmentError> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| EnrollmentError::Unavailable)?;
            state.live()?;
            if !matches!(&state.phase, Phase::Bound { cookie: expected_cookie, csrf: expected_csrf }
                if bool::from(expected_cookie.as_slice().ct_eq(cookie) & expected_csrf.as_slice().ct_eq(csrf)))
            {
                state.record(AuditKind::Denied);
                return Err(EnrollmentError::Denied);
            }
            state.phase = Phase::Enrolling;
            state.record(AuditKind::EnrollmentAccepted);
        }
        let shared = Arc::clone(&self.state);
        let result = self
            .auth
            .run(move |store| {
                let outcome = (|| {
                    {
                        let mut state = shared.lock().map_err(|_| EnrollmentError::Unavailable)?;
                        state.live()?;
                        if !matches!(state.phase, Phase::Enrolling) {
                            return Err(EnrollmentError::Denied);
                        }
                        state.phase = Phase::Closed;
                    }
                    unconfigured(store)?;
                    setup(store, &password).map_err(|error| match error {
                        owner_auth::SetupError::Store(
                            crate::owner_store::StoreError::AlreadyExists,
                        ) => EnrollmentError::AlreadyConfigured,
                        owner_auth::SetupError::Password(_) => EnrollmentError::InvalidPassword,
                        _ => EnrollmentError::Unavailable,
                    })
                })();
                // Record completion inside the blocking worker, even if its caller
                // was cancelled. An I/O failure can follow publication: never reset.
                close(
                    &shared,
                    match outcome {
                        Ok(_) => AuditKind::OwnerCreated,
                        Err(EnrollmentError::Unavailable) => AuditKind::CompletionUncertain,
                        Err(error) => error_kind(error),
                    },
                );
                Ok(outcome)
            })
            .await;
        self.finish_admission(result, AuditKind::CompletionUncertain)
    }

    fn finish_admission<T>(
        &self,
        result: Result<Result<T, EnrollmentError>, ServiceError>,
        unavailable_kind: AuditKind,
    ) -> Result<T, EnrollmentError> {
        match result {
            Ok(outcome) => outcome,
            Err(error) => {
                close(
                    &self.state,
                    if matches!(error, ServiceError::Unavailable) {
                        unavailable_kind
                    } else {
                        AuditKind::Failed
                    },
                );
                Err(error.into())
            }
        }
    }

    pub fn audit(&self) -> Result<AuditSnapshot, EnrollmentError> {
        let state = self
            .state
            .lock()
            .map_err(|_| EnrollmentError::Unavailable)?;
        Ok(AuditSnapshot {
            attempt_id: state.attempt_id,
            omitted_events: state.omitted_events,
            events: state.events.iter().cloned().collect(),
        })
    }
}

fn unconfigured(store: &OwnerStore) -> Result<(), EnrollmentError> {
    match store.load() {
        Ok(None) => Ok(()),
        Ok(Some(_)) => Err(EnrollmentError::AlreadyConfigured),
        Err(_) => Err(EnrollmentError::Unavailable),
    }
}

fn error_kind(error: EnrollmentError) -> AuditKind {
    match error {
        EnrollmentError::Expired => AuditKind::Expired,
        EnrollmentError::AlreadyConfigured => AuditKind::AlreadyConfigured,
        _ => AuditKind::Failed,
    }
}

fn close(shared: &Mutex<State>, kind: AuditKind) {
    if let Ok(mut state) = shared.lock() {
        state.phase = Phase::Closed;
        state.record(kind);
    }
}

#[cfg(test)]
mod tests;
