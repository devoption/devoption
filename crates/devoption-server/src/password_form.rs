//! Strict input boundary for future browser authentication routes.
//!
//! Accepts one URL-encoded `password` field on POST, at most 4 KiB encoded and
//! 1–1024 UTF-8 bytes decoded. A five-second deadline covers the complete body read.
//! Invalid escapes/UTF-8 are rejected rather than repaired; password bytes are not
//! normalized or truncated. This is input validation, not enrollment strength policy.
//!
//! Only fixture routers use this extractor. Real routes must still enforce
//! Host/Origin, enrollment, CSRF, authentication, and shared admission controls.
//! This bounds one body, not total server connections or memory. Requires a Tokio
//! runtime with its time driver enabled. Passwords must not enter responses or logs.

use std::{error::Error, fmt, time::Duration};

use axum::{
    body::to_bytes,
    extract::{FromRequest, Request},
    http::{
        HeaderMap, HeaderName, HeaderValue, Method, StatusCode,
        header::{ALLOW, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};
use http_body_util::LengthLimitError;

use crate::password;

const MAX_FORM_BYTES: usize = 4096;
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// A bounded password, intentionally neither Debug nor Clone.
pub struct PasswordForm(String);

impl PasswordForm {
    pub fn into_password(self) -> String {
        self.0
    }
}

/// Fixed rejection categories contain no input or underlying I/O error details.
#[derive(Debug, PartialEq, Eq)]
pub enum FormError {
    BadRequest,
    TooLarge,
    UnsupportedMediaType,
    Timeout,
    MethodNotAllowed,
}

impl fmt::Display for FormError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BadRequest => "invalid password form",
            Self::TooLarge => "password form body exceeds the limit",
            Self::UnsupportedMediaType => "unsupported password form encoding",
            Self::Timeout => "password form body read timed out",
            Self::MethodNotAllowed => "password forms require POST",
        })
    }
}

impl Error for FormError {}

impl IntoResponse for FormError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::Timeout => StatusCode::REQUEST_TIMEOUT,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
        };
        let mut response = (status, [(CACHE_CONTROL, "no-store")]).into_response();
        if self == Self::MethodNotAllowed {
            response
                .headers_mut()
                .insert(ALLOW, HeaderValue::from_static("POST"));
        }
        response
    }
}

impl<S: Send + Sync> FromRequest<S> for PasswordForm {
    type Rejection = FormError;

    async fn from_request(request: Request, _state: &S) -> Result<Self, FormError> {
        if request.method() != Method::POST {
            return Err(FormError::MethodNotAllowed);
        }
        let declared_length = validate_headers(request.headers())?;
        let body =
            tokio::time::timeout(READ_TIMEOUT, to_bytes(request.into_body(), MAX_FORM_BYTES))
                .await
                .map_err(|_| FormError::Timeout)?
                .map_err(|error| {
                    if error
                        .source()
                        .is_some_and(|source| source.is::<LengthLimitError>())
                    {
                        FormError::TooLarge
                    } else {
                        FormError::BadRequest
                    }
                })?;
        if declared_length.is_some_and(|length| length != body.len()) {
            return Err(FormError::BadRequest);
        }
        parse(&body).map(Self)
    }
}

fn one_header(headers: &HeaderMap, name: HeaderName) -> Result<Option<&str>, FormError> {
    let mut values = headers.get_all(name).iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(FormError::BadRequest);
    }
    value
        .map(|value| value.to_str().map_err(|_| FormError::BadRequest))
        .transpose()
}

fn validate_headers(headers: &HeaderMap) -> Result<Option<usize>, FormError> {
    let media = one_header(headers, CONTENT_TYPE)?.ok_or(FormError::UnsupportedMediaType)?;
    let mut parts = media.split(';');
    if !parts
        .next()
        .unwrap_or_default()
        .trim()
        .eq_ignore_ascii_case("application/x-www-form-urlencoded")
    {
        return Err(FormError::UnsupportedMediaType);
    }
    if let Some(parameter) = parts.next() {
        let (name, value) = parameter
            .split_once('=')
            .ok_or(FormError::UnsupportedMediaType)?;
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(value);
        if !name.trim().eq_ignore_ascii_case("charset")
            || !value.eq_ignore_ascii_case("utf-8")
            || parts.next().is_some()
        {
            return Err(FormError::UnsupportedMediaType);
        }
    }
    if one_header(headers, CONTENT_ENCODING)?
        .is_some_and(|value| !value.trim().eq_ignore_ascii_case("identity"))
    {
        return Err(FormError::UnsupportedMediaType);
    }
    if let Some(length) = one_header(headers, CONTENT_LENGTH)? {
        let length = length.trim();
        if length.is_empty() || !length.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(FormError::BadRequest);
        }
        let length: u64 = length.parse().map_err(|_| FormError::BadRequest)?;
        if length > MAX_FORM_BYTES as u64 {
            return Err(FormError::TooLarge);
        }
        return Ok(Some(length as usize));
    }
    Ok(None)
}

fn parse(body: &[u8]) -> Result<String, FormError> {
    let mut fields = body.split(|byte| *byte == b'&');
    let field = fields.next().ok_or(FormError::BadRequest)?;
    if fields.next().is_some() {
        return Err(FormError::BadRequest);
    }
    let separator = field
        .iter()
        .position(|byte| *byte == b'=')
        .ok_or(FormError::BadRequest)?;
    if decode(&field[..separator])? != "password" {
        return Err(FormError::BadRequest);
    }
    let password = decode(&field[separator + 1..])?;
    if !password::valid_length(&password) {
        return Err(FormError::BadRequest);
    }
    Ok(password)
}

fn decode(encoded: &[u8]) -> Result<String, FormError> {
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut bytes = encoded.iter().copied();
    while let Some(byte) = bytes.next() {
        decoded.push(match byte {
            b'+' => b' ',
            b'%' => {
                let high = bytes.next().and_then(hex).ok_or(FormError::BadRequest)?;
                let low = bytes.next().and_then(hex).ok_or(FormError::BadRequest)?;
                (high << 4) | low
            }
            value => value,
        });
    }
    String::from_utf8(decoded).map_err(|_| FormError::BadRequest)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
