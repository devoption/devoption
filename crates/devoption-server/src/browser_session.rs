//! Cookie transport for the browser session behind a validated origin.
//!
//! HTTP callers must first validate Host. Browser mutations also require matching
//! Origin and an explicitly submitted CSRF header with the HttpOnly session cookie. This
//! does not protect pre-authentication flows or authorize first-owner enrollment.
//!
//! Secure cookies require browser support for secure loopback contexts; never
//! remove Secure to accommodate a browser. Explicit public mode terminates TLS at
//! the configured local proxy; this adapter does not infer proxy trust. Cookies are not
//! isolated by port, so other services on the same host remain in the trust boundary.
//! Existing-owner HTTP sign-in and sign-out use this adapter.

use axum::{
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{AUTHORIZATION, CACHE_CONTROL, COOKIE, SET_COOKIE},
    },
    response::{IntoResponse, Response},
};

use crate::{
    identity::IdentityId,
    owner_auth::service::IssuedSession,
    session::{
        CSRF_TOKEN_BYTES, TOKEN_BYTES,
        slot::{ActivationError, SessionSlot},
    },
};

pub const CSRF_HEADER: &str = "x-csrf-token";
const COOKIE_NAME: &str = "__Host-devoption_session";
pub(crate) const COOKIE_ATTRIBUTES: &str = "Path=/; Secure; HttpOnly; SameSite=Strict";
const MAX_COOKIE_BYTES: usize = 4096;

/// Activate first, then release the session cookie and separate CSRF response header.
/// The response has no body or persistent cookie expiry; server expiry is authoritative.
/// The caller must authorize/protect sign-in before calling this helper.
pub fn activate(grant: IssuedSession, slot: &SessionSlot) -> Result<Response, ActivationError> {
    let credentials = grant.activate_browser(slot)?;
    let mut response = empty(StatusCode::NO_CONTENT);
    response.headers_mut().insert(
        SET_COOKIE,
        sensitive(format!(
            "{COOKIE_NAME}={}; {COOKIE_ATTRIBUTES}",
            encode(credentials.session_token())
        )),
    );
    response
        .headers_mut()
        .insert(CSRF_HEADER, sensitive(encode(credentials.csrf_token())));
    Ok(response)
}

/// Require the cookie and explicit CSRF header from the same live session.
/// Never falls back to Authorization, URL, body, proxy, or additional cookie values.
pub fn authenticate(headers: &HeaderMap, slot: &SessionSlot) -> Option<IdentityId> {
    let (session, csrf) = credentials(headers)?;
    slot.authenticate_browser(Some(&session), Some(&csrf))
}

/// Authenticate a GET/HEAD page read using only the strict session cookie.
/// The caller must validate Host and restrict this to reads; mutations still
/// require `authenticate` with explicit CSRF. No bearer fallback or renewal.
pub(crate) fn authenticate_read(headers: &HeaderMap, slot: &SessionSlot) -> Option<IdentityId> {
    let token = named_session(headers, COOKIE_NAME)?;
    slot.authenticate(Some(&token))
}

/// Recover the current session's CSRF token using only its strict session cookie.
/// The caller MUST first require a matching Host/Origin (including on this read).
/// This neither changes the cookie nor creates, rotates, or extends a session.
pub fn recover(headers: &HeaderMap, slot: &SessionSlot) -> Response {
    let Some(csrf) =
        named_session(headers, COOKIE_NAME).and_then(|token| slot.recover_browser_csrf(&token))
    else {
        return empty(StatusCode::UNAUTHORIZED);
    };
    let mut response = empty(StatusCode::NO_CONTENT);
    response
        .headers_mut()
        .insert(CSRF_HEADER, sensitive(encode(&csrf)));
    response
}

/// Clear the cookie only after atomically logging out the current matching pair.
/// A failed/stale logout must not clear a newer browser session's cookie.
/// The caller must enforce Host/Origin before invoking this mutation.
pub fn logout(headers: &HeaderMap, slot: &SessionSlot) -> Response {
    let accepted = credentials(headers)
        .is_some_and(|(session, csrf)| slot.logout_browser(Some(&session), Some(&csrf)));
    if !accepted {
        return empty(StatusCode::UNAUTHORIZED);
    }
    let mut response = empty(StatusCode::NO_CONTENT);
    response.headers_mut().insert(
        SET_COOKIE,
        sensitive(format!("{COOKIE_NAME}=; {COOKIE_ATTRIBUTES}; Max-Age=0")),
    );
    response
}

pub(crate) fn empty(status: StatusCode) -> Response {
    (status, [(CACHE_CONTROL, "no-store")]).into_response()
}

pub(crate) fn sensitive(value: String) -> HeaderValue {
    // Every caller supplies fixed ASCII attributes and/or hex-encoded bytes.
    let mut value = HeaderValue::from_str(&value).expect("fixed cookie/hex header is valid");
    value.set_sensitive(true);
    value
}

fn credentials(headers: &HeaderMap) -> Option<([u8; TOKEN_BYTES], [u8; CSRF_TOKEN_BYTES])> {
    named_credentials(headers, COOKIE_NAME)
}

pub(crate) fn named_credentials(
    headers: &HeaderMap,
    cookie_name: &str,
) -> Option<([u8; TOKEN_BYTES], [u8; CSRF_TOKEN_BYTES])> {
    Some((
        named_session(headers, cookie_name)?,
        decode(one_header(headers, CSRF_HEADER)?)?,
    ))
}

fn named_session(headers: &HeaderMap, cookie_name: &str) -> Option<[u8; TOKEN_BYTES]> {
    if headers.contains_key(AUTHORIZATION) {
        return None;
    }
    let cookies = one_header(headers, COOKIE.as_str())?;
    if cookies.len() > MAX_COOKIE_BYTES {
        return None;
    }
    let mut session = None;
    for pair in cookies.split(';') {
        let (name, value) = pair.trim_matches([' ', '\t']).split_once('=')?;
        if name.is_empty() || !name.bytes().all(cookie_name_byte) || !cookie_value(value) {
            return None;
        }
        if name == cookie_name {
            if session.is_some() {
                return None;
            }
            session = Some(decode(value)?);
        }
    }
    session
}

fn one_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    value.to_str().ok()
}

fn cookie_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn cookie_value(value: &str) -> bool {
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    value
        .bytes()
        .all(|byte| matches!(byte, 0x21 | 0x23..=0x2b | 0x2d..=0x3a | 0x3c..=0x5b | 0x5d..=0x7e))
}

fn decode<const N: usize>(value: &str) -> Option<[u8; N]> {
    if value.len() != N * 2 {
        return None;
    }
    let mut output = [0; N];
    for (byte, pair) in output.iter_mut().zip(value.as_bytes().as_chunks::<2>().0) {
        *byte = (hex(pair[0])? << 4) | hex(pair[1])?;
    }
    Some(output)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests;
