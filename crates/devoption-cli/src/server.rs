use std::{
    io,
    net::Ipv4Addr,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    extract::Request,
    http::{
        HeaderMap, Method, StatusCode,
        header::{AUTHORIZATION, CACHE_CONTROL, HOST, ORIGIN, WWW_AUTHENTICATE},
    },
    middleware::{self, Next},
    response::{Html, IntoResponse},
    routing::get,
};
use devoption_cli::{
    identity::IdentityId,
    session::{Session, TOKEN_BYTES},
};
use devoption_ui::Page;
use tokio::{net::TcpListener, runtime::Builder};

type SharedSession = Arc<Mutex<Option<Session>>>;

#[cfg(test)]
mod session_tests;

pub fn run(port: u16) -> io::Result<()> {
    Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
            let address = listener.local_addr()?;
            println!("Listening on http://{address} (Ctrl+C to stop)");
            let app = Router::new()
                .route(
                    "/",
                    get(|| async { Html(devoption_ui::render(Page::Home)) }),
                )
                .route(
                    "/projects",
                    get(|| async { Html(devoption_ui::render(Page::Projects)) }),
                )
                .route(
                    "/about",
                    get(|| async { Html(devoption_ui::render(Page::About)) }),
                )
                .route(
                    "/settings/appearance",
                    get(|| async { Html(devoption_ui::render(Page::Appearance)) }),
                );
            // No production issuance or bootstrap yet: unsafe requests fail closed.
            let app = protect(app, address.port(), Arc::new(Mutex::new(None)));
            axum::serve(listener, app).await
        })
}

fn protect(app: Router, port: u16, session: SharedSession) -> Router {
    app.layer(middleware::from_fn(
        move |mut request: Request, next: Next| {
            let session = Arc::clone(&session);
            async move {
                let Some(host) = validated_host(&request, port) else {
                    return StatusCode::BAD_REQUEST.into_response();
                };
                // RFC 9110 safe methods; extensions require Origin and a session.
                // Public preview reads remain unauthenticated and must never mutate.
                if !matches!(
                    *request.method(),
                    Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
                ) {
                    if !same_origin(&request, host, port) {
                        return StatusCode::FORBIDDEN.into_response();
                    }
                    let Some(identity) = authenticated_identity(request.headers(), &session) else {
                        return (
                            StatusCode::UNAUTHORIZED,
                            [(WWW_AUTHENTICATE, "Bearer"), (CACHE_CONTROL, "no-store")],
                        )
                            .into_response();
                    };
                    // Downstream permission checks must use this server-owned ID,
                    // never a username/role asserted in headers, the URL, or a body.
                    request.extensions_mut().insert(identity);
                }
                next.run(request).await
            }
        },
    ))
}

fn authenticated_identity(headers: &HeaderMap, session: &SharedSession) -> Option<IdentityId> {
    let candidate = bearer_token(headers);
    let Ok(mut session) = session.lock() else {
        // A poisoned validator cannot establish authorization. Never hold the
        // synchronous lock across an await or into the downstream handler.
        return None;
    };
    session
        .as_mut()
        .and_then(|session| session.authenticate(candidate.as_ref().map(|token| token.as_slice())))
}

fn bearer_token(headers: &HeaderMap) -> Option<[u8; TOKEN_BYTES]> {
    let mut values = headers.get_all(AUTHORIZATION).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    let (scheme, value) = value.split_once(' ')?;
    // HTTP auth schemes are case-insensitive and permit one or more SP bytes.
    let value = value.trim_start_matches(' ');
    if !scheme.eq_ignore_ascii_case("Bearer") || value.len() != TOKEN_BYTES * 2 {
        return None;
    }
    let mut token = [0; TOKEN_BYTES];
    for (output, pair) in token.iter_mut().zip(value.as_bytes().as_chunks::<2>().0) {
        *output = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Some(token)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn validated_host(request: &Request, port: u16) -> Option<&'static str> {
    // Require one explicit Host even for HTTP/1.0; never infer it from proxy headers.
    let mut hosts = request.headers().get_all(HOST).iter();
    let host = hosts
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| local_host(value, port))?;
    if hosts.next().is_some() {
        return None;
    }

    // RFC 9112 §3.2: absolute-form carries its own authority. Validate both,
    // rejecting conflicting authorities rather than letting Host mask the target.
    let uri = request.uri();
    let valid = match (uri.scheme_str(), uri.authority()) {
        (Some(scheme), Some(authority)) => {
            scheme.eq_ignore_ascii_case("http")
                && local_host(authority.as_str(), port) == Some(host)
        }
        (None, None) => uri.path().starts_with('/') && !uri.path().starts_with("//"),
        _ => false,
    };
    valid.then_some(host)
}

fn same_origin(request: &Request, host: &str, port: u16) -> bool {
    let mut origins = request.headers().get_all(ORIGIN).iter();
    let Some(origin) = origins.next().and_then(|value| value.to_str().ok()) else {
        return false;
    };
    if origins.next().is_some() {
        return false;
    }
    // An Origin has only scheme and authority, never URL paths or userinfo.
    // Do not infer a missing Origin from Referer, proxy headers, or Fetch Metadata.
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    scheme.eq_ignore_ascii_case("http") && local_host(authority, port) == Some(host)
}

fn local_host(authority: &str, bound_port: u16) -> Option<&'static str> {
    // This allowlist needs no DNS lookup or general URL normalization. In
    // particular, userinfo, encoded names, IPv6, suffixes, and lists cannot match.
    let host = match authority.split_once(':') {
        Some((host, port)) => {
            if port.is_empty()
                || !port.bytes().all(|byte| byte.is_ascii_digit())
                || port.parse::<u16>().ok() != Some(bound_port)
            {
                return None;
            }
            host
        }
        None if bound_port == 80 => authority,
        None => return None,
    };
    if host.eq_ignore_ascii_case("localhost") {
        Some("localhost")
    } else if host == "127.0.0.1" {
        Some("127.0.0.1")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    #[test]
    fn omitted_ports_mean_http_port_80_not_the_listener_default() {
        // Exercise port 80 without requiring a privileged listener in CI.
        for (target, host, port, allowed) in [
            ("/", "localhost", 80, true),
            ("/", "127.0.0.1", 80, true),
            ("http://LOCALHOST/", "localhost:80", 80, true),
            ("http://127.0.0.1:80/", "127.0.0.1", 80, true),
            ("/", "localhost", 3000, false),
            ("http://localhost/", "localhost:3000", 3000, false),
            ("/", "localhost:", 80, false),
            ("http://localhost:/", "localhost:80", 80, false),
        ] {
            let request = Request::builder()
                .uri(target)
                .header(HOST, host)
                .body(Body::empty())
                .unwrap();
            assert_eq!(
                validated_host(&request, port).is_some(),
                allowed,
                "{target} {host}"
            );
        }
    }

    #[test]
    fn origin_default_port_and_host_aliases_are_compared_as_origins() {
        for (origin, host, port, allowed) in [
            ("http://localhost", "localhost:80", 80, true),
            ("HTTP://LOCALHOST:080", "localhost", 80, true),
            ("http://127.0.0.1:80", "127.0.0.1", 80, true),
            ("http://localhost", "localhost:3000", 3000, false),
            ("http://localhost:", "localhost:80", 80, false),
            ("http://localhost/", "localhost:80", 80, false),
            ("https://localhost:80", "localhost:80", 80, false),
            ("http://127.0.0.1", "localhost:80", 80, false),
        ] {
            let request = Request::builder()
                .method(Method::POST)
                .uri("/")
                .header(HOST, host)
                .header(ORIGIN, origin)
                .body(Body::empty())
                .unwrap();
            let host = validated_host(&request, port).unwrap();
            assert_eq!(same_origin(&request, host, port), allowed, "{origin}");
        }
    }
}
