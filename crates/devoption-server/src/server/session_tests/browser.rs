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
fn browser_session_credentials_are_rejected_in_authorization_transport() {
    let (_directory, auth, identity) = owner();
    let slot = Arc::new(SessionSlot::default());
    let issued = sign_in(&auth, &slot);
    let (app, calls) = router_for(identity, slot);
    let token = issued["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .split_once('=')
        .unwrap()
        .1;
    for values in [
        vec![format!("Bearer {token}")],
        vec![format!("bEaReR   {}", token.to_uppercase())],
        vec![format!("Bearer {token}"), format!("Bearer {token}")],
        vec![format!("Bearer {token}"), "Basic other".into()],
        vec![String::new()],
    ] {
        for (cookie, csrf) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut candidate = browser_request(&issued);
            if !cookie {
                candidate.headers_mut().remove(COOKIE);
            }
            if !csrf {
                candidate.headers_mut().remove(browser_session::CSRF_HEADER);
            }
            for value in &values {
                candidate
                    .headers_mut()
                    .append(AUTHORIZATION, value.parse().unwrap());
            }
            assert_unauthorized(&app, candidate);
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        response(&app, browser_request(&issued)).0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
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

/// Isolated, ten-minute fixture for the documented browser UI/transport check.
/// It seeds only temporary storage; all auth routes are the production handlers.
#[test]
#[ignore = "manual browser fixture; see docs/browser-session-check.md"]
fn manual_cookie_browser_fixture() {
    let (directory, auth, identity) = owner();
    let data = directory.path().canonicalize().unwrap();
    let repositories = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let root = repositories.path().canonicalize().unwrap();
    for name in ["frontend", "backend", "tools", "工具 & <fixture>"] {
        for child in ["", ".git", ".git/objects", ".git/refs"] {
            let path = root.join(name).join(child);
            std::fs::create_dir(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::fs::write(
            root.join(name).join(".git/config"),
            "[core]\nrepositoryformatversion = 0\nbare = false\n",
        )
        .unwrap();
        std::fs::write(root.join(name).join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    }
    std::fs::write(
        root.join("frontend/README.md"),
        "# Disposable browser fixture\n",
    )
    .unwrap();
    let mut catalog = devoption_projects::Catalog::default();
    let project = catalog.create_project("Example application").unwrap();
    let docs = catalog.create_project("Example documentation").unwrap();
    let permitted = devoption_projects::worktree::PermittedRoot::open_absolute(&root).unwrap();
    {
        let mut store = devoption_projects::CatalogStore::open_absolute(&data).unwrap();
        store.save(&catalog).unwrap();
        store
            .attach_repository(project, &permitted, Path::new("frontend"))
            .unwrap();
        store
            .attach_repository(project, &permitted, Path::new("backend"))
            .unwrap();
        store
            .attach_repository(docs, &permitted, Path::new("frontend"))
            .unwrap();
    }
    let enabled = std::env::var_os("DEVOPTION_BROWSER_ROOT_DISABLED").is_none();
    let store = crate::owner_store::OwnerStore::open_absolute(&data).unwrap();
    let projects =
        projects::Projects::open_with_root(&store, enabled.then_some(root.as_path())).unwrap();
    let profile = crate::profile::ProfileStore::new(&store, identity).unwrap();
    println!("Browser fixture root: {}", root.display());
    Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let slot = Arc::new(SessionSlot::default());
            let protected = protect(
                page_routes(
                    true,
                    Arc::clone(&slot),
                    projects::routes(projects).merge(profile::routes(profile)),
                )
                .route(
                    "/probe",
                    post(move |Extension(id): Extension<IdentityId>| async move {
                        assert_eq!(id, identity);
                        StatusCode::NO_CONTENT
                    }),
                ),
                port,
                Arc::clone(&slot),
            );
            let app = auth::routes(auth, port, slot).merge(protected);
            println!("Cookie fixture: http://127.0.0.1:{port}/ (expires after 600 seconds)");
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    tokio::time::sleep(Duration::from_secs(600)).await
                })
                .await
                .unwrap();
        });
}
