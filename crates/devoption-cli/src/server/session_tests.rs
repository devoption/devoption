use super::*;
use axum::{Extension, body::Body, http::HeaderValue, routing::post};
use devoption_cli::session::MAX_LIFETIME;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tower::ServiceExt;

// Predictable fixtures are installed only in this test router, never the server.
const FIXTURE: [u8; TOKEN_BYTES] = [0xab; TOKEN_BYTES];
const IDENTITY: IdentityId = IdentityId::from_bytes([1; 16]);

fn credential() -> String {
    format!("Bearer {}", "ab".repeat(TOKEN_BYTES))
}

fn fixture() -> (Router, SharedSession, Arc<AtomicUsize>) {
    fixture_for(IDENTITY)
}

fn fixture_for(identity: IdentityId) -> (Router, SharedSession, Arc<AtomicUsize>) {
    let session = Arc::new(Mutex::new(Some(
        Session::new(identity, FIXTURE, MAX_LIFETIME).unwrap(),
    )));
    let calls = Arc::new(AtomicUsize::new(0));
    let handler_calls = Arc::clone(&calls);
    let app = Router::new().route(
        "/probe",
        post(
            move |Extension(authenticated): Extension<IdentityId>| async move {
                assert_eq!(authenticated, identity);
                handler_calls.fetch_add(1, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            },
        ),
    );
    (protect(app, 3000, Arc::clone(&session)), session, calls)
}

fn request() -> Request {
    Request::builder()
        .method(Method::POST)
        .uri("/probe")
        .header(HOST, "127.0.0.1:3000")
        .header(ORIGIN, "http://127.0.0.1:3000")
        .body(Body::empty())
        .unwrap()
}

fn response(app: &Router, request: Request) -> (StatusCode, HeaderMap, Vec<u8>) {
    Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let response = app.clone().oneshot(request).await.unwrap();
            let (parts, body) = response.into_parts();
            let body = axum::body::to_bytes(body, 1024).await.unwrap();
            (parts.status, parts.headers, body.to_vec())
        })
}

fn assert_unauthorized(app: &Router, request: Request) {
    let (status, headers, body) = response(app, request);
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(headers[WWW_AUTHENTICATE], "Bearer");
    assert_eq!(headers[CACHE_CONTROL], "no-store");
    assert!(body.is_empty());
    assert!(!headers.contains_key("set-cookie"));
    assert!(!headers.contains_key("access-control-allow-origin"));
}

#[test]
fn only_a_single_valid_bearer_header_reaches_the_handler() {
    let (app, _, calls) = fixture();
    assert_unauthorized(&app, request());
    let valid = credential();
    for values in [
        vec![String::new()],
        vec!["Bearer".into()],
        vec!["Bearer ".into()],
        vec![format!("Basic {}", "ab".repeat(TOKEN_BYTES))],
        vec![format!("Bearer\t{}", "ab".repeat(TOKEN_BYTES))],
        vec![format!("Bearer {}", "a".repeat(63))],
        vec![format!("Bearer {}", "a".repeat(65))],
        vec![format!("Bearer {}", "g".repeat(64))],
        vec![format!("Bearer {}", "é".repeat(32))],
        vec![format!("Bearer {}", "00".repeat(TOKEN_BYTES))],
        vec![format!("{valid} ")],
        vec![format!("{valid}, {valid}")],
        vec![valid.clone(), valid.clone()],
        vec![valid.clone(), "Basic x".into()],
        vec!["Basic x".into(), valid.clone()],
    ] {
        let mut request = request();
        for value in values {
            request
                .headers_mut()
                .append(AUTHORIZATION, value.parse().unwrap());
        }
        assert_unauthorized(&app, request);
    }
    let mut non_ascii = request();
    non_ascii.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_bytes(b"Bearer \xff").unwrap(),
    );
    assert_unauthorized(&app, non_ascii);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    for value in [valid, format!("bEaReR   {}", "AB".repeat(TOKEN_BYTES))] {
        let mut request = request();
        request
            .headers_mut()
            .insert(AUTHORIZATION, value.parse().unwrap());
        assert_eq!(response(&app, request).0, StatusCode::NO_CONTENT);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn route_misses_and_unsupported_methods_require_authorization_first() {
    let (app, _, calls) = fixture();
    for (method, path, expected) in [
        ("POST", "/missing", StatusCode::NOT_FOUND),
        ("DELETE", "/probe", StatusCode::METHOD_NOT_ALLOWED),
        ("CUSTOM", "/probe", StatusCode::METHOD_NOT_ALLOWED),
    ] {
        let build = || {
            let mut request = request();
            *request.method_mut() = method.parse().unwrap();
            *request.uri_mut() = path.parse().unwrap();
            request
        };
        assert_unauthorized(&app, build());
        let mut request = build();
        request
            .headers_mut()
            .insert(AUTHORIZATION, credential().parse().unwrap());
        assert_eq!(response(&app, request).0, expected);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn credentials_in_urls_cookies_bodies_and_proxy_headers_are_ignored() {
    let (app, _, calls) = fixture();
    let token = "ab".repeat(TOKEN_BYTES);
    let mut request = request();
    *request.uri_mut() = format!("/probe?access_token={token}").parse().unwrap();
    for (name, value) in [
        ("cookie", format!("session={token}; access_token={token}")),
        ("x-forwarded-authorization", credential()),
        ("proxy-authorization", credential()),
        ("content-type", "application/x-www-form-urlencoded".into()),
    ] {
        request.headers_mut().insert(name, value.parse().unwrap());
    }
    *request.body_mut() = Body::from(format!("access_token={token}"));
    assert_unauthorized(&app, request);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn valid_credentials_do_not_bypass_host_or_origin_validation() {
    let (app, _, calls) = fixture();
    for (host, origin, expected) in [
        (
            "attacker.invalid",
            "http://127.0.0.1:3000",
            StatusCode::BAD_REQUEST,
        ),
        (
            "127.0.0.1:3000",
            "http://attacker.invalid",
            StatusCode::FORBIDDEN,
        ),
        (
            "127.0.0.1:3000",
            "http://localhost:3000",
            StatusCode::FORBIDDEN,
        ),
        ("127.0.0.1:3000", "null", StatusCode::FORBIDDEN),
    ] {
        let mut request = request();
        request.headers_mut().insert(HOST, host.parse().unwrap());
        request
            .headers_mut()
            .insert(ORIGIN, origin.parse().unwrap());
        request
            .headers_mut()
            .insert(AUTHORIZATION, credential().parse().unwrap());
        let (status, _, body) = response(&app, request);
        assert_eq!(status, expected);
        assert!(body.is_empty());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn each_request_observes_revocation_expiry_and_absent_session_state() {
    let (app, session, calls) = fixture();
    let authenticated = || {
        let mut request = request();
        request
            .headers_mut()
            .insert(AUTHORIZATION, credential().parse().unwrap());
        request
    };
    assert_eq!(response(&app, authenticated()).0, StatusCode::NO_CONTENT);
    session.lock().unwrap().as_mut().unwrap().revoke();
    assert_unauthorized(&app, authenticated());

    *session.lock().unwrap() =
        Some(Session::new(IDENTITY, FIXTURE, Duration::from_nanos(1)).unwrap());
    // The core's exact deadline tests use a controlled clock. Here a short wait
    // exercises real-clock expiration through the HTTP middleware as well.
    std::thread::sleep(Duration::from_millis(1));
    assert_unauthorized(&app, authenticated());
    *session.lock().unwrap() = None;
    assert_unauthorized(&app, authenticated());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn poisoned_session_state_fails_closed() {
    let (app, session, calls) = fixture();
    assert!(
        std::thread::spawn(move || {
            let _guard = session.lock().unwrap();
            panic!("simulate a failed state update");
        })
        .join()
        .is_err()
    );
    let mut request = request();
    request
        .headers_mut()
        .insert(AUTHORIZATION, credential().parse().unwrap());
    assert_unauthorized(&app, request);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn protected_handlers_receive_only_the_session_bound_identity() {
    for identity in [IDENTITY, IdentityId::from_bytes([2; 16])] {
        let (app, _, calls) = fixture_for(identity);
        let mut authenticated = request();
        *authenticated.uri_mut() = "/probe?identity=forged&role=owner".parse().unwrap();
        for name in [
            "x-forwarded-user",
            "x-auth-request-user",
            "x-identity-id",
            "x-role",
        ] {
            authenticated
                .headers_mut()
                .insert(name, "forged".parse().unwrap());
        }
        authenticated
            .headers_mut()
            .insert(AUTHORIZATION, credential().parse().unwrap());
        *authenticated.body_mut() = Body::from("identity=forged&role=owner");
        // Even earlier request decoration cannot override the validated session.
        authenticated
            .extensions_mut()
            .insert(IdentityId::from_bytes([0xff; 16]));
        assert_eq!(response(&app, authenticated).0, StatusCode::NO_CONTENT);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let mut forged = request();
        forged
            .headers_mut()
            .insert("x-forwarded-user", "owner".parse().unwrap());
        forged.extensions_mut().insert(identity);
        assert_unauthorized(&app, forged);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
