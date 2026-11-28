use super::*;
use axum::{
    body::{Body, to_bytes},
    http::header::{COOKIE as COOKIE_HEADER, ORIGIN},
};
use saml_rs::{
    Attribute, AttributeValue, Attributes, AuthnRequest, CertificatePem, Credentials, EntityId,
    Idp, IdpConfig, IdpValidationPolicy, MetadataTrustPolicy, NameId, PrivateKeyPem, RespondSso,
    Saml, SpDescriptor, SsoEndpoint, Subject,
};
use std::os::unix::fs::PermissionsExt;
use tower::ServiceExt;

const ORIGIN_URL: &str = "https://platform.example.test";
const PASSWORD: &str = "public saml browser fixture only";
const IDP: &str = "https://idp.example.test/metadata";
struct Fixture {
    directory: tempfile::TempDir,
    auth: OwnerAuthService,
    slot: SharedSession,
    app: Router,
    owner: HeaderMap,
    idp: Saml<Idp>,
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let store = crate::owner_store::OwnerStore::open(directory.path()).unwrap();
        let auth = OwnerAuthService::new(store);
        runtime().block_on(auth.setup(PASSWORD.into())).unwrap();
        drop(auth);
        let auth = OwnerAuthService::with_users(
            crate::owner_store::OwnerStore::open(directory.path()).unwrap(),
        )
        .unwrap();
        let slot = Arc::new(crate::session::slot::SessionSlot::default());
        let owner = runtime().block_on(async {
            browser_session::activate(auth.sign_in(PASSWORD.into()).await.unwrap().unwrap(), &slot)
                .unwrap()
                .headers()
                .clone()
        });
        let app = super::super::routes(
            auth.clone(),
            super::super::super::RequestOrigin::Public(
                super::super::super::PublicOrigin::parse(ORIGIN_URL).unwrap(),
            ),
            slot.clone(),
        );
        let idp = Saml::idp(
            IdpConfig::builder(EntityId::try_new(IDP).unwrap())
                .sso_endpoint(SsoEndpoint::redirect("https://idp.example.test/sso").unwrap())
                .credentials(Credentials {
                    signing_key: Some(PrivateKeyPem::new(include_str!(
                        "fixtures/public-test-key.pem"
                    ))),
                    signing_certificate: Some(CertificatePem::new(include_str!(
                        "fixtures/certificate.pem"
                    ))),
                    ..Credentials::default()
                })
                .validation(IdpValidationPolicy::recommended())
                .build()
                .unwrap(),
        )
        .unwrap();
        Self {
            directory,
            auth,
            slot,
            app,
            owner,
            idp,
        }
    }
    fn request(
        &self,
        method: &str,
        path: &str,
        credentials: Option<&HeaderMap>,
        origin: &str,
        media: &str,
        body: &str,
    ) -> (StatusCode, HeaderMap, String) {
        let mut builder = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header("host", "platform.example.test")
            .header(ORIGIN, origin)
            .header(CONTENT_TYPE, media);
        if let Some(h) = credentials {
            if let Some(cookie) = h.get(SET_COOKIE) {
                builder = builder.header(
                    COOKIE_HEADER,
                    cookie.to_str().unwrap().split(';').next().unwrap(),
                );
            }
            if let Some(csrf) = h.get(browser_session::CSRF_HEADER) {
                builder = builder.header(browser_session::CSRF_HEADER, csrf);
            }
        }
        runtime().block_on(async {
            let response = self
                .app
                .clone()
                .oneshot(builder.body(Body::from(body.to_owned())).unwrap())
                .await
                .unwrap();
            let status = response.status();
            let headers = response.headers().clone();
            (
                status,
                headers,
                String::from_utf8(
                    to_bytes(response.into_body(), 512 * 1024)
                        .await
                        .unwrap()
                        .to_vec(),
                )
                .unwrap(),
            )
        })
    }
    fn save(
        &self,
        credentials: &HeaderMap,
        revision: u64,
        local_auth: bool,
        group: &str,
    ) -> StatusCode {
        self.request("POST", "/auth/saml/settings", Some(credentials), ORIGIN_URL, "text/plain", &json!({"action":"save", "revision":revision, "enabled":true, "local_auth":local_auth, "idp_entity_id": IDP, "idp_metadata": self.idp.metadata_xml(), "group_attribute":if group.is_empty() {""} else {"groups"}, "required_group":group}).to_string()).0
    }
    fn begin(&self) -> (HeaderMap, String) {
        let (status, preauth, _) = self.request(
            "POST",
            "/auth/challenge",
            None,
            ORIGIN_URL,
            "text/plain",
            "",
        );
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, headers, body) = self.request(
            "POST",
            "/auth/saml/start",
            Some(&preauth),
            ORIGIN_URL,
            "text/plain",
            "",
        );
        assert_eq!(status, StatusCode::OK, "{body}");
        let url = url::Url::parse(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["redirect"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        (headers, url.query().unwrap().to_owned())
    }
    fn response(&self, query: &str, subject: &str, group: &str) -> String {
        let sp = core::sp(ORIGIN_URL).unwrap();
        let sp = SpDescriptor::from_metadata_xml_for(
            EntityId::try_new(format!("{ORIGIN_URL}/auth/saml/metadata")).unwrap(),
            sp.metadata_xml(),
            MetadataTrustPolicy::UnsignedForCompatibility,
        )
        .unwrap();
        let request = self
            .idp
            .receive_sso(
                &sp,
                BrowserInput::<AuthnRequest>::redirect(query),
                SamlValidationContext::new(
                    SystemTime::now(),
                    ReplayPolicy::DisabledForCompatibility,
                ),
            )
            .unwrap();
        let response = self
            .idp
            .respond_sso(
                &sp,
                &request,
                Subject::new(NameId::persistent(subject).unwrap(), Vec::new()),
                RespondSso::post()
                    .sign_response()
                    .attributes(Attributes::new(vec![Attribute::new(
                        "groups",
                        None,
                        vec![AttributeValue::new(group)],
                    )])),
            )
            .unwrap();
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        for field in response.post_form().unwrap().fields() {
            form.append_pair(field.name(), field.value());
        }
        form.finish()
    }
    fn callback(&self, flow: Option<&HeaderMap>, body: &str) -> (StatusCode, HeaderMap, String) {
        self.request(
            "POST",
            "/auth/saml/acs",
            flow,
            "https://idp.example.test",
            "application/x-www-form-urlencoded",
            body,
        )
    }
}
#[test]
fn signed_browser_flow_has_independent_administrators_and_verified_local_auth_control() {
    let f = Fixture::new();
    assert_eq!(f.save(&f.owner, 0, false, ""), StatusCode::CONFLICT);
    assert_eq!(
        f.save(&f.owner, 0, true, "operators"),
        StatusCode::NO_CONTENT
    );
    let (flow, query) = f.begin();
    let body = f.response(&query, "employee-001", "operators");
    let (status, first, _) = f.callback(Some(&flow), &body);
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(f.callback(Some(&flow), &body).0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        f.request(
            "GET",
            "/auth/account",
            Some(&first),
            ORIGIN_URL,
            "text/plain",
            ""
        )
        .2,
        "{\"role\":\"Administrator\"}"
    );
    let (flow, query) = f.begin();
    let body = f.response(&query, "employee-002", "operators");
    let (_, second, _) = f.callback(Some(&flow), &body);
    let first_id = browser_session::authenticate_read(&cookie_headers(&first), &f.slot).unwrap();
    assert!(
        runtime()
            .block_on(f.auth.mcp_access(first_id, &f.slot))
            .unwrap()
    );
    assert!(browser_session::authenticate_read(&cookie_headers(&second), &f.slot).is_some());
    assert_eq!(
        f.save(&first, 1, false, "operators"),
        StatusCode::NO_CONTENT
    );
    assert!(browser_session::authenticate_read(&cookie_headers(&second), &f.slot).is_none());
    assert!(
        !runtime()
            .block_on(f.auth.mcp_access(first_id, &f.slot))
            .unwrap()
    );
    assert!(
        runtime()
            .block_on(f.auth.sign_in(PASSWORD.into()))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        f.request(
            "POST",
            "/auth/register",
            None,
            ORIGIN_URL,
            "text/plain",
            "{}"
        )
        .0,
        StatusCode::FORBIDDEN
    );
    let store = f.auth.saml().unwrap();
    assert!(!store.local_auth());
    assert!(
        !crate::saml::Store::open(
            &crate::owner_store::OwnerStore::open(f.directory.path()).unwrap()
        )
        .unwrap()
        .local_auth()
    );
    assert_eq!(f.save(&f.owner, 1, true, "operators"), StatusCode::CONFLICT); // Stale edit.
    assert_eq!(
        f.save(&f.owner, 2, true, "operators"),
        StatusCode::NO_CONTENT
    );
    assert!(
        runtime()
            .block_on(f.auth.sign_in(PASSWORD.into()))
            .unwrap()
            .is_some()
    );
}
fn cookie_headers(issued: &HeaderMap) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE_HEADER,
        issued[SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .parse()
            .unwrap(),
    );
    headers
}
#[test]
fn callback_rejects_missing_or_foreign_browser_binding_wrong_groups_tampering_and_unsolicited_posts()
 {
    let f = Fixture::new();
    assert_eq!(
        f.save(&f.owner, 0, true, "operators"),
        StatusCode::NO_CONTENT
    );
    let (flow, query) = f.begin();
    let body = f.response(&query, "employee-001", "operators");
    assert_eq!(f.callback(None, &body).0, StatusCode::UNAUTHORIZED);
    let mut wrong = flow.clone();
    wrong.insert(
        SET_COOKIE,
        format!("{COOKIE}={}", "0".repeat(64)).parse().unwrap(),
    );
    assert_eq!(f.callback(Some(&wrong), &body).0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        f.callback(Some(&flow), &(body.clone() + "&RelayState=duplicate"))
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(f.callback(Some(&flow), &body).0, StatusCode::SEE_OTHER);
    let (flow, query) = f.begin();
    let body = f.response(&query, "employee-003", "other-group");
    assert_eq!(f.callback(Some(&flow), &body).0, StatusCode::UNAUTHORIZED);
    let (flow, query) = f.begin();
    let body = f.response(&query, "employee-003", "operators");
    let mut pairs: Vec<(String, String)> = url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect();
    use base64::Engine as _;
    let encoded = &mut pairs.iter_mut().find(|p| p.0 == "SAMLResponse").unwrap().1;
    let xml = String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(&*encoded)
            .unwrap(),
    )
    .unwrap();
    *encoded = base64::engine::general_purpose::STANDARD
        .encode(xml.replace("employee-003", "employee-999"));
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    form.extend_pairs(pairs);
    assert_eq!(
        f.callback(Some(&flow), &form.finish()).0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(f.callback(Some(&flow), &body).0, StatusCode::UNAUTHORIZED); // Invalid attempt consumed it.
}

fn change_attribute(xml: &str, name: &str, value: &str) -> String {
    let pattern = format!("{name}=\"");
    let mut rest = xml;
    let mut result = String::new();
    let mut changed = false;
    while let Some(index) = rest.find(&pattern) {
        let start = index + pattern.len();
        let end = rest[start..].find('"').unwrap() + start;
        result.push_str(&rest[..start]);
        result.push_str(value);
        rest = &rest[end..];
        changed = true;
    }
    assert!(changed, "missing fixture attribute {name}");
    result.push_str(rest);
    result
}
fn signed_variant(body: &str, variant: &str) -> String {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let mut pairs: Vec<(String, String)> = url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect();
    let encoded = &mut pairs.iter_mut().find(|p| p.0 == "SAMLResponse").unwrap().1;
    let mut xml = String::from_utf8(STANDARD.decode(&*encoded).unwrap()).unwrap();
    while let Some(start) = xml.find("<ds:Signature") {
        let end = xml[start..].find("</ds:Signature>").unwrap() + start + "</ds:Signature>".len();
        xml.replace_range(start..end, "");
    }
    xml = match variant {
        "audience" => xml.replace(
            &format!("{ORIGIN_URL}/auth/saml/metadata"),
            "https://other.example.test/metadata",
        ),
        "issuer" => xml.replace(IDP, "https://other.example.test/idp"),
        "destination" => change_attribute(&xml, "Destination", "https://other.example.test/acs"),
        "recipient" => change_attribute(&xml, "Recipient", "https://other.example.test/acs"),
        "correlation" => change_attribute(&xml, "InResponseTo", "_different-request"),
        "expired" => change_attribute(&xml, "NotOnOrAfter", "2000-01-01T00:00:00Z"),
        "future" => change_attribute(&xml, "NotBefore", "2099-01-01T00:00:00Z"),
        "unstable-name" => xml.replace(
            "urn:oasis:names:tc:SAML:2.0:nameid-format:persistent",
            "urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress",
        ),
        "unsigned" => xml,
        _ => panic!("unknown variant"),
    };
    if variant != "unsigned" {
        let key = saml_rs::crypto::keys::load_private_key(
            include_str!("fixtures/public-test-key.pem"),
            None,
        )
        .unwrap();
        xml = saml_rs::crypto::construct_saml_signature(
            &xml,
            true,
            &key,
            include_str!("fixtures/certificate.pem"),
            saml_rs::constants::signature_algorithm::RSA_SHA256,
            &[],
            None,
        )
        .unwrap();
    }
    *encoded = STANDARD.encode(xml);
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    form.extend_pairs(pairs);
    form.finish()
}
#[test]
fn signed_protocol_failures_never_create_a_session_or_verify_configuration() {
    for variant in [
        "audience",
        "issuer",
        "destination",
        "recipient",
        "correlation",
        "expired",
        "future",
        "unstable-name",
        "unsigned",
    ] {
        let f = Fixture::new();
        assert_eq!(f.save(&f.owner, 0, true, ""), StatusCode::NO_CONTENT);
        let (flow, query) = f.begin();
        let body = f.response(&query, "employee-001", "");
        let (status, headers, _) = f.callback(Some(&flow), &signed_variant(&body, variant));
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{variant}");
        assert!(!headers.contains_key(SET_COOKIE));
        assert!(
            !f.auth
                .saml()
                .unwrap()
                .state
                .lock()
                .unwrap()
                .configuration
                .verified
        );
        assert_eq!(f.save(&f.owner, 1, false, ""), StatusCode::CONFLICT);
    }
}
#[test]
fn settings_and_start_enforce_origin_csrf_and_administrator_boundaries() {
    let f = Fixture::new();
    assert_eq!(
        f.request(
            "GET",
            "/auth/saml/settings",
            None,
            ORIGIN_URL,
            "text/plain",
            ""
        )
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.request(
            "POST",
            "/auth/saml/settings",
            Some(&f.owner),
            "https://other.example.test",
            "text/plain",
            "{}"
        )
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.request(
            "POST",
            "/auth/saml/start",
            None,
            ORIGIN_URL,
            "text/plain",
            ""
        )
        .0,
        StatusCode::FORBIDDEN
    );
    let member = browser_session::activate(
        crate::owner_auth::service::issue(crate::identity::IdentityId::from_bytes([19; 16]), |b| {
            getrandom::fill(b).unwrap();
            Ok(())
        })
        .unwrap(),
        &f.slot,
    )
    .unwrap()
    .headers()
    .clone();
    assert_eq!(
        f.request(
            "GET",
            "/auth/saml/settings",
            Some(&member),
            ORIGIN_URL,
            "text/plain",
            ""
        )
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(f.save(&member, 0, true, ""), StatusCode::FORBIDDEN);
    assert_eq!(f.save(&f.owner, 0, true, ""), StatusCode::NO_CONTENT);
    let (flow, query) = f.begin();
    let body = f.response(&query, "employee-001", "");
    f.auth.saml().unwrap().state.lock().unwrap().flows[0].expires = Instant::now();
    assert_eq!(f.callback(Some(&flow), &body).0, StatusCode::UNAUTHORIZED);
    let (flow, query) = f.begin();
    let body = f.response(&query, "employee-001", "");
    assert_eq!(f.save(&f.owner, 1, true, ""), StatusCode::NO_CONTENT);
    assert_eq!(f.callback(Some(&flow), &body).0, StatusCode::UNAUTHORIZED);
}

/// Synthetic IdP and real platform handlers; no Google, production data or credentials.
#[test]
#[ignore = "manual disposable browser fixture; see docs/saml.md"]
fn manual_saml_browser_fixture() {
    let f = Arc::new(Fixture::new());
    assert_eq!(
        f.save(&f.owner, 0, true, "operators"),
        StatusCode::NO_CONTENT
    );
    let idp_fixture = f.clone();
    let app = f.app.clone()
        .route("/sign-in", get(|| async { axum::response::Html(devoption_ui::render_with_account(devoption_ui::Page::SignIn, true)) }))
        .route("/", get(|| async { (StatusCode::SEE_OTHER, [(LOCATION, "/settings/users")]) }))
        .route("/sso", get(move |axum::extract::RawQuery(query): axum::extract::RawQuery| {
            let f = idp_fixture.clone();
            async move {
                let body = f.response(&query.unwrap(), "public-fixture-employee", "operators");
                let mut html = format!("<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Synthetic identity provider</title><form action=\"{ORIGIN_URL}/auth/saml/acs\" method=\"post\">");
                for (name, value) in url::form_urlencoded::parse(body.as_bytes()) { html.push_str(&format!("<input type=\"hidden\" name=\"{name}\" value=\"{value}\">")); }
                html.push_str("<button>Continue synthetic sign-in</button></form></html>");
                axum::response::Html(html)
            }
        }));
    runtime().block_on(async {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        println!("Synthetic SAML fixture: 127.0.0.1:{port}; proxy platform.example.test and idp.example.test to this address; expires after 600 seconds");
        axum::serve(listener, app).with_graceful_shutdown(async { tokio::time::sleep(std::time::Duration::from_secs(600)).await }).await.unwrap();
    });
}
