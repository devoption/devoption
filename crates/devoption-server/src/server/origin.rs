//! Operator-selected HTTPS authority. No DNS lookups or proxy-header trust.
use axum::extract::Request;
use axum::http::header::{HOST, ORIGIN};

/// Canonical HTTPS origin for the local TLS-terminating proxy deployment.
/// Only lowercase ASCII DNS names on the default HTTPS port are supported.
#[derive(Clone)]
pub struct PublicOrigin(String);

impl PublicOrigin {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        let host = value
            .strip_prefix("https://")
            .ok_or("public origin must be https:// followed by a lowercase DNS name")?;
        if host.len() > 253
            || !host.contains('.')
            || host.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
            || !host
                .rsplit('.')
                .next()
                .unwrap_or_default()
                .bytes()
                .any(|b| b.is_ascii_lowercase())
        {
            return Err(
                "public origin requires a lowercase DNS name without port, path, query or fragment",
            );
        }
        Ok(Self(value.to_owned()))
    }

    fn authority_matches(&self, authority: &str) -> bool {
        authority
            .strip_suffix(":443")
            .unwrap_or(authority)
            .eq_ignore_ascii_case(&self.0[8..])
    }

    fn origin_matches(&self, value: &str) -> bool {
        value
            .strip_prefix("https://")
            .is_some_and(|host| self.authority_matches(host))
    }

    fn host_valid(&self, request: &Request) -> bool {
        // The configured proxy removes forwarding metadata, including its own.
        // Never infer public authority or scheme from caller-supplied headers.
        if request.headers().keys().any(|key| {
            key == "forwarded" || key == "x-real-ip" || key.as_str().starts_with("x-forwarded-")
        }) {
            return false;
        }
        let mut hosts = request.headers().get_all(HOST).iter();
        if !hosts
            .next()
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| self.authority_matches(v))
            || hosts.next().is_some()
        {
            return false;
        }
        let uri = request.uri();
        match (uri.scheme_str(), uri.authority()) {
            (Some("https"), Some(authority)) => self.authority_matches(authority.as_str()),
            (None, None) => uri.path().starts_with('/') && !uri.path().starts_with("//"),
            _ => false,
        }
    }
}

#[derive(Clone)]
pub(super) enum RequestOrigin {
    Local(u16),
    Public(PublicOrigin),
}

impl From<u16> for RequestOrigin {
    fn from(port: u16) -> Self {
        Self::Local(port)
    }
}

impl RequestOrigin {
    pub(super) fn check(&self, request: &Request) -> Result<(), axum::http::StatusCode> {
        use axum::http::StatusCode;
        match self {
            Self::Local(port) => {
                let host = super::validated_host(request, *port).ok_or(StatusCode::BAD_REQUEST)?;
                if !super::safe_method(request.method())
                    && !super::same_origin(request, host, *port)
                {
                    return Err(StatusCode::FORBIDDEN);
                }
            }
            Self::Public(origin) => {
                if !origin.host_valid(request) {
                    return Err(StatusCode::BAD_REQUEST);
                }
                if !super::safe_method(request.method()) {
                    let mut values = request.headers().get_all(ORIGIN).iter();
                    if !values
                        .next()
                        .and_then(|v| v.to_str().ok())
                        .is_some_and(|v| origin.origin_matches(v))
                        || values.next().is_some()
                    {
                        return Err(StatusCode::FORBIDDEN);
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::StatusCode};

    #[test]
    fn configuration_is_a_bounded_canonical_dns_origin() {
        for invalid in [
            "",
            "http://platform.example.test",
            "https://localhost",
            "https://127.0.0.1",
            "https://PLATFORM.example.test",
            "https://platform.example.test/",
            "https://platform.example.test:443",
            "https://user@platform.example.test",
            "https://platform.example.test?x",
            "https://platform.example.test#x",
            "https://platform.example.test.",
            "https://a..test",
            "https://-a.test",
            "https://a-.test",
            "https://é.test",
            "https://a_test.example",
            "https://[::1]",
            "https://a.test\n",
        ] {
            assert!(PublicOrigin::parse(invalid).is_err(), "{invalid}");
        }
        assert!(PublicOrigin::parse(&format!("https://{}.test", "a".repeat(64))).is_err());
        assert!(PublicOrigin::parse(&format!("https://{}test", "a.".repeat(127))).is_err());
        assert!(PublicOrigin::parse("https://platform.example.test").is_ok());
        assert!(PublicOrigin::parse("https://xn--bcher-kva.example").is_ok());
    }

    #[test]
    fn public_authority_and_origin_must_match_without_forwarding_metadata() {
        let policy =
            RequestOrigin::Public(PublicOrigin::parse("https://platform.example.test").unwrap());
        for (target, host, origin, expected) in [
            (
                "/",
                "platform.example.test",
                "https://platform.example.test",
                Ok(()),
            ),
            (
                "https://platform.example.test/",
                "PLATFORM.example.test:443",
                "https://platform.example.test:443",
                Ok(()),
            ),
            (
                "http://platform.example.test/",
                "platform.example.test",
                "https://platform.example.test",
                Err(StatusCode::BAD_REQUEST),
            ),
            (
                "https://other.example.test/",
                "platform.example.test",
                "https://platform.example.test",
                Err(StatusCode::BAD_REQUEST),
            ),
            (
                "//other.example.test/",
                "platform.example.test",
                "https://platform.example.test",
                Err(StatusCode::BAD_REQUEST),
            ),
            (
                "/",
                "platform.example.test:3000",
                "https://platform.example.test",
                Err(StatusCode::BAD_REQUEST),
            ),
            (
                "/",
                "127.0.0.1:3000",
                "https://platform.example.test",
                Err(StatusCode::BAD_REQUEST),
            ),
            (
                "/",
                "platform.example.test",
                "http://platform.example.test",
                Err(StatusCode::FORBIDDEN),
            ),
            (
                "/",
                "platform.example.test",
                "https://platform.example.test/",
                Err(StatusCode::FORBIDDEN),
            ),
            (
                "/",
                "platform.example.test",
                "null",
                Err(StatusCode::FORBIDDEN),
            ),
            (
                "/",
                "platform.example.test",
                "https://app.example.test",
                Err(StatusCode::FORBIDDEN),
            ),
        ] {
            let request = Request::builder()
                .method("POST")
                .uri(target)
                .header(HOST, host)
                .header(ORIGIN, origin)
                .body(Body::empty())
                .unwrap();
            assert_eq!(policy.check(&request), expected, "{target} {host} {origin}");
        }
        for name in [
            "forwarded",
            "x-forwarded-host",
            "x-forwarded-proto",
            "x-forwarded-for",
            "x-forwarded-port",
            "x-real-ip",
            "host",
            "origin",
        ] {
            let request = Request::builder()
                .method("POST")
                .uri("/")
                .header(HOST, "platform.example.test")
                .header(ORIGIN, "https://platform.example.test")
                .header(name, "untrusted")
                .body(Body::empty())
                .unwrap();
            assert!(policy.check(&request).is_err(), "{name}");
        }
        let request = Request::builder()
            .method("POST")
            .uri("/")
            .header(HOST, "platform.example.test")
            .body(Body::empty())
            .unwrap();
        assert_eq!(policy.check(&request), Err(StatusCode::FORBIDDEN));
    }
}
