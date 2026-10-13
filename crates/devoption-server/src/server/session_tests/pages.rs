use super::*;

const PATHS: [&str; 4] = ["/", "/projects", "/about", "/settings/appearance"];
const CSRF: [u8; 32] = [0xcd; 32];

fn app(configured: bool, slot: SharedSession) -> Router {
    let projects = if configured {
        Router::new().route(
            "/projects",
            get(|| async {
                (
                    [(CACHE_CONTROL, "no-store")],
                    Html(devoption_ui::render_projects(&[], false)),
                )
            }),
        )
    } else {
        Router::new()
    };
    protect(
        page_routes(configured, Arc::clone(&slot), projects),
        3000,
        slot,
    )
}

fn read(method: Method, path: &str, token: Option<&[u8]>) -> Request {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(HOST, "127.0.0.1:3000");
    if let Some(token) = token {
        builder = builder.header(
            COOKIE,
            format!(
                "__Host-devoption_session={}",
                crate::browser_session::encode(token)
            ),
        );
    }
    builder.body(Body::empty()).unwrap()
}

fn reply(app: &Router, request: Request) -> (StatusCode, HeaderMap, Vec<u8>) {
    Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let (parts, body) = app.clone().oneshot(request).await.unwrap().into_parts();
            let body = axum::body::to_bytes(body, 128 * 1024).await.unwrap();
            (parts.status, parts.headers, body.to_vec())
        })
}

fn redirected(app: &Router, request: Request) {
    let (status, headers, body) = reply(app, request);
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[LOCATION], "/sign-in");
    assert_eq!(headers[CACHE_CONTROL], "no-store");
    assert!(body.is_empty());
    for name in ["set-cookie", "x-csrf-token", "access-control-allow-origin"] {
        assert!(!headers.contains_key(name));
    }
}

fn activate(slot: &SessionSlot, token: [u8; TOKEN_BYTES], lifetime: Duration) {
    slot.activate(
        Session::new(IDENTITY, token, lifetime)
            .unwrap()
            .with_csrf(CSRF),
        &token,
    )
    .unwrap();
}

#[test]
fn configured_get_and_head_pages_require_a_strict_live_cookie() {
    let slot = Arc::new(SessionSlot::default());
    activate(&slot, FIXTURE, MAX_LIFETIME);
    let app = app(true, Arc::clone(&slot));
    for method in [Method::GET, Method::HEAD] {
        for path in PATHS {
            redirected(&app, read(method.clone(), path, None));
            let request = read(method.clone(), path, Some(&FIXTURE));
            assert_eq!(page_identity(request.headers(), &slot), Some(IDENTITY));
            let (status, headers, body) = reply(&app, request);
            assert_eq!(status, StatusCode::OK);
            assert_eq!(headers[CACHE_CONTROL], "no-store");
            assert!(!headers.contains_key("set-cookie"));
            assert!(!headers.contains_key("x-csrf-token"));
            assert_eq!(body.is_empty(), method == Method::HEAD);
        }
    }
    let cookie = format!("__Host-devoption_session={}", "ab".repeat(32));
    for fields in [
        vec![(COOKIE, format!("{cookie}; {cookie}"))],
        vec![(COOKIE, cookie.clone()), (COOKIE, cookie.clone())],
        vec![(COOKIE, cookie.clone()), (AUTHORIZATION, credential())],
        vec![(AUTHORIZATION, credential())],
        vec![(COOKIE, "__Host-devoption_session=invalid".into())],
        vec![(
            COOKIE,
            format!("__Host-devoption_session={}", "00".repeat(32)),
        )],
        vec![(COOKIE, format!("other={}", "ab".repeat(32)))],
    ] {
        for method in [Method::GET, Method::HEAD] {
            let mut request = read(
                method,
                "/projects?preview=true&next=https://attacker.invalid/",
                None,
            );
            for (name, value) in &fields {
                request.headers_mut().append(name, value.parse().unwrap());
            }
            redirected(&app, request);
        }
    }
    redirected(
        &app,
        read(
            Method::GET,
            &format!("/projects?session={}", "ab".repeat(32)),
            None,
        ),
    );
}

#[test]
fn page_reads_observe_expiry_rotation_and_logout_and_never_replace_mutation_csrf() {
    let slot = Arc::new(SessionSlot::default());
    let app = app(true, Arc::clone(&slot));
    activate(&slot, FIXTURE, Duration::from_secs(1));
    assert_eq!(
        reply(&app, read(Method::GET, "/", Some(&FIXTURE))).0,
        StatusCode::OK
    );
    // A real HTTP read must not renew the core's absolute deadline.
    std::thread::sleep(Duration::from_secs(1));
    for method in [Method::GET, Method::HEAD] {
        redirected(&app, read(method, "/", Some(&FIXTURE)));
    }
    activate(&slot, FIXTURE, MAX_LIFETIME);
    let replacement = [0xef; TOKEN_BYTES];
    activate(&slot, replacement, MAX_LIFETIME);
    redirected(&app, read(Method::GET, "/projects", Some(&FIXTURE)));
    assert_eq!(
        reply(&app, read(Method::GET, "/projects", Some(&replacement))).0,
        StatusCode::OK
    );
    let mut mutation = read(Method::POST, "/projects", Some(&replacement));
    mutation
        .headers_mut()
        .insert(ORIGIN, "http://127.0.0.1:3000".parse().unwrap());
    assert_eq!(reply(&app, mutation).0, StatusCode::UNAUTHORIZED);
    assert!(slot.logout_browser(Some(&replacement), Some(&CSRF)));
    for method in [Method::GET, Method::HEAD] {
        redirected(&app, read(method, "/projects", Some(&replacement)));
    }
}

#[test]
fn sign_in_and_unconfigured_preview_remain_public_but_host_checks_still_run_first() {
    for configured in [false, true] {
        let app = app(configured, Arc::new(SessionSlot::default()));
        for method in [Method::GET, Method::HEAD] {
            let (status, headers, _) = reply(&app, read(method.clone(), "/sign-in", None));
            assert_eq!(status, StatusCode::OK);
            assert!(!headers.contains_key(LOCATION));
            for path in PATHS {
                if !configured {
                    assert_eq!(
                        reply(&app, read(method.clone(), path, None)).0,
                        StatusCode::OK
                    );
                }
                let mut invalid = read(method.clone(), path, None);
                invalid
                    .headers_mut()
                    .insert(HOST, "attacker.invalid".parse().unwrap());
                assert_eq!(reply(&app, invalid).0, StatusCode::BAD_REQUEST);
            }
        }
        // Unsupported safe methods expose no page body, with or without auth.
        for method in [Method::OPTIONS, Method::TRACE] {
            let (status, _, body) = reply(&app, read(method, "/projects", None));
            assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
            assert!(body.is_empty());
        }
    }
}
