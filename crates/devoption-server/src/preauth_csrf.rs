//! Single-use synchronizer challenges before browser password authentication.
//!
//! Callers must validate Host/Origin before issuance or consumption, require POST,
//! and consume before reading a password body or starting authentication work.
//! A challenge grants no identity, session, or permission to enroll an owner.
//! Existing-owner HTTP sign-in uses this adapter; enrollment remains closed.
//!
//! Keep one shared instance per server. Challenges expire without renewal, and
//! successful consumption destroys server state even if later password work fails
//! or is cancelled. The leftover cookie is inert and the next challenge replaces
//! it; consume does not send a clearing cookie that could erase a newer challenge.
//! Authentication must issue a fresh session, never promote the pre-session.
//! Secure-cookie browser support and same-host/other-port trust limitations are
//! the same as for [`crate::browser_session`].

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    http::{
        HeaderMap, StatusCode,
        header::{RETRY_AFTER, SET_COOKIE},
    },
    response::Response,
};
use subtle::ConstantTimeEq;

use crate::{
    browser_session::{
        COOKIE_ATTRIBUTES, CSRF_HEADER, empty, encode, named_credentials, sensitive,
    },
    session::{CSRF_TOKEN_BYTES, TOKEN_BYTES},
};

const COOKIE_NAME: &str = "__Host-devoption_preauth";
const LIFETIME: Duration = Duration::from_secs(5 * 60);
const MAX_PENDING: usize = 16;

struct Challenge {
    cookie: [u8; TOKEN_BYTES],
    csrf: [u8; CSRF_TOKEN_BYTES],
    expires_at: Instant,
}

/// Bounded pre-session storage. Secrets are deliberately neither Debug nor Clone.
#[derive(Default)]
pub struct PreAuthCsrf {
    pending: Mutex<Vec<Challenge>>,
}

impl PreAuthCsrf {
    /// Return a distinct HttpOnly cookie and an explicit CSRF response header.
    /// Full storage, entropy failure, or a poisoned lock returns an empty,
    /// non-cacheable 503 without credentials or evicting a live challenge.
    /// Only full storage includes a retry delay, rounded up to the earliest
    /// pending expiry and bounded to 1–300 seconds. Consumption may free it sooner.
    pub fn issue(&self) -> Response {
        self.issue_using(Instant::now, getrandom::fill)
    }

    fn issue_using(
        &self,
        now: impl FnOnce() -> Instant,
        mut random: impl FnMut(&mut [u8]) -> Result<(), getrandom::Error>,
    ) -> Response {
        let Ok(mut pending) = self.pending.lock() else {
            return empty(StatusCode::SERVICE_UNAVAILABLE);
        };
        // Capture the deadline after acquiring the shared lock.
        let now = now();
        pending.retain(|challenge| now < challenge.expires_at);
        if pending.len() == MAX_PENDING {
            // Consumption uses swap_remove, so the earliest expiry need not be first.
            let earliest = pending
                .iter()
                .map(|challenge| challenge.expires_at)
                .min()
                .expect("full challenge storage is nonempty");
            let remaining = earliest.saturating_duration_since(now);
            let seconds = remaining
                .as_secs()
                .saturating_add(u64::from(remaining.subsec_nanos() != 0))
                .clamp(1, LIFETIME.as_secs());
            let mut response = empty(StatusCode::SERVICE_UNAVAILABLE);
            response.headers_mut().insert(RETRY_AFTER, seconds.into());
            return response;
        }
        let Some(expires_at) = now.checked_add(LIFETIME) else {
            return empty(StatusCode::SERVICE_UNAVAILABLE);
        };
        let mut cookie = [0; TOKEN_BYTES];
        let mut csrf = [0; CSRF_TOKEN_BYTES];
        if random(&mut cookie).is_err() || random(&mut csrf).is_err() {
            return empty(StatusCode::SERVICE_UNAVAILABLE);
        }
        pending.push(Challenge {
            cookie,
            csrf,
            expires_at,
        });
        let mut response = empty(StatusCode::NO_CONTENT);
        response.headers_mut().insert(
            SET_COOKIE,
            sensitive(format!(
                "{COOKIE_NAME}={}; {COOKIE_ATTRIBUTES}; Max-Age={}",
                encode(&cookie),
                LIFETIME.as_secs()
            )),
        );
        response
            .headers_mut()
            .insert(CSRF_HEADER, sensitive(encode(&csrf)));
        response
    }

    /// Atomically consume exactly one matching, live cookie/header pair.
    /// Malformed or mismatched credentials cannot consume another challenge.
    /// Reject before password processing and require a new challenge after every
    /// admitted submission, including an invalid password or form.
    pub fn consume(&self, headers: &HeaderMap) -> bool {
        self.consume_using(headers, Instant::now)
    }

    fn consume_using(&self, headers: &HeaderMap, now: impl FnOnce() -> Instant) -> bool {
        let Some((cookie, csrf)) = named_credentials(headers, COOKIE_NAME) else {
            return false;
        };
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        let now = now();
        pending.retain(|challenge| now < challenge.expires_at);
        let Some(index) = pending.iter().position(|challenge| {
            bool::from(challenge.cookie.ct_eq(&cookie) & challenge.csrf.ct_eq(&csrf))
        }) else {
            return false;
        };
        pending.swap_remove(index);
        true
    }
}

#[cfg(test)]
mod tests;
