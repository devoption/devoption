use super::*;
use crate::{
    owner_auth::{self, service::OwnerAuthService},
    owner_store::OwnerStore,
};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt};

const PASSWORD: &str = "public test fixture password";
const EMAIL: &str = "member@example.test";
struct Fixture {
    _directory: tempfile::TempDir,
    app: Router,
    owner: HeaderMap,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let store = OwnerStore::open(directory.path()).unwrap();
        let id = owner_auth::setup(&store, PASSWORD).unwrap();
        let auth = OwnerAuthService::with_users(store).unwrap();
        let slot = Arc::new(SessionSlot::default());
        let runtime = Builder::new_current_thread().enable_all().build().unwrap();
        let owner = runtime.block_on(async {
            crate::browser_session::activate(
                auth.sign_in(PASSWORD.into()).await.unwrap().unwrap(),
                &slot,
            )
            .unwrap()
            .headers()
            .clone()
        });
        let owner_store = OwnerStore::open(directory.path()).unwrap();
        let projects = projects::Projects::open_with_root(&owner_store, None).unwrap();
        let profile = crate::profile::ProfileStore::new(&owner_store, id).unwrap();
        let protected = protect(
            page_routes(
                true,
                Arc::clone(&slot),
                projects::routes(projects).merge(profile::routes(profile)),
                false,
                None,
            ),
            3000,
            Arc::clone(&slot),
        );
        let app = auth::routes(auth, 3000, slot).merge(protected);
        Self {
            _directory: directory,
            app,
            owner,
        }
    }
    fn send(
        &self,
        method: &str,
        path: &str,
        credentials: Option<&HeaderMap>,
        body: &str,
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(HOST, "127.0.0.1:3000")
            .header(ORIGIN, "http://127.0.0.1:3000")
            .header("content-type", "text/plain;charset=UTF-8");
        if let Some(credentials) = credentials {
            request = request
                .header(
                    COOKIE,
                    credentials["set-cookie"]
                        .to_str()
                        .unwrap()
                        .split(';')
                        .next()
                        .unwrap(),
                )
                .header(CSRF_HEADER, &credentials[CSRF_HEADER]);
        }
        Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let response = self
                    .app
                    .clone()
                    .oneshot(request.body(Body::from(body.to_owned())).unwrap())
                    .await
                    .unwrap();
                let (parts, body) = response.into_parts();
                assert_eq!(parts.headers[CACHE_CONTROL], "no-store");
                let bytes = axum::body::to_bytes(body, 1024 * 1024).await.unwrap();
                (parts.status, parts.headers, bytes.to_vec())
            })
    }
    fn public(&self, path: &str, body: Value) -> (StatusCode, HeaderMap, Vec<u8>) {
        let challenge = self.send("POST", "/auth/challenge", None, "");
        assert_eq!(challenge.0, StatusCode::NO_CONTENT);
        self.send("POST", path, Some(&challenge.1), &body.to_string())
    }
    fn manage(&self, command: Value) -> (StatusCode, HeaderMap, Vec<u8>) {
        self.send(
            "POST",
            "/auth/users",
            Some(&self.owner),
            &command.to_string(),
        )
    }
    fn invite(&self) -> String {
        let result = self.manage(json!({"action":"invite", "email":EMAIL}));
        assert_eq!(result.0, StatusCode::OK);
        serde_json::from_slice::<Value>(&result.2).unwrap()["invitation"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    fn join(&self, token: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
        self.public(
            "/auth/register",
            json!({"email":EMAIL,"password":PASSWORD,"invitation":token}),
        )
    }
    fn login(&self) -> (StatusCode, HeaderMap, Vec<u8>) {
        self.public(
            "/auth/user-sign-in",
            json!({"email":EMAIL,"password":PASSWORD}),
        )
    }
}
#[test]
fn invitation_is_single_use_and_members_share_projects_but_cannot_manage_accounts() {
    let f = Fixture::new();
    let token = f.invite();
    assert_eq!(f.join(&token).0, StatusCode::ACCEPTED);
    let member = f.login();
    assert_eq!(member.0, StatusCode::NO_CONTENT);
    for path in ["/projects", "/settings/profile"] {
        assert_eq!(f.send("GET", path, Some(&member.1), "").0, StatusCode::OK);
    }
    // Original owner remains signed in after a different account signs in.
    assert_eq!(
        f.send("GET", "/auth/users", Some(&f.owner), "").0,
        StatusCode::OK
    );
    let create = f.send(
        "POST",
        "/projects",
        Some(&member.1),
        "Shared fixture project",
    );
    assert_eq!(create.0, StatusCode::NO_CONTENT);
    let projects = f.send("GET", "/projects", Some(&f.owner), "");
    assert!(
        String::from_utf8(projects.2)
            .unwrap()
            .contains("Shared fixture project")
    );
    for path in ["/auth/users", "/settings/users"] {
        assert_eq!(
            f.send("GET", path, Some(&member.1), "").0,
            StatusCode::FORBIDDEN
        );
    }
    for action in [
        json!({"action":"registration","enabled":true}),
        json!({"action":"invite","email":"other@example.test"}),
        json!({"action":"approve","email":EMAIL}),
        json!({"action":"disable","email":EMAIL}),
    ] {
        assert_eq!(
            f.send("POST", "/auth/users", Some(&member.1), &action.to_string())
                .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(f.join(&token).0, StatusCode::ACCEPTED); // No second credential replacement.
    assert_eq!(
        f.send(
            "POST",
            "/account/profile",
            Some(&member.1),
            "Member fixture"
        )
        .0,
        StatusCode::NO_CONTENT
    );
    assert!(
        f.send("GET", "/account/profile", Some(&f.owner), "")
            .2
            .is_empty()
    );
    assert_eq!(
        f.manage(json!({"action":"disable","email":EMAIL})).0,
        StatusCode::OK
    );
    assert_eq!(
        f.send("POST", "/auth/session", Some(&member.1), "").0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(f.login().0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        f.send("POST", "/auth/session", Some(&f.owner), "").0,
        StatusCode::NO_CONTENT
    );
}
#[test]
fn registration_is_closed_by_default_and_pending_accounts_require_explicit_approval() {
    let f = Fixture::new();
    assert_eq!(f.join("").0, StatusCode::ACCEPTED);
    let list = f.send("GET", "/auth/users", Some(&f.owner), "");
    assert_eq!(
        serde_json::from_slice::<Value>(&list.2).unwrap()["users"],
        json!([])
    );
    assert_eq!(
        f.manage(json!({"action":"registration","enabled":true})).0,
        StatusCode::OK
    );
    let joined = f.join("");
    assert_eq!(joined.0, StatusCode::ACCEPTED);
    assert!(!joined.1.contains_key("set-cookie"));
    assert_eq!(f.login().0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        f.manage(json!({"action":"approve","email":EMAIL})).0,
        StatusCode::OK
    );
    assert_eq!(
        f.manage(json!({"action":"registration","enabled":false})).0,
        StatusCode::OK
    );
    assert_eq!(f.login().0, StatusCode::NO_CONTENT);
}
#[test]
fn account_mutations_require_owner_session_csrf_and_same_origin() {
    let f = Fixture::new();
    assert_eq!(
        f.send("POST", "/auth/users", None, "{}").0,
        StatusCode::UNAUTHORIZED
    );
    for path in ["/auth/register", "/auth/user-sign-in"] {
        assert_eq!(f.send("POST", path, None, "{}").0, StatusCode::FORBIDDEN);
    }
    for (header, value, expected) in [
        (ORIGIN, "https://attacker.invalid", StatusCode::FORBIDDEN),
        (HOST, "attacker.invalid", StatusCode::BAD_REQUEST),
        (
            axum::http::HeaderName::from_static(CSRF_HEADER),
            "00",
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/auth/users")
            .header(HOST, "127.0.0.1:3000")
            .header(ORIGIN, "http://127.0.0.1:3000")
            .header(
                COOKIE,
                f.owner["set-cookie"]
                    .to_str()
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap(),
            )
            .header(CSRF_HEADER, &f.owner[CSRF_HEADER])
            .body(Body::from("{}"))
            .unwrap();
        request.headers_mut().insert(header, value.parse().unwrap());
        let response = Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(f.app.clone().oneshot(request))
            .unwrap();
        assert_eq!(response.status(), expected);
    }
}
