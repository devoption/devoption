use super::*;
use crate::{
    owner_auth,
    owner_store::OwnerStore,
    session::slot::SessionSlot,
    session::{MAX_LIFETIME, Session},
};
use axum::{
    body::Body,
    http::header::{HOST, ORIGIN},
};
use std::{fs, os::unix::fs::PermissionsExt};
use tower::ServiceExt;
const ISSUER: &str = "https://platform.example.test";
const REDIRECT: &str = "http://127.0.0.1:43123/callback";
const VERIFIER: &str = "public-fixture-verifier-with-at-least-43-characters";
struct Fixture {
    dir: tempfile::TempDir,
    app: Router,
    session: SharedSession,
    identity: IdentityId,
    mcp: Mcp,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let owner = OwnerStore::open(dir.path()).unwrap();
        let identity = owner_auth::setup(&owner, "public fixture password only").unwrap();
        let clients = json!({"version":1,"clients":[{"client_id":"fixture","name":"Fixture harness","redirect_uris":[REDIRECT]}]});
        let path = dir.path().join("mcp-clients.json");
        fs::write(&path, clients.to_string()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let config = Configuration::open(&owner).unwrap().unwrap();
        let projects = Projects::open_with_root(&owner, None).unwrap();
        let mut writer = projects.directory.writer().unwrap();
        let mut catalog = devoption_projects::Catalog::default();
        catalog.create_project("Example project").unwrap();
        writer.save(&catalog).unwrap();
        drop(writer);
        let auth = OwnerAuthService::with_users(owner).unwrap();
        let session = Arc::new(SessionSlot::default());
        session
            .activate(
                Session::new(identity, [0xab; 32], MAX_LIFETIME)
                    .unwrap()
                    .with_csrf([0xcd; 32]),
                &[0xab; 32],
            )
            .unwrap();
        let mcp = Mcp {
            origin: PublicOrigin::parse(ISSUER).unwrap(),
            clients: Arc::new(config.clients),
            state: Arc::new(Mutex::new(Grants {
                codes: vec![],
                tokens: vec![],
                audit: config.audit,
            })),
            session: session.clone(),
            auth,
            projects,
        };
        let app = router(mcp.clone());
        Self {
            dir,
            app,
            session,
            identity,
            mcp,
        }
    }
    async fn send(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> (StatusCode, HeaderMap, Vec<u8>) {
        let mut b = Request::builder()
            .method(method)
            .uri(path)
            .header(HOST, "platform.example.test");
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        let (parts, body) = self
            .app
            .clone()
            .oneshot(b.body(Body::from(body.to_owned())).unwrap())
            .await
            .unwrap()
            .into_parts();
        assert!(!parts.headers.contains_key("access-control-allow-origin"));
        let bytes = axum::body::to_bytes(body, 1024 * 1024)
            .await
            .unwrap()
            .to_vec();
        (parts.status, parts.headers, bytes)
    }
    fn authorization(&self) -> String {
        let q = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("client_id", "fixture"),
                ("redirect_uri", REDIRECT),
                ("response_type", "code"),
                ("scope", SCOPE),
                ("resource", &format!("{ISSUER}/mcp")),
                ("code_challenge_method", "S256"),
                ("code_challenge", &URL_SAFE_NO_PAD.encode(digest(VERIFIER))),
                ("state", "fixture-state"),
            ])
            .finish();
        format!("/oauth/authorize?{q}")
    }
    async fn consent(&self, decision: &str) -> (StatusCode, Value) {
        let(status,_,body)=self.send("POST",&self.authorization(),&[(ORIGIN.as_str(),ISSUER),(COOKIE.as_str(),"__Host-devoption_session=abababababababababababababababababababababababababababababababab"),(browser_session::CSRF_HEADER,"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"),(CONTENT_TYPE.as_str(),"application/x-www-form-urlencoded")],&format!("decision={decision}")).await;
        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }
    async fn code(&self) -> String {
        let (status, reply) = self.consent("allow").await;
        assert_eq!(status, StatusCode::OK);
        let url = url::Url::parse(reply["redirect"].as_str().unwrap()).unwrap();
        assert_eq!(
            url.query_pairs().find(|(k, _)| k == "state").unwrap().1,
            "fixture-state"
        );
        url.query_pairs()
            .find(|(k, _)| k == "code")
            .unwrap()
            .1
            .into_owned()
    }
    async fn exchange(&self, code: &str, verifier: &str, resource: &str) -> (StatusCode, Value) {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("grant_type", "authorization_code"),
                ("client_id", "fixture"),
                ("redirect_uri", REDIRECT),
                ("code", code),
                ("code_verifier", verifier),
                ("resource", resource),
            ])
            .finish();
        let (status, _, bytes) = self
            .send(
                "POST",
                "/oauth/token",
                &[(CONTENT_TYPE.as_str(), "application/x-www-form-urlencoded")],
                &body,
            )
            .await;
        (status, serde_json::from_slice(&bytes).unwrap())
    }
    async fn token(&self) -> String {
        let code = self.code().await;
        let (status, reply) = self
            .exchange(&code, VERIFIER, &format!("{ISSUER}/mcp"))
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(reply["expires_in"], 900);
        reply["access_token"].as_str().unwrap().into()
    }
    async fn rpc(&self, token: Option<&str>, value: Value) -> (StatusCode, HeaderMap, Value) {
        let credential = format!("Bearer {}", token.unwrap_or_default());
        let mut headers = vec![
            (CONTENT_TYPE.as_str(), "application/json"),
            ("accept", "application/json, text/event-stream"),
            ("mcp-protocol-version", VERSION),
        ];
        if token.is_some() {
            headers.push((AUTHORIZATION.as_str(), &credential));
        }
        let (status, h, bytes) = self
            .send("POST", "/mcp", &headers, &value.to_string())
            .await;
        (
            status,
            h,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}
fn runtime(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(test);
}
#[test]
fn oauth_mcp_read_and_revoke_end_to_end() {
    runtime(async {
        let f = Fixture::new();
        let (status, headers, _) = f
            .rpc(None, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(
            headers["www-authenticate"]
                .to_str()
                .unwrap()
                .contains("/.well-known/oauth-protected-resource/mcp")
        );
        let (status, _, bytes) = f
            .send("GET", "/.well-known/oauth-authorization-server", &[], "")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["code_challenge_methods_supported"],
            json!(["S256"])
        );
        let token = f.token().await;
        let(_,_,init)=f.rpc(Some(&token),json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}})).await;
        assert_eq!(init["result"]["protocolVersion"], VERSION);
        let (_, _, list) = f
            .rpc(
                Some(&token),
                json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            )
            .await;
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 5);
        let(_,_,data)=f.rpc(Some(&token),json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_projects","arguments":{}}})).await;
        assert!(
            data["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Example project")
        );
        let(_,_,unknown)=f.rpc(Some(&token),json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"deploy","arguments":{}}})).await;
        assert_eq!(unknown["error"]["code"], -32602);
        let (status, _, _) = f
            .send(
                "POST",
                "/oauth/revoke",
                &[(CONTENT_TYPE.as_str(), "application/x-www-form-urlencoded")],
                &format!("client_id=fixture&token={token}"),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            f.rpc(
                Some(&token),
                json!({"jsonrpc":"2.0","id":5,"method":"ping"})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        let audit = fs::read_to_string(f.dir.path().join("mcp-audit.jsonl")).unwrap();
        assert!(
            audit.contains("list_projects")
                && audit.contains("revoked")
                && audit.contains(&hex(&f.identity.to_bytes()))
        );
        assert!(!audit.contains(&token) && !audit.contains(VERIFIER));
    });
}
#[test]
fn pkce_resource_redirect_consent_and_code_reuse_fail_closed() {
    runtime(async {
        let f = Fixture::new();
        let (_, denied) = f.consent("deny").await;
        assert!(
            denied["redirect"]
                .as_str()
                .unwrap()
                .contains("error=access_denied")
        );
        let code = f.code().await;
        assert_eq!(
            f.exchange(&code, VERIFIER, "https://other.example/mcp")
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.exchange(&code, &"x".repeat(43), &format!("{ISSUER}/mcp"))
                .await
                .1["error"],
            "invalid_grant"
        );
        assert_eq!(
            f.exchange(&code, VERIFIER, &format!("{ISSUER}/mcp"))
                .await
                .1["error"],
            "invalid_grant"
        );
        let path = f
            .authorization()
            .replace("127.0.0.1%3A43123", "127.0.0.1%3A9999");
        assert_eq!(
            f.send("GET", &path, &[], "").await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.send(
                "POST",
                &f.authorization(),
                &[
                    (ORIGIN.as_str(), ISSUER),
                    (CONTENT_TYPE.as_str(), "application/x-www-form-urlencoded")
                ],
                "decision=allow"
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        let token = f.token().await;
        // A bearer credential is never accepted as a browser session/consent.
        assert_eq!(
            f.send(
                "POST",
                &f.authorization(),
                &[
                    (ORIGIN.as_str(), ISSUER),
                    (AUTHORIZATION.as_str(), &format!("Bearer {token}"))
                ],
                ""
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    });
}
#[test]
fn hostile_transport_and_ambiguous_credentials_are_rejected() {
    runtime(async {
        let f = Fixture::new();
        let token = f.token().await;
        for header in [
            (ORIGIN.as_str(), "https://evil.example"),
            ("x-forwarded-host", "platform.example.test"),
            (HOST.as_str(), "evil.example"),
        ] {
            let (status, _, _) = f
                .send(
                    "GET",
                    "/.well-known/oauth-protected-resource/mcp",
                    &[header],
                    "",
                )
                .await;
            assert!(status.is_client_error());
        }
        let bearer = format!("Bearer {token}");
        for headers in [
            vec![
                (AUTHORIZATION.as_str(), bearer.as_str()),
                (COOKIE.as_str(), "other=value"),
            ],
            vec![
                (AUTHORIZATION.as_str(), bearer.as_str()),
                (AUTHORIZATION.as_str(), bearer.as_str()),
            ],
        ] {
            assert_eq!(
                f.send("GET", "/mcp", &headers, "").await.0,
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(
            f.send("GET", "/mcp", &[(AUTHORIZATION.as_str(), &bearer)], "")
                .await
                .0,
            StatusCode::METHOD_NOT_ALLOWED
        );
        // Browser logout doesn't convert a browser token into an MCP credential.
        f.session.revoke_identity(f.identity);
        assert_eq!(
            f.rpc(
                Some(&"ab".repeat(32)),
                json!({"jsonrpc":"2.0","id":1,"method":"ping"})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    });
}
#[test]
fn audit_failure_and_missing_projects_never_release_data() {
    runtime(async {
        let f = Fixture::new();
        let token = f.token().await;
        let(_,_,missing)=f.rpc(Some(&token),json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_repositories","arguments":{"project_id":"00000000000000000000000000000000"}}})).await;
        assert_eq!(missing["error"]["code"], -32602);
        // Test the audit sink itself rather than relying on an owner-store filename.
        let audit = f.dir.path().join("mcp-audit.jsonl");
        fs::set_permissions(audit, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            f.rpc(
                Some(&token),
                json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})
            )
            .await
            .0,
            StatusCode::SERVICE_UNAVAILABLE
        );
    });
}

#[test]
fn expiry_and_browser_revocation_are_enforced_without_renewal() {
    runtime(async {
        let f = Fixture::new();
        let code = f.code().await;
        f.mcp.state.lock().unwrap().codes[0].expires = Instant::now();
        assert_eq!(
            f.exchange(&code, VERIFIER, &format!("{ISSUER}/mcp"))
                .await
                .1["error"],
            "invalid_grant"
        );
        let token = f.token().await;
        f.mcp.state.lock().unwrap().tokens[0].expires = Instant::now();
        assert_eq!(
            f.rpc(
                Some(&token),
                json!({"jsonrpc":"2.0","id":1,"method":"ping"})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        let token = f.token().await;
        let grant = f
            .mcp
            .state
            .lock()
            .unwrap()
            .tokens
            .last()
            .unwrap()
            .id
            .clone();
        let(status,_,html)=f.send("GET","/mcp/grants",&[(COOKIE.as_str(),"__Host-devoption_session=abababababababababababababababababababababababababababababababab")],"").await;
        assert_eq!(status, StatusCode::OK);
        assert!(!String::from_utf8(html).unwrap().contains(&token));
        let(status,_,_)=f.send("POST","/mcp/grants",&[(ORIGIN.as_str(),ISSUER),(COOKIE.as_str(),"__Host-devoption_session=abababababababababababababababababababababababababababababababab"),(browser_session::CSRF_HEADER,"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"),(CONTENT_TYPE.as_str(),"application/x-www-form-urlencoded")],&format!("grant_id={grant}")).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(
            f.rpc(
                Some(&token),
                json!({"jsonrpc":"2.0","id":2,"method":"ping"})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    });
}

#[test]
fn disabled_members_lose_delegated_access_and_other_accounts_cannot_revoke() {
    runtime(async {
        let f = Fixture::new();
        let owner_token = f.token().await;
        let owner_grant = f.mcp.state.lock().unwrap().tokens[0].id.clone();
        let member = f
            .mcp
            .auth
            .accounts(false, |_, users| {
                let invitation = users.invite("member@example.test")?;
                users.enroll(
                    "member@example.test",
                    "public member fixture password",
                    &invitation,
                )?;
                Ok(IdentityId::from_bytes(users.load()?.users[0].id))
            })
            .await
            .unwrap();
        f.session.revoke_identity(f.identity);
        f.session
            .activate(
                Session::new(member, [0xab; 32], MAX_LIFETIME)
                    .unwrap()
                    .with_csrf([0xcd; 32]),
                &[0xab; 32],
            )
            .unwrap();
        let member_token = f.token().await;
        // The member's valid browser session cannot revoke another account's grant.
        f.send("POST","/mcp/grants",&[(ORIGIN.as_str(),ISSUER),(COOKIE.as_str(),"__Host-devoption_session=abababababababababababababababababababababababababababababababab"),(browser_session::CSRF_HEADER,"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"),(CONTENT_TYPE.as_str(),"application/x-www-form-urlencoded")],&format!("grant_id={owner_grant}")).await;
        assert_eq!(
            f.rpc(
                Some(&owner_token),
                json!({"jsonrpc":"2.0","id":1,"method":"ping"})
            )
            .await
            .0,
            StatusCode::OK
        );
        f.mcp
            .auth
            .accounts(false, |_, users| {
                let mut a = users.load()?;
                a.users[0].status = crate::users::Status::Disabled;
                users.save(&a)
            })
            .await
            .unwrap();
        assert_eq!(
            f.rpc(
                Some(&member_token),
                json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    });
}

#[test]
fn opt_in_and_unsafe_storage_fail_closed() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let owner = OwnerStore::open(dir.path()).unwrap();
    assert!(Configuration::open(&owner).unwrap().is_none());
    assert!(!dir.path().join("mcp-audit.jsonl").exists());
    let config = dir.path().join("mcp-clients.json");
    let target = dir.path().join("other.json");
    fs::write(&target, "{}").unwrap();
    symlink(&target, &config).unwrap();
    assert!(Configuration::open(&owner).is_err());
    fs::remove_file(&config).unwrap();
    fs::write(&config, "{}").unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Configuration::open(&owner).is_err());
}

/// Disposable browser proof only: a synthetic proxy in Playwright maps the fixture
/// HTTPS authority to this temporary loopback listener. No host TLS configuration.
#[test]
#[ignore = "manual disposable MCP browser fixture"]
fn manual_mcp_browser_fixture() {
    runtime(async {
        let fixture = Fixture::new();
        let app = crate::server::auth::routes(
            fixture.mcp.auth.clone(),
            crate::server::RequestOrigin::Public(PublicOrigin::parse(ISSUER).unwrap()),
            fixture.session.clone(),
        )
        .merge(fixture.app.clone());
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        if let Ok(path) = std::env::var("DEVOPTION_MCP_FIXTURE_INFO") {
            fs::write(path,json!({"address":format!("http://{address}"),"authorization":fixture.authorization()}).to_string()).unwrap();
        }
        println!("MCP browser fixture at http://{address}; synthetic authority {ISSUER}");
        let _ = tokio::time::timeout(
            Duration::from_secs(600),
            axum::serve(listener, app).into_future(),
        )
        .await;
        drop(fixture);
    });
}

#[test]
fn tools_page_and_scope_current_catalog_without_accepting_paths() {
    runtime(async {
        let f = Fixture::new();
        let project = {
            let mut store = f.mcp.projects.directory.writer().unwrap();
            let mut catalog = store.load().unwrap().unwrap();
            let project = catalog.projects()[0].id();
            for i in 0..55 {
                catalog.create_project(&format!("Project {i}")).unwrap();
            }
            let attached = catalog.create_repository("Attached repository").unwrap();
            catalog.create_repository("Unattached repository").unwrap();
            catalog.associate(project, attached).unwrap();
            store.save(&catalog).unwrap();
            project
        };
        let first = tools::call(
            &f.mcp.projects,
            &json!({"name":"list_projects","arguments":{}}),
        )
        .await
        .unwrap();
        assert_eq!(first["items"].as_array().unwrap().len(), 50);
        assert_eq!(first["next_offset"], 50);
        let next = tools::call(
            &f.mcp.projects,
            &json!({"name":"list_projects","arguments":{"offset":50}}),
        )
        .await
        .unwrap();
        assert_eq!(next["items"].as_array().unwrap().len(), 6);
        assert!(next["next_offset"].is_null());
        let repos=tools::call(&f.mcp.projects,&json!({"name":"list_repositories","arguments":{"project_id":hex(&project.to_bytes())}})).await.unwrap();
        assert_eq!(repos["items"].as_array().unwrap().len(), 1);
        assert_eq!(repos["items"][0]["name"], "Attached repository");
        let builds = tools::call(
            &f.mcp.projects,
            &json!({"name":"list_builds","arguments":{"project_id":hex(&project.to_bytes())}}),
        )
        .await
        .unwrap();
        assert_eq!(builds["availability"], "unconfigured");
        assert!(tools::call(&f.mcp.projects,&json!({"name":"list_repositories","arguments":{"project_id":hex(&project.to_bytes()),"path":"/tmp"}})).await.is_err());
        let status=tools::call(&f.mcp.projects,&json!({"name":"get_project_status","arguments":{"project_id":hex(&project.to_bytes())}})).await.unwrap();
        assert_eq!(status["runtime"], "Not configured");
        assert_eq!(status["history_available"], false);
    });
}
