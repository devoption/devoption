//! One authoritative in-memory session, shared by activation and request validation.
//!
//! Activation replaces the prior session; logout requires its current live token.
//! Share one slot across all relevant handlers. A new slot/process starts empty.
//! Rotation and logout affect subsequent validations, not already-authorized work.
//! No HTTP activation/logout endpoints or credential transport are implemented here.

use std::{error::Error, fmt, sync::Mutex, time::Instant};

use super::Session;
use crate::identity::IdentityId;

/// Activation failed without replacing the current session. Contains no credential.
#[derive(Debug, PartialEq, Eq)]
pub enum ActivationError {
    InvalidSession,
    Unavailable,
}

impl fmt::Display for ActivationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidSession => "session grant is expired, revoked, or mismatched",
            Self::Unavailable => "session state unavailable",
        })
    }
}

impl Error for ActivationError {}

/// A single session slot. Share with `Arc`; cloning independent state is unsupported.
#[derive(Default)]
pub struct SessionSlot {
    active: Mutex<Option<Session>>,
}

impl SessionSlot {
    /// Atomically retain a live session, replacing the previous one without renewal.
    /// The issuer must have authenticated its identity and supplied fresh entropy.
    /// Validating the session/token pair here does not authenticate a password.
    pub fn activate(&self, session: Session, token: &[u8]) -> Result<(), ActivationError> {
        self.activate_with_clock(session, token, Instant::now)
    }

    fn activate_with_clock(
        &self,
        mut session: Session,
        token: &[u8],
        now: impl FnOnce() -> Instant,
    ) -> Result<(), ActivationError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| ActivationError::Unavailable)?;
        // Observe expiry after acquiring the lock, so contention cannot use stale time.
        if session.authenticate_at(Some(token), now()).is_none() {
            return Err(ActivationError::InvalidSession);
        }
        *active = Some(session);
        Ok(())
    }

    /// Validate only the current live session; absent or poisoned state fails closed.
    /// This does not validate CSRF; browser mutations must use `authenticate_browser`.
    pub fn authenticate(&self, candidate: Option<&[u8]>) -> Option<IdentityId> {
        self.authenticate_with_clock(candidate, Instant::now)
    }

    fn authenticate_with_clock(
        &self,
        candidate: Option<&[u8]>,
        now: impl FnOnce() -> Instant,
    ) -> Option<IdentityId> {
        self.active
            .lock()
            .ok()?
            .as_mut()?
            .authenticate_at(candidate, now())
    }

    /// Validate both browser credentials against one current session under one lock.
    /// The CSRF token must come from an explicit request field/header, not a cookie.
    /// Cookie transport and Host/Origin validation remain the HTTP caller's duty.
    pub fn authenticate_browser(
        &self,
        candidate: Option<&[u8]>,
        csrf: Option<&[u8]>,
    ) -> Option<IdentityId> {
        self.browser_with_clock(candidate, csrf, false, Instant::now)
    }

    /// Remove only the live session identified by the matching browser token pair.
    /// Missing, mismatched, or stale CSRF tokens cannot revoke a replacement session.
    pub fn logout_browser(&self, candidate: Option<&[u8]>, csrf: Option<&[u8]>) -> bool {
        self.browser_with_clock(candidate, csrf, true, Instant::now)
            .is_some()
    }

    fn browser_with_clock(
        &self,
        candidate: Option<&[u8]>,
        csrf: Option<&[u8]>,
        logout: bool,
        now: impl FnOnce() -> Instant,
    ) -> Option<IdentityId> {
        let mut active = self.active.lock().ok()?;
        let identity = active
            .as_mut()?
            .authenticate_browser_at(candidate, csrf, now())?;
        if logout {
            *active = None;
        }
        Some(identity)
    }

    /// Remove the session only when this request presents its current live token.
    /// A stale logout cannot revoke a replacement session. Poisoned state returns false.
    /// For bearer-authorized callers; cookie-authenticated logout must use `logout_browser`.
    pub fn logout(&self, candidate: Option<&[u8]>) -> bool {
        self.logout_with_clock(candidate, Instant::now)
    }

    fn logout_with_clock(&self, candidate: Option<&[u8]>, now: impl FnOnce() -> Instant) -> bool {
        let Ok(mut active) = self.active.lock() else {
            return false;
        };
        let valid = active
            .as_mut()
            .and_then(|session| session.authenticate_at(candidate, now()))
            .is_some();
        if valid {
            *active = None;
        }
        valid
    }
}

#[cfg(test)]
mod tests;
