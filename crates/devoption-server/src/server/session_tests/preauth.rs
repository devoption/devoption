use super::*;
use crate::browser_session;
use browser::{PASSWORD, owner};

fn fixture() -> (tempfile::TempDir, Router) {
    let (directory, auth, identity) = owner();
    let slot = Arc::new(SessionSlot::default());
    let (protected, _) = router_for(identity, Arc::clone(&slot));
    (directory, auth::routes(auth, 3000, slot).merge(protected))
}

fn build(path: &str, issued: Option<&HeaderMap>, password: &str) -> Request {
    let mut request = request();
    *request.uri_mut() = path.parse().unwrap();
    // These fixture passwords contain spaces but no URL-encoded delimiters.
    *request.body_mut() = Body::from(format!("password={password}"));
    request.headers_mut().insert(
        "content-type",
        "application/x-www-form-urlencoded".parse().unwrap(),
    );
    if let Some(issued) = issued {
        request.headers_mut().insert(
            COOKIE,
            issued["set-cookie"]
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .parse()
                .unwrap(),
        );
        request.headers_mut().insert(
            browser_session::CSRF_HEADER,
            issued[browser_session::CSRF_HEADER].clone(),
        );
    }
    request
}

fn send(app: &Router, request: Request) -> (StatusCode, HeaderMap) {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let response = app.clone().oneshot(request).await.unwrap();
            let (parts, body) = response.into_parts();
            assert!(axum::body::to_bytes(body, 1024).await.unwrap().is_empty());
            (parts.status, parts.headers)
        })
}

fn challenge(app: &Router) -> HeaderMap {
    let (status, headers) = send(app, build("/auth/challenge", None, ""));
    assert_eq!(status, StatusCode::NO_CONTENT);
    headers
}

#[test]
fn challenge_capacity_returns_a_bounded_retry_hint_and_recovers_after_consumption() {
    let (_directory, app) = fixture();
    let issued: Vec<_> = (0..16).map(|_| challenge(&app)).collect();
    let (status, headers) = send(&app, build("/auth/challenge", None, ""));
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(headers[CACHE_CONTROL], "no-store");
    assert!(!headers.contains_key("set-cookie"));
    assert!(!headers.contains_key(browser_session::CSRF_HEADER));
    let retry = headers
        .get("retry-after")
        .expect("challenge capacity must provide a retry hint")
        .to_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert!((1..=300).contains(&retry));
    // Capacity rejection must preserve live pairs. Consumption frees one slot.
    assert_eq!(
        send(&app, build("/auth/sign-in", Some(&issued[0]), PASSWORD)).0,
        StatusCode::NO_CONTENT
    );
    assert!(!challenge(&app).contains_key("retry-after"));
}

#[test]
fn host_origin_and_post_guard_both_challenge_and_submission_before_work() {
    let (_directory, app) = fixture();
    let valid = challenge(&app);
    for path in ["/auth/challenge", "/auth/sign-in", "/auth/sign-out"] {
        for (header, value, expected) in [
            (HOST, "attacker.invalid", StatusCode::BAD_REQUEST),
            (ORIGIN, "http://attacker.invalid", StatusCode::FORBIDDEN),
            (ORIGIN, "http://localhost:3000", StatusCode::FORBIDDEN),
            (ORIGIN, "http://127.0.0.1:3001", StatusCode::FORBIDDEN),
            (ORIGIN, "null", StatusCode::FORBIDDEN),
        ] {
            let mut request = build(path, Some(&valid), PASSWORD);
            request.headers_mut().insert(header, value.parse().unwrap());
            let (status, headers) = send(&app, request);
            assert_eq!(status, expected);
            assert_eq!(headers[CACHE_CONTROL], "no-store");
            assert!(!headers.contains_key("set-cookie"));
        }
        for name in [HOST, ORIGIN] {
            let mut request = build(path, Some(&valid), PASSWORD);
            request.headers_mut().remove(&name);
            assert_eq!(
                send(&app, request).0,
                if name == HOST {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::FORBIDDEN
                }
            );
        }
        for method in [Method::GET, Method::HEAD, Method::OPTIONS, Method::PUT] {
            let mut request = build(path, Some(&valid), PASSWORD);
            *request.method_mut() = method;
            let (status, headers) = send(&app, request);
            assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
            assert!(!headers.contains_key("set-cookie"));
        }
    }
    assert_eq!(
        send(&app, build("/auth/sign-in", Some(&valid), PASSWORD)).0,
        StatusCode::NO_CONTENT
    );
}

#[test]
fn sign_in_consumes_before_forms_and_issues_a_fresh_authenticated_session() {
    let (directory, app) = fixture();
    let original = std::fs::read(directory.path().join("owner.bin")).unwrap();
    let first = challenge(&app);
    // A pre-session is neither an authenticated session nor an identity grant.
    assert_eq!(
        send(&app, build("/probe", Some(&first), "")).0,
        StatusCode::UNAUTHORIZED
    );
    let mut invalid_csrf = build("/auth/sign-in", Some(&first), PASSWORD);
    invalid_csrf
        .headers_mut()
        .remove(browser_session::CSRF_HEADER);
    invalid_csrf
        .headers_mut()
        .insert("content-type", "invalid/type".parse().unwrap());
    let (status, headers) = send(&app, invalid_csrf);
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(headers[CACHE_CONTROL], "no-store");
    // Form failure still consumes the challenge before any password verification.
    let mut malformed = build("/auth/sign-in", Some(&first), PASSWORD);
    *malformed.body_mut() = Body::from("password=%GG");
    assert_eq!(send(&app, malformed).0, StatusCode::BAD_REQUEST);
    assert_eq!(
        send(&app, build("/auth/sign-in", Some(&first), PASSWORD)).0,
        StatusCode::FORBIDDEN
    );
    let second = challenge(&app);
    let (status, headers) = send(
        &app,
        build("/auth/sign-in", Some(&second), "wrong fixture password"),
    );
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(!headers.contains_key("set-cookie"));
    assert_eq!(
        send(&app, build("/auth/sign-in", Some(&second), PASSWORD)).0,
        StatusCode::FORBIDDEN
    );
    let third = challenge(&app);
    let (status, authenticated) = send(&app, build("/auth/sign-in", Some(&third), PASSWORD));
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_ne!(
        third[browser_session::CSRF_HEADER],
        authenticated[browser_session::CSRF_HEADER]
    );
    let pre_cookie = third["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let auth_cookie = authenticated["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    assert_ne!(
        pre_cookie.split_once('=').unwrap().1,
        auth_cookie.split_once('=').unwrap().1
    );
    assert_eq!(
        send(&app, build("/auth/sign-in", Some(&third), PASSWORD)).0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&app, build("/auth/sign-in", Some(&authenticated), PASSWORD)).0,
        StatusCode::FORBIDDEN
    );
    let mut probe = build("/probe", Some(&authenticated), "");
    // The inert pre-session cookie may coexist; it cannot replace auth CSRF.
    probe.headers_mut().insert(
        COOKIE,
        format!("{pre_cookie}; {auth_cookie}").parse().unwrap(),
    );
    assert_eq!(send(&app, probe).0, StatusCode::NO_CONTENT);
    let mut wrong_csrf = build("/probe", Some(&authenticated), "");
    wrong_csrf.headers_mut().insert(
        browser_session::CSRF_HEADER,
        third[browser_session::CSRF_HEADER].clone(),
    );
    assert_eq!(send(&app, wrong_csrf).0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        std::fs::read(directory.path().join("owner.bin")).unwrap(),
        original
    );
}
