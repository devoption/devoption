use super::*;
use crate::{
    browser_session::encode,
    owner_store::OwnerStore,
    session::slot::SessionSlot,
    session::{MAX_LIFETIME, Session},
};
use axum::{body::Body, http::header::COOKIE};
use std::{fs, os::unix::fs::PermissionsExt};
use tower::ServiceExt;
const TOKEN: [u8; 32] = [0xab; 32];
const CSRF: [u8; 32] = [0xef; 32];
const OWNER: IdentityId = IdentityId::from_bytes([1; 16]);
const VALUE: &str = "http-fixture-secret-never-disclose";
struct Fixture {
    _temp: tempfile::TempDir,
    projects: Projects,
    id: ProjectId,
    other: ProjectId,
}
impl Fixture {
    fn new(enabled: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let data = base.join("data");
        fs::create_dir(&data).unwrap();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
        let owner = OwnerStore::open_absolute(&data).unwrap();
        let mut catalog = Catalog::default();
        let id = catalog.create_project("Fixture project").unwrap();
        let other = catalog.create_project("Other project").unwrap();
        devoption_projects::CatalogStore::open_absolute(&data)
            .unwrap()
            .save(&catalog)
            .unwrap();
        let mut projects = Projects::open_with_root(&owner, None).unwrap();
        if enabled {
            let key = base.join("fixture.key");
            fs::write(&key, [1; 32]).unwrap();
            fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
            projects
                .enable_secrets(
                    Store::open(
                        owner.duplicate_directory().unwrap(),
                        &key,
                        OWNER.to_bytes(),
                        &[],
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        Self {
            _temp: temp,
            projects,
            id,
            other,
        }
    }
    fn path(&self) -> String {
        format!(
            "/projects/{}/environments/production",
            hex(&self.id.to_bytes())
        )
    }
    fn router(&self) -> Router {
        let session = Arc::new(SessionSlot::default());
        session
            .activate(
                Session::new(OWNER, TOKEN, MAX_LIFETIME)
                    .unwrap()
                    .with_csrf(CSRF),
                &TOKEN,
            )
            .unwrap();
        let pages = super::super::super::page_routes(
            true,
            session.clone(),
            super::super::routes(self.projects.clone()),
            false,
            None,
        );
        super::super::super::protect(pages, 3000, session)
    }
}
async fn response(
    app: &Router,
    method: &str,
    path: &str,
    body: &str,
    authenticated: bool,
    origin: &str,
    csrf: bool,
) -> (StatusCode, String) {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header(HOST, "127.0.0.1:3000")
        .header(ORIGIN, origin)
        .header("content-type", "text/plain");
    if authenticated {
        b = b.header(
            COOKIE,
            format!("__Host-devoption_session={}", encode(&TOKEN)),
        );
    }
    if csrf {
        b = b.header("x-csrf-token", encode(&CSRF));
    }
    let r = app
        .clone()
        .oneshot(b.body(Body::from(body.to_owned())).unwrap())
        .await
        .unwrap();
    let (status, headers) = (r.status(), r.headers().clone());
    assert_eq!(headers.get(CACHE_CONTROL).unwrap(), "no-store");
    let text = String::from_utf8(
        axum::body::to_bytes(r.into_body(), 256 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(!text.contains(VALUE));
    (status, text)
}
fn runtime() -> tokio::runtime::Runtime {
    Builder::new_current_thread().enable_all().build().unwrap()
}
#[test]
fn routes_enforce_owner_csrf_origin_and_write_only_project_environment_scoping() {
    runtime().block_on(async {
        let f = Fixture::new(true);
        let app = f.router();
        let path = f.path();
        let write = format!("{path}/secrets/SERVICE_KEY");
        let body = format!(r#"{{"value":"{VALUE}","revision":0}}"#);
        let origin = "http://127.0.0.1:3000";
        for (auth, csrf, origin, expected) in [
            (false, false, origin, StatusCode::UNAUTHORIZED),
            (true, false, origin, StatusCode::UNAUTHORIZED),
            (true, true, "https://foreign.example", StatusCode::FORBIDDEN),
        ] {
            assert_eq!(
                response(&app, "POST", &write, &body, auth, origin, csrf)
                    .await
                    .0,
                expected
            );
        }
        assert_eq!(
            response(&app, "GET", &path, "", false, origin, false)
                .await
                .0,
            StatusCode::SEE_OTHER
        );
        assert_eq!(
            response(&app, "POST", &write, &body, true, origin, true)
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        let list = response(&app, "GET", &path, "", true, origin, false).await;
        assert_eq!(list.0, StatusCode::OK);
        assert!(list.1.contains("SERVICE_KEY"));
        assert!(list.1.contains("Revision 1"));
        assert!(!list.1.contains("value=\"http-fixture"));
        let preview = path.replace("production", "preview");
        assert!(
            !response(&app, "GET", &preview, "", true, origin, false)
                .await
                .1
                .contains("SERVICE_KEY")
        );
        let other = path.replace(&hex(&f.id.to_bytes()), &hex(&f.other.to_bytes()));
        assert!(
            !response(&app, "GET", &other, "", true, origin, false)
                .await
                .1
                .contains("SERVICE_KEY")
        );
        assert_eq!(
            response(&app, "POST", &write, &body, true, origin, true)
                .await
                .0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            response(
                &app,
                "POST",
                &format!("{write}/remove"),
                "1",
                true,
                origin,
                true
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        assert!(
            response(&app, "GET", &path, "", true, origin, false)
                .await
                .1
                .contains("No secrets in this environment")
        );
    });
}
#[test]
fn missing_projects_invalid_environments_and_unconfigured_storage_do_not_disclose_or_mutate() {
    runtime().block_on(async {
        let f = Fixture::new(false);
        let app = f.router();
        let path = f.path();
        let origin = "http://127.0.0.1:3000";
        assert!(
            response(&app, "GET", &path, "", true, origin, false)
                .await
                .1
                .contains("Secret storage is not enabled")
        );
        assert_eq!(
            response(
                &app,
                "POST",
                &format!("{path}/secrets/KEY"),
                r#"{"value":"fixture","revision":0}"#,
                true,
                origin,
                true
            )
            .await
            .0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        for p in [
            path.replace("production", "unknown"),
            path.replace(&hex(&f.id.to_bytes()), &"00".repeat(16)),
        ] {
            assert_eq!(
                response(&app, "GET", &p, "", true, origin, false).await.0,
                StatusCode::NOT_FOUND
            );
        }
    });
}
#[test]
fn corrupt_ciphertext_is_visible_as_unavailable_and_cannot_be_overwritten() {
    runtime().block_on(async {
        let f = Fixture::new(true);
        let app = f.router();
        let path = f.path();
        let origin = "http://127.0.0.1:3000";
        let write = format!("{path}/secrets/KEY");
        assert_eq!(
            response(
                &app,
                "POST",
                &write,
                r#"{"value":"fixture","revision":0}"#,
                true,
                origin,
                true
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        let data = f._temp.path().join("data");
        let file = fs::read_dir(&data)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("project-secrets-")
                    && p.extension().is_some_and(|e| e == "bin")
            })
            .unwrap();
        fs::write(&file, b"tampered").unwrap();
        assert_eq!(
            response(&app, "GET", &path, "", true, origin, false)
                .await
                .0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            response(
                &app,
                "POST",
                &write,
                r#"{"value":"replacement","revision":1}"#,
                true,
                origin,
                true
            )
            .await
            .0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(fs::read(file).unwrap(), b"tampered");
    });
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn simultaneous_replacements_have_one_winner_and_invalid_bodies_never_write() {
    runtime().block_on(async {
        let f = Fixture::new(true);
        let app = f.router();
        let path = f.path();
        let write = format!("{path}/secrets/KEY");
        let origin = "http://127.0.0.1:3000";
        let body = r#"{"value":"first fixture","revision":0}"#;
        assert_eq!(
            response(&app, "POST", &write, body, true, origin, true)
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        let replacement = r#"{"value":"new fixture","revision":1}"#;
        let (a, b) = tokio::join!(
            response(&app, "POST", &write, replacement, true, origin, true),
            response(&app, "POST", &write, replacement, true, origin, true)
        );
        assert!(matches!(
            (a.0, b.0),
            (StatusCode::NO_CONTENT, StatusCode::CONFLICT)
                | (StatusCode::CONFLICT, StatusCode::NO_CONTENT)
        ));
        for invalid in [
            format!(r#"{{"value":"{VALUE}","revision":2,"unexpected":true}}"#),
            format!(r#"{{"value":"{VALUE}","value":"duplicate","revision":2}}"#),
            format!(
                r#"{{"value":"{}","revision":2}}"#,
                "x".repeat(MAX_VALUE + 1)
            ),
        ] {
            assert_eq!(
                response(&app, "POST", &write, &invalid, true, origin, true)
                    .await
                    .0,
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(
            response(
                &app,
                "POST",
                &write,
                &"x".repeat(6 * MAX_VALUE + 257),
                true,
                origin,
                true
            )
            .await
            .0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert!(
            response(&app, "GET", &path, "", true, origin, false)
                .await
                .1
                .contains("Revision 2")
        );
    });
}

#[test]
fn catalog_removal_blocks_existing_secret_reads_and_writes_without_erasing_them() {
    runtime().block_on(async {
        let f = Fixture::new(true);
        let app = f.router();
        let path = f.path();
        let origin = "http://127.0.0.1:3000";
        let write = format!("{path}/secrets/KEY");
        assert_eq!(
            response(
                &app,
                "POST",
                &write,
                r#"{"value":"fixture","revision":0}"#,
                true,
                origin,
                true
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        let data = f._temp.path().join("data");
        let snapshots: Vec<_> = fs::read_dir(&data)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("project-secrets-")
            })
            .map(|p| (p.clone(), fs::read(&p).unwrap()))
            .collect();
        devoption_projects::CatalogStore::open_absolute(&data.canonicalize().unwrap())
            .unwrap()
            .save(&Catalog::default())
            .unwrap();
        assert_eq!(
            response(&app, "GET", &path, "", true, origin, false)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            response(
                &app,
                "POST",
                &write,
                r#"{"value":"replacement","revision":1}"#,
                true,
                origin,
                true
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        for (path, before) in snapshots {
            assert_eq!(fs::read(path).unwrap(), before);
        }
    });
}
