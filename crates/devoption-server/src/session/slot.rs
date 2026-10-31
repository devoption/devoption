//! Bounded in-memory sessions, with one independently rotating session per identity.
//!
//! Activation replaces only the same identity’s prior session; logout is token-scoped.
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

/// Shared sessions. Share with `Arc`; cloning independent state is unsupported.
#[derive(Default)]
pub struct SessionSlot {
    active: Mutex<Vec<Session>>,
}

impl SessionSlot {
    /// Atomically retain a live session, replacing only this account’s prior session.
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
        let now = now();
        if session.authenticate_at(Some(token), now).is_none() {
            return Err(ActivationError::InvalidSession);
        }
        active.retain(|entry| entry.expires_at > now && entry.identity != session.identity);
        if active.len() >= 129 {
            return Err(ActivationError::Unavailable);
        }
        active.push(session);
        Ok(())
    }

    /// Validate a live session; absent or poisoned state fails closed.
    /// This does not validate CSRF; browser mutations must use `authenticate_browser`.
    pub fn authenticate(&self, candidate: Option<&[u8]>) -> Option<IdentityId> {
        self.authenticate_with_clock(candidate, Instant::now)
    }

    fn authenticate_with_clock(
        &self,
        candidate: Option<&[u8]>,
        now: impl FnOnce() -> Instant,
    ) -> Option<IdentityId> {
        let mut active = self.active.lock().ok()?;
        let now = now();
        active
            .iter_mut()
            .find_map(|session| session.authenticate_at(candidate, now))
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

    /// Read the current session's existing CSRF token, with no rotation or renewal.
    /// Only a same-origin, cookie-authenticated recovery handler may expose it.
    pub fn recover_browser_csrf(&self, candidate: &[u8]) -> Option<[u8; super::CSRF_TOKEN_BYTES]> {
        self.recover_csrf_with_clock(candidate, Instant::now)
    }

    fn recover_csrf_with_clock(
        &self,
        candidate: &[u8],
        now: impl FnOnce() -> Instant,
    ) -> Option<[u8; super::CSRF_TOKEN_BYTES]> {
        let mut active = self.active.lock().ok()?;
        let now = now();
        active
            .iter_mut()
            .find_map(|session| session.recover_csrf_at(candidate, now))
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
        let now = now();
        let index = active.iter_mut().position(|session| {
            session
                .authenticate_browser_at(candidate, csrf, now)
                .is_some()
        })?;
        let identity = active[index].identity;
        if logout {
            active.remove(index);
        }
        Some(identity)
    }

    /// Revoke all sessions for an account disabled by its administrator.
    #[cfg(any(unix, test))]
    pub(crate) fn revoke_identity(&self, identity: IdentityId) {
        if let Ok(mut active) = self.active.lock() {
            active.retain(|entry| entry.identity != identity);
        }
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
        let now = now();
        let index = active
            .iter_mut()
            .position(|session| session.authenticate_at(candidate, now).is_some());
        if let Some(index) = index {
            active.remove(index);
        }
        index.is_some()
    }
}

#[cfg(test)]
mod tests;
