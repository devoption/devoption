//! Pre-registered public clients, exact redirects, S256 PKCE, explicit consent.
use super::*;
fn form(raw: &[u8]) -> Result<BTreeMap<String, String>, &'static str> {
    if raw.len() > 8192 {
        return Err("invalid_request");
    }
    let mut map = BTreeMap::new();
    for (k, v) in url::form_urlencoded::parse(raw) {
        if map.insert(k.into_owned(), v.into_owned()).is_some() {
            return Err("invalid_request");
        }
    }
    Ok(map)
}
fn fields(map: &BTreeMap<String, String>, allowed: &[&str]) -> bool {
    map.keys().all(|k| allowed.contains(&k.as_str()))
}
fn field<'a>(map: &'a BTreeMap<String, String>, k: &str) -> &'a str {
    map.get(k).map(String::as_str).unwrap_or_default()
}
struct Authorization {
    client: String,
    redirect: String,
    challenge: String,
    state: String,
}
fn authorization(s: &Mcp, raw: &[u8]) -> Result<Authorization, &'static str> {
    let p = form(raw)?;
    let id = field(&p, "client_id");
    let redirect = field(&p, "redirect_uri");
    let challenge = field(&p, "code_challenge");
    let state = field(&p, "state");
    if !fields(
        &p,
        &[
            "client_id",
            "redirect_uri",
            "response_type",
            "scope",
            "resource",
            "code_challenge",
            "code_challenge_method",
            "state",
        ],
    ) || !s
        .client(id)
        .is_some_and(|c| c.redirect_uris.iter().any(|r| r == redirect))
        || field(&p, "response_type") != "code"
        || field(&p, "scope") != SCOPE
        || field(&p, "resource") != s.resource()
        || field(&p, "code_challenge_method") != "S256"
        || URL_SAFE_NO_PAD
            .decode(challenge)
            .ok()
            .is_none_or(|b| b.len() != 32)
        || state.is_empty()
        || state.len() > 512
        || state.chars().any(char::is_control)
    {
        return Err("invalid_request");
    }
    Ok(Authorization {
        client: id.into(),
        redirect: redirect.into(),
        challenge: challenge.into(),
        state: state.into(),
    })
}
async fn browser(s: &Mcp, headers: &HeaderMap, mutation: bool) -> Result<IdentityId, StatusCode> {
    let identity = if mutation {
        browser_session::authenticate(headers, &s.session)
    } else {
        browser_session::authenticate_read(headers, &s.session)
    }
    .ok_or(StatusCode::UNAUTHORIZED)?;
    match s.auth.mcp_access(identity).await {
        Ok(true) => Ok(identity),
        Ok(false) => Err(StatusCode::FORBIDDEN),
        Err(_) => Err(StatusCode::SERVICE_UNAVAILABLE),
    }
}
pub(super) async fn consent(
    State(s): State<Mcp>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let a = match authorization(&s, query.as_deref().unwrap_or_default().as_bytes()) {
        Ok(a) => a,
        Err(e) => return error(e),
    };
    if browser(&s, &headers, false).await.is_err() {
        return Html(devoption_ui::render_mcp_consent(None)).into_response();
    }
    Html(devoption_ui::render_mcp_consent(Some(
        &s.client(&a.client).expect("validated client").name,
    )))
    .into_response()
}
pub(super) async fn authorize(
    State(s): State<Mcp>,
    RawQuery(query): RawQuery,
    request: Request,
) -> Response {
    // Explicit Origin and browser synchronizer CSRF are mandatory, even though other
    // OAuth endpoints accept native requests without Origin.
    if one(request.headers(), "origin") != Some(s.issuer()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let identity = match browser(&s, request.headers(), true).await {
        Ok(i) => i,
        Err(e) => return e.into_response(),
    };
    let a = match authorization(&s, query.as_deref().unwrap_or_default().as_bytes()) {
        Ok(a) => a,
        Err(e) => return error(e),
    };
    let raw = match body(request, "application/x-www-form-urlencoded").await {
        Ok(b) => b,
        Err(e) => return e.into_response(),
    };
    let p = match form(&raw) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    if !fields(&p, &["decision"]) || !matches!(field(&p, "decision"), "allow" | "deny") {
        return error("invalid_request");
    }
    let mut redirect = url::Url::parse(&a.redirect).expect("validated redirect");
    if field(&p, "decision") == "deny" {
        if s.record(Some(identity), &a.client, "authorize", "denied")
            .is_err()
        {
            return unavailable();
        }
        redirect
            .query_pairs_mut()
            .append_pair("error", "access_denied");
    } else {
        let code = match random() {
            Ok(v) => v,
            Err(e) => return e.into_response(),
        };
        let mut state = match s.state.lock() {
            Ok(v) => v,
            Err(_) => return unavailable(),
        };
        state.codes.retain(|c| c.expires > Instant::now());
        if state.codes.len() >= 256 {
            return unavailable();
        }
        if audit(
            &mut state.audit,
            Some(identity),
            &a.client,
            "authorize",
            "approved",
        )
        .is_err()
        {
            return unavailable();
        }
        state.codes.push(Code {
            hash: digest(&code),
            identity,
            client: a.client,
            redirect: a.redirect,
            challenge: a.challenge,
            expires: Instant::now() + Duration::from_secs(60),
        });
        redirect.query_pairs_mut().append_pair("code", &code);
    }
    redirect
        .query_pairs_mut()
        .append_pair("state", &a.state)
        .append_pair("iss", s.issuer());
    // Fetch follows no callback automatically; the browser navigates only after
    // receiving this validated, pre-registered redirect in a protected response.
    ok(json!({"redirect":redirect.as_str()}))
}
pub(super) async fn token(State(s): State<Mcp>, request: Request) -> Response {
    if request.headers().contains_key(COOKIE) || request.headers().contains_key(AUTHORIZATION) {
        return error("invalid_request");
    }
    let raw = match body(request, "application/x-www-form-urlencoded").await {
        Ok(b) => b,
        Err(e) => return e.into_response(),
    };
    let p = match form(&raw) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    let client = field(&p, "client_id");
    let verifier = field(&p, "code_verifier");
    if !fields(
        &p,
        &[
            "grant_type",
            "client_id",
            "code",
            "redirect_uri",
            "code_verifier",
            "resource",
        ],
    ) || field(&p, "grant_type") != "authorization_code"
        || s.client(client).is_none()
        || field(&p, "resource") != s.resource()
        || !(43..=128).contains(&verifier.len())
        || !verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
    {
        return error("invalid_request");
    }
    let code = {
        let mut grants = match s.state.lock() {
            Ok(v) => v,
            Err(_) => return unavailable(),
        };
        let hash = digest(field(&p, "code"));
        let Some(index) = grants.codes.iter().position(|c| c.hash == hash) else {
            return error("invalid_grant");
        };
        // Consume even on invalid PKCE, redirect or expiry; never allow code reuse.
        grants.codes.remove(index)
    };
    if code.expires <= Instant::now()
        || code.client != client
        || code.redirect != field(&p, "redirect_uri")
        || code.challenge != URL_SAFE_NO_PAD.encode(digest(verifier))
    {
        return error("invalid_grant");
    }
    if !matches!(s.auth.mcp_access(code.identity).await, Ok(true)) {
        return error("invalid_grant");
    }
    let value = match random() {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let id = match random() {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let mut grants = match s.state.lock() {
        Ok(v) => v,
        Err(_) => return unavailable(),
    };
    grants.tokens.retain(|t| t.expires > Instant::now());
    if grants.tokens.len() >= 256 {
        return unavailable();
    }
    if audit(
        &mut grants.audit,
        Some(code.identity),
        client,
        "token",
        "issued",
    )
    .is_err()
    {
        return unavailable();
    }
    grants.tokens.push(Token {
        hash: digest(&value),
        id,
        identity: code.identity,
        client: client.into(),
        expires: Instant::now() + TOKEN_TTL,
    });
    ok(
        json!({"access_token":value,"token_type":"Bearer","expires_in":TOKEN_TTL.as_secs(),"scope":SCOPE}),
    )
}
pub(super) async fn revoke(State(s): State<Mcp>, request: Request) -> Response {
    if request.headers().contains_key(COOKIE) || request.headers().contains_key(AUTHORIZATION) {
        return error("invalid_request");
    }
    let raw = match body(request, "application/x-www-form-urlencoded").await {
        Ok(b) => b,
        Err(e) => return e.into_response(),
    };
    let p = match form(&raw) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    if !fields(&p, &["client_id", "token", "token_type_hint"])
        || s.client(field(&p, "client_id")).is_none()
        || field(&p, "token").is_empty()
    {
        return error("invalid_request");
    }
    let mut state = match s.state.lock() {
        Ok(v) => v,
        Err(_) => return unavailable(),
    };
    let hash = digest(field(&p, "token"));
    if let Some(i) = state
        .tokens
        .iter()
        .position(|t| t.hash == hash && t.client == field(&p, "client_id"))
    {
        let t = state.tokens.remove(i);
        if audit(
            &mut state.audit,
            Some(t.identity),
            &t.client,
            "revoke",
            "revoked",
        )
        .is_err()
        {
            return unavailable();
        }
    }
    StatusCode::OK.into_response()
}
pub(super) async fn grants(State(s): State<Mcp>, headers: HeaderMap) -> Response {
    let identity = match browser(&s, &headers, false).await {
        Ok(i) => i,
        Err(e) => return e.into_response(),
    };
    let state = match s.state.lock() {
        Ok(v) => v,
        Err(_) => return unavailable(),
    };
    let grants = state
        .tokens
        .iter()
        .filter(|t| t.identity == identity && t.expires > Instant::now())
        .map(|t| {
            (
                t.id.clone(),
                s.client(&t.client).expect("registered client").name.clone(),
            )
        })
        .collect::<Vec<_>>();
    Html(devoption_ui::render_mcp_grants(&grants)).into_response()
}
pub(super) async fn revoke_grant(State(s): State<Mcp>, request: Request) -> Response {
    if one(request.headers(), "origin") != Some(s.issuer()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let identity = match browser(&s, request.headers(), true).await {
        Ok(i) => i,
        Err(e) => return e.into_response(),
    };
    let raw = match body(request, "application/x-www-form-urlencoded").await {
        Ok(b) => b,
        Err(e) => return e.into_response(),
    };
    let p = match form(&raw) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    if !fields(&p, &["grant_id"]) {
        return error("invalid_request");
    }
    let mut state = match s.state.lock() {
        Ok(v) => v,
        Err(_) => return unavailable(),
    };
    if let Some(i) = state
        .tokens
        .iter()
        .position(|t| t.id == field(&p, "grant_id") && t.identity == identity)
    {
        let t = state.tokens.remove(i);
        if audit(
            &mut state.audit,
            Some(identity),
            &t.client,
            "revoke",
            "revoked",
        )
        .is_err()
        {
            return unavailable();
        }
    }
    StatusCode::NO_CONTENT.into_response()
}
