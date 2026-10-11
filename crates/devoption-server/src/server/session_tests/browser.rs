use super::*;
use crate::{browser_session, owner_auth::service::OwnerAuthService, owner_store::OwnerStore};
use std::os::unix::fs::PermissionsExt;

pub(super) const PASSWORD: &str = "browser transport fixture only";

pub(super) fn owner() -> (tempfile::TempDir, OwnerAuthService, IdentityId) {
    let directory = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let auth = OwnerAuthService::new(OwnerStore::open(directory.path()).unwrap());
    let identity = Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(auth.setup(PASSWORD.into()))
        .unwrap();
    (directory, auth, identity)
}

fn sign_in(auth: &OwnerAuthService, slot: &SessionSlot) -> HeaderMap {
    Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            browser_session::activate(auth.sign_in(PASSWORD.into()).await.unwrap().unwrap(), slot)
                .unwrap()
                .headers()
                .clone()
        })
}

fn browser_request(issued: &HeaderMap) -> Request {
    let mut request = request();
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
    request
}

#[test]
fn cookie_mutations_require_explicit_csrf_without_bearer_or_alternate_fallback() {
    let (_directory, auth, identity) = owner();
    let slot = Arc::new(SessionSlot::default());
    let issued = sign_in(&auth, &slot);
    let (app, calls) = router_for(identity, slot);
    assert_eq!(
        response(&app, browser_request(&issued)).0,
        StatusCode::NO_CONTENT
    );
    for name in [COOKIE.as_str(), browser_session::CSRF_HEADER] {
        let mut missing = browser_request(&issued);
        missing.headers_mut().remove(name);
        assert_unauthorized(&app, missing);
        let mut duplicate = browser_request(&issued);
        let value = duplicate.headers()[name].clone();
        duplicate.headers_mut().append(name, value);
        assert_unauthorized(&app, duplicate);
    }
    let mut wrong = browser_request(&issued);
    wrong.headers_mut().insert(
        browser_session::CSRF_HEADER,
        "00".repeat(32).parse().unwrap(),
    );
    assert_unauthorized(&app, wrong);
    let mut alternatives = browser_request(&issued);
    alternatives
        .headers_mut()
        .remove(browser_session::CSRF_HEADER);
    let csrf = issued[browser_session::CSRF_HEADER].to_str().unwrap();
    *alternatives.uri_mut() = format!("/probe?csrf_token={csrf}").parse().unwrap();
    *alternatives.body_mut() = Body::from(format!("csrf_token={csrf}"));
    alternatives
        .headers_mut()
        .insert("x-forwarded-csrf-token", csrf.parse().unwrap());
    assert_unauthorized(&app, alternatives);
    let mut mixed = browser_request(&issued);
    let token = issued["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .split_once('=')
        .unwrap()
        .1;
    mixed
        .headers_mut()
        .insert(AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
    assert_unauthorized(&app, mixed);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn cookie_credentials_cannot_bypass_host_origin_rotation_or_logout() {
    let (_directory, auth, identity) = owner();
    let slot = Arc::new(SessionSlot::default());
    let first = sign_in(&auth, &slot);
    let (app, calls) = router_for(identity, Arc::clone(&slot));
    for (header, value, status) in [
        (HOST, "attacker.invalid", StatusCode::BAD_REQUEST),
        (ORIGIN, "http://localhost:3000", StatusCode::FORBIDDEN),
        (ORIGIN, "http://127.0.0.1:3001", StatusCode::FORBIDDEN),
        (ORIGIN, "null", StatusCode::FORBIDDEN),
    ] {
        let mut invalid = browser_request(&first);
        invalid.headers_mut().insert(header, value.parse().unwrap());
        assert_eq!(response(&app, invalid).0, status);
    }
    let mut missing_origin = browser_request(&first);
    missing_origin.headers_mut().remove(ORIGIN);
    assert_eq!(response(&app, missing_origin).0, StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let second = sign_in(&auth, &slot);
    assert_unauthorized(&app, browser_request(&first));
    let mut mixed = browser_request(&second);
    mixed.headers_mut().insert(
        browser_session::CSRF_HEADER,
        first[browser_session::CSRF_HEADER].clone(),
    );
    assert_unauthorized(&app, mixed);
    assert_eq!(
        response(&app, browser_request(&second)).0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        browser_session::logout(browser_request(&first).headers(), &slot).status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        browser_session::logout(browser_request(&second).headers(), &slot).status(),
        StatusCode::NO_CONTENT
    );
    assert_unauthorized(&app, browser_request(&second));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// Isolated, three-minute fixture for the documented manual browser transport check.
/// Its activation route uses a fixed fixture password, not the product sign-in flow.
#[test]
#[ignore = "manual browser fixture; see docs/browser-session-check.md"]
fn manual_cookie_browser_fixture() {
    let (_directory, auth, identity) = owner();
    Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let slot = Arc::new(SessionSlot::default());
        let logout_slot = Arc::clone(&slot);
        let protected = protect(Router::new()
            .route("/probe", post(move |Extension(id): Extension<IdentityId>| async move {
                assert_eq!(id, identity);
                StatusCode::NO_CONTENT
            }))
            .route("/logout", post(move |headers: HeaderMap| async move { browser_session::logout(&headers, &logout_slot) })), port, Arc::clone(&slot));
        let app = protected
            .route("/", get(|| async { Html("<!doctype html><title>Cookie fixture</title><p>Temporary cookie transport fixture.</p>") }))
            .route("/fixture/activate", post(move |request: Request| {
                let auth = auth.clone();
                let slot = Arc::clone(&slot);
                async move {
                    let Some(host) = validated_host(&request, port) else { return StatusCode::BAD_REQUEST.into_response() };
                    if !same_origin(&request, host, port) { return StatusCode::FORBIDDEN.into_response() }
                    browser_session::activate(auth.sign_in(PASSWORD.into()).await.unwrap().unwrap(), &slot).unwrap()
                }
            }));
        println!("Cookie fixture: http://127.0.0.1:{port}/ (expires after 180 seconds)");
        axum::serve(listener, app).with_graceful_shutdown(async { tokio::time::sleep(Duration::from_secs(180)).await }).await.unwrap();
    });
}
