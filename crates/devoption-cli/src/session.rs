//! In-memory validation of one opaque session token, independent of HTTP.
//!
//! This core neither generates tokens nor authenticates their owner. A future
//! issuer must supply fresh, cryptographically random tokens after authorized
//! bootstrap and retain this object as the authoritative validation state.
//! Revocation affects this object; constructing another with the same token would
//! defeat it. There is no persistence, renewal, or secure memory-erasure guarantee.
//! The HTTP guard accepts these tokens on unsafe requests, but issuance and
//! complete CSRF protection remain separate work under #15.

use std::{
    error::Error,
    fmt,
    time::{Duration, Instant},
};

use subtle::ConstantTimeEq;

/// Required raw token length; this module does not choose a wire encoding.
pub const TOKEN_BYTES: usize = 32;

/// Hard ceiling for a session's absolute lifetime, with no sliding renewal.
pub const MAX_LIFETIME: Duration = Duration::from_secs(60 * 60);

/// Lifetime was zero, exceeded the ceiling, or could not form a deadline.
#[derive(Debug, PartialEq, Eq)]
pub struct InvalidLifetime;

impl fmt::Display for InvalidLifetime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("session lifetime must be positive and at most one hour")
    }
}

impl Error for InvalidLifetime {}

/// One non-cloneable, expiring session. Tokens are deliberately not printable.
pub struct Session {
    token: Option<[u8; TOKEN_BYTES]>,
    expires_at: Instant,
}

impl Session {
    /// Retain an already-issued token for a bounded lifetime starting now.
    ///
    /// The caller is responsible for token entropy and authorized issuance.
    /// No production caller provisions sessions in the current preview.
    pub fn new(token: [u8; TOKEN_BYTES], lifetime: Duration) -> Result<Self, InvalidLifetime> {
        Self::new_at(token, lifetime, Instant::now())
    }

    fn new_at(
        token: [u8; TOKEN_BYTES],
        lifetime: Duration,
        now: Instant,
    ) -> Result<Self, InvalidLifetime> {
        if lifetime.is_zero() || lifetime > MAX_LIFETIME {
            return Err(InvalidLifetime);
        }
        let expires_at = now.checked_add(lifetime).ok_or(InvalidLifetime)?;
        Ok(Self {
            token: Some(token),
            expires_at,
        })
    }

    /// Reject missing, incorrect, expired, or revoked tokens without renewal.
    ///
    /// Equal-length token contents are compared using `subtle::ConstantTimeEq`.
    /// Length and session lifecycle state are not secret. An observed expiry is
    /// latched as revocation, and candidates never create or reactivate sessions.
    pub fn validate(&mut self, candidate: Option<&[u8]>) -> bool {
        self.validate_at(candidate, Instant::now())
    }

    fn validate_at(&mut self, candidate: Option<&[u8]>, now: Instant) -> bool {
        if now >= self.expires_at {
            self.revoke();
        }
        let (Some(token), Some(candidate)) = (&self.token, candidate) else {
            return false;
        };
        bool::from(token.as_slice().ct_eq(candidate))
    }

    /// Permanently invalidate this object. Repeated revocation is harmless.
    pub fn revoke(&mut self) {
        self.token = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Deliberately predictable test data, never a production credential.
    const FIXTURE: [u8; TOKEN_BYTES] = [0x5a; TOKEN_BYTES];

    #[test]
    fn rejects_missing_wrong_length_and_every_single_byte_mismatch() {
        let now = Instant::now();
        let mut session = Session::new_at(FIXTURE, MAX_LIFETIME, now).unwrap();
        assert!(!session.validate_at(None, now));
        for candidate in [&[][..], &FIXTURE[..31], &[0x5a; TOKEN_BYTES + 1]] {
            assert!(!session.validate_at(Some(candidate), now));
        }
        for index in 0..TOKEN_BYTES {
            let mut candidate = FIXTURE;
            candidate[index] ^= 1;
            assert!(!session.validate_at(Some(&candidate), now));
        }
        assert!(session.validate_at(Some(&FIXTURE), now));
    }

    #[test]
    fn expiry_is_absolute_and_rejects_at_the_deadline() {
        let now = Instant::now();
        let mut session = Session::new_at(FIXTURE, MAX_LIFETIME, now).unwrap();
        let deadline = now + MAX_LIFETIME;
        assert!(session.validate_at(Some(&FIXTURE), now));
        assert!(session.validate_at(Some(&FIXTURE), deadline - Duration::from_nanos(1)));
        assert!(!session.validate_at(Some(&FIXTURE), deadline));
        assert!(!session.validate_at(Some(&FIXTURE), deadline + MAX_LIFETIME));
        // Even a clock anomaly cannot revive an observed expired session.
        assert!(!session.validate_at(Some(&FIXTURE), now));
    }

    #[test]
    fn a_missing_candidate_still_observes_expiry() {
        let now = Instant::now();
        let mut session = Session::new_at(FIXTURE, MAX_LIFETIME, now).unwrap();
        assert!(!session.validate_at(None, now + MAX_LIFETIME));
        assert!(!session.validate_at(Some(&FIXTURE), now));
    }

    #[test]
    fn revocation_is_permanent_and_idempotent() {
        let now = Instant::now();
        let mut session = Session::new_at(FIXTURE, MAX_LIFETIME, now).unwrap();
        assert!(session.validate_at(Some(&FIXTURE), now));
        session.revoke();
        assert!(!session.validate_at(Some(&FIXTURE), now));
        session.revoke();
        assert!(!session.validate_at(Some(&FIXTURE), now));
        assert!(!session.validate_at(Some(&FIXTURE), now + MAX_LIFETIME));
    }

    #[test]
    fn lifetime_must_be_positive_and_bounded() {
        let now = Instant::now();
        for invalid in [
            Duration::ZERO,
            MAX_LIFETIME + Duration::from_nanos(1),
            Duration::MAX,
        ] {
            assert!(matches!(
                Session::new_at(FIXTURE, invalid, now),
                Err(InvalidLifetime)
            ));
        }
        for valid in [Duration::from_nanos(1), MAX_LIFETIME] {
            let mut session = Session::new_at(FIXTURE, valid, now).unwrap();
            assert!(session.validate_at(Some(&FIXTURE), now));
            assert!(!session.validate_at(Some(&FIXTURE), now + valid));
        }
    }

    #[test]
    fn public_api_validates_and_revokes_a_fixture() {
        let mut session = Session::new(FIXTURE, MAX_LIFETIME).unwrap();
        assert!(session.validate(Some(&FIXTURE)));
        session.revoke();
        assert!(!session.validate(Some(&FIXTURE)));
    }
}
