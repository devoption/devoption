use super::*;
use devoption_builds::history::History;
use devoption_projects::{Catalog, CatalogStore};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

pub(super) fn seed(
    path: &Path,
    project: [u8; 16],
    repository: [u8; 16],
    other: [u8; 16],
    other_repo: [u8; 16],
) -> History {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    let hex = |bytes: [u8; 16]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let result = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/server/session_tests/history_fixture.py"
        ))
        .arg(path)
        .args([hex(project), hex(repository), hex(other), hex(other_repo)])
        .output()
        .unwrap();
    assert!(result.status.success());
    History::open(path.to_str().unwrap()).unwrap()
}
struct Fixture {
    _temp: tempfile::TempDir,
    data: std::path::PathBuf,
    history: std::path::PathBuf,
    app: Router,
    project: String,
    other: String,
    slot: SharedSession,
    unassociated: [u8; 16],
}
fn fixture(configured: bool) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let data = root.join("data");
    fs::create_dir(&data).unwrap();
    fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
    let owner = crate::owner_store::OwnerStore::open_absolute(&data).unwrap();
    let mut catalog = Catalog::default();
    let project = catalog.create_project("<Project>").unwrap();
    let other = catalog.create_project("Other project").unwrap();
    let repo = catalog.create_repository("<Repository>").unwrap();
    let unassociated = catalog.create_repository("Unassociated").unwrap();
    catalog.associate(project, repo).unwrap();
    catalog.associate(other, repo).unwrap();
    {
        let mut store = CatalogStore::open_absolute(&data).unwrap();
        store.save(&catalog).unwrap();
    }
    let history = root.join("history");
    let mut projects = projects::Projects::open_with_root(&owner, None).unwrap();
    if configured {
        let mut store = seed(
            &history,
            project.to_bytes(),
            repo.to_bytes(),
            other.to_bytes(),
            repo.to_bytes(),
        );
        projects.recover_build_history(&mut store).unwrap();
        projects = projects.with_history(store);
    }
    let slot = Arc::new(SessionSlot::default());
    slot.activate(
        Session::new(IDENTITY, FIXTURE, MAX_LIFETIME)
            .unwrap()
            .with_csrf(CSRF),
        &FIXTURE,
    )
    .unwrap();
    let app = protect(
        page_routes(true, Arc::clone(&slot), projects::routes(projects)),
        3000,
        Arc::clone(&slot),
    );
    let hex = |bytes: [u8; 16]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    Fixture {
        _temp: temp,
        data,
        history,
        app,
        project: hex(project.to_bytes()),
        other: hex(other.to_bytes()),
        slot,
        unassociated: unassociated.to_bytes(),
    }
}
fn read(f: &Fixture, path: &str, authenticated: bool) -> (StatusCode, String) {
    let mut request = Request::builder().uri(path).header(HOST, "127.0.0.1:3000");
    if authenticated {
        request = request.header(
            COOKIE,
            format!("__Host-devoption_session={}", encode(&FIXTURE)),
        );
    }
    Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let response = f
                .app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
                .await
                .unwrap();
            (status, String::from_utf8(bytes.to_vec()).unwrap())
        })
}
#[test]
fn saved_history_and_logs_are_authenticated_scoped_and_escaped() {
    let f = fixture(true);
    let base = format!("/projects/{}/builds", f.project);
    let detail = format!("{base}/attempts/{}", "02".repeat(16));
    for path in [&base, &detail] {
        assert_eq!(read(&f, path, false).0, StatusCode::SEE_OTHER);
    }
    let (status, html) = read(&f, &base, true);
    assert_eq!(status, StatusCode::OK);
    for text in [
        "Interrupted",
        "Failed",
        "Succeeded",
        "&lt;Repository&gt;",
        "View attempt",
    ] {
        assert!(html.contains(text), "{text}");
    }
    assert!(!html.contains("Cancelled"));
    let (status, html) = read(&f, &detail, true);
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("&lt;script&gt;fixture-only&lt;/script&gt;"));
    assert!(!html.contains("<script>fixture-only"));
    assert!(html.contains("\\u{1b}"));
    assert!(html.contains("\\u{202e}"));
    assert!(!html.contains('\u{202e}'));
    assert!(!html.contains('\u{1b}'));
    assert!(html.contains("Output was truncated"));
    assert!(html.contains("No artifact download"));
    for path in [
        format!("{base}/attempts/{}", "04".repeat(16)),
        format!("/projects/{}/builds/attempts/{}", f.other, "01".repeat(16)),
        format!("{base}/attempts/invalid"),
    ] {
        let (status, body) = read(&f, &path, true);
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(!body.contains("fixture-only"));
    }
    let (_, missing) = read(&f, &format!("{base}/attempts/{}", "03".repeat(16)), true);
    assert!(missing.contains("No output checkpoint"));
    assert!(f.slot.logout(Some(&FIXTURE)));
    assert_eq!(read(&f, &detail, true).0, StatusCode::SEE_OTHER);
}
#[test]
fn unavailable_history_and_logs_are_not_mistaken_for_empty_or_mutated() {
    let f = fixture(true);
    let base = format!("/projects/{}/builds", f.project);
    let log = f
        .history
        .join(format!("build-output-{}.bin", "02".repeat(16)));
    fs::write(&log, b"corrupt").unwrap();
    let (status, html) = read(&f, &format!("{base}/attempts/{}", "02".repeat(16)), true);
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(html.contains("Saved output could not be loaded"));
    assert_eq!(fs::read(&log).unwrap(), b"corrupt");
    fs::write(f.history.join("build-history.bin"), b"corrupt history").unwrap();
    let (status, html) = read(&f, &base, true);
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(html.contains("Build history could not be loaded"));
    assert!(!html.contains("No saved build attempts"));
}
#[test]
fn fresh_catalog_checks_precede_log_access() {
    let f = fixture(true);
    let old = fs::read(f.history.join("build-history.bin")).unwrap();
    let mut catalog = Catalog::default();
    catalog.create_project("Replacement").unwrap();
    CatalogStore::open_absolute(&f.data)
        .unwrap()
        .save(&catalog)
        .unwrap();
    let (status, html) = read(
        &f,
        &format!(
            "/projects/{}/builds/attempts/{}",
            f.project,
            "02".repeat(16)
        ),
        true,
    );
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!html.contains("fixture-only"));
    assert_eq!(fs::read(f.history.join("build-history.bin")).unwrap(), old);
}
#[test]
fn unconfigured_history_has_its_own_state() {
    let f = fixture(false);
    let (status, html) = read(&f, &format!("/projects/{}/builds", f.project), true);
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Build history is not configured"));
    assert!(!f.history.exists());
}

#[test]
fn saved_repository_must_still_belong_to_the_selected_project() {
    let f = fixture(true);
    let replacement = f.history.with_extension("replacement");
    let catalog = devoption_projects::read_catalog(&f.data).unwrap().unwrap();
    let project = catalog
        .projects()
        .iter()
        .find(|p| p.name() == "<Project>")
        .unwrap()
        .id()
        .to_bytes();
    let other = catalog
        .projects()
        .iter()
        .find(|p| p.name() == "Other project")
        .unwrap()
        .id()
        .to_bytes();
    let _replacement = seed(&replacement, project, f.unassociated, other, f.unassociated);
    fs::copy(
        replacement.join("build-history.bin"),
        f.history.join("build-history.bin"),
    )
    .unwrap();
    let log = f
        .history
        .join(format!("build-output-{}.bin", "02".repeat(16)));
    let before = fs::read(&log).unwrap();
    for path in [
        format!("/projects/{}/builds", f.project),
        format!(
            "/projects/{}/builds/attempts/{}",
            f.project,
            "02".repeat(16)
        ),
    ] {
        let (status, body) = read(&f, &path, true);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(!body.contains("fixture-only"));
    }
    assert_eq!(fs::read(log).unwrap(), before);
}

#[test]
fn history_detail_rejects_unsafe_host_and_mutation_methods() {
    let f = fixture(true);
    let path = format!(
        "/projects/{}/builds/attempts/{}",
        f.project,
        "02".repeat(16)
    );
    Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            for (method, host) in [
                (Method::GET, "attacker.invalid"),
                (Method::POST, "127.0.0.1:3000"),
                (Method::DELETE, "127.0.0.1:3000"),
            ] {
                let request = Request::builder()
                    .method(method)
                    .uri(&path)
                    .header(HOST, host)
                    .header(
                        COOKIE,
                        format!("__Host-devoption_session={}", encode(&FIXTURE)),
                    )
                    .body(Body::empty())
                    .unwrap();
                let response = f.app.clone().oneshot(request).await.unwrap();
                assert!(!response.status().is_success());
                let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap();
                assert!(!String::from_utf8_lossy(&body).contains("fixture-only"));
            }
            let request = Request::builder()
                .method(Method::HEAD)
                .uri(&path)
                .header(HOST, "127.0.0.1:3000")
                .header(
                    COOKIE,
                    format!("__Host-devoption_session={}", encode(&FIXTURE)),
                )
                .body(Body::empty())
                .unwrap();
            let response = f.app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
            assert!(
                axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap()
                    .is_empty()
            );
        });
}

#[test]
fn build_actions_require_session_csrf_and_explicit_runner_without_mutating_history() {
    fn action_status(app: &Router, request: Request) -> StatusCode {
        Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async { app.clone().oneshot(request).await.unwrap().status() })
    }

    let f = fixture(true);
    let saved = fs::read(f.history.join("build-history.bin")).unwrap();
    let repository: String = f.unassociated.iter().map(|b| format!("{b:02x}")).collect();
    let base = format!("/projects/{}/builds/{repository}", f.project);
    assert_eq!(
        read(&f, &format!("{base}/run"), false).0,
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        read(&f, &format!("{base}/run"), true).0,
        StatusCode::NOT_FOUND
    );
    for action in ["prepare", "approve", "cancel"] {
        for authenticated in [false, true] {
            let body = if action == "approve" {
                "ab".repeat(16)
            } else {
                String::new()
            };
            let mut request = Request::builder()
                .method(Method::POST)
                .uri(format!("{base}/{action}"))
                .header(HOST, "127.0.0.1:3000")
                .header(ORIGIN, "http://127.0.0.1:3000")
                .header("content-type", "text/plain;charset=UTF-8")
                .body(Body::from(body))
                .unwrap();
            if authenticated {
                authorize(&mut request, &FIXTURE, &CSRF);
            }
            let status = action_status(&f.app, request);
            assert_eq!(
                status,
                if authenticated {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::UNAUTHORIZED
                }
            );
        }
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(format!("{base}/{action}"))
            .header(HOST, "127.0.0.1:3000")
            .header(ORIGIN, "http://127.0.0.1:3000")
            .header("content-type", "text/plain;charset=UTF-8")
            .body(Body::from("x".repeat(33)))
            .unwrap();
        authorize(&mut request, &FIXTURE, &CSRF);
        assert!(action_status(&f.app, request).is_client_error());
    }
    assert_eq!(
        fs::read(f.history.join("build-history.bin")).unwrap(),
        saved
    );
}
