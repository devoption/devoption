//! Browser SAML adapter. Only ACS accepts a cross-site POST; it requires a live,
//! one-use request, independent browser binding, pinned signature and exact recipient.
use super::*;
use crate::saml::{self as core, Configuration, Flow, invalid};
use axum::{
    http::header::{CONTENT_TYPE, LOCATION, SET_COOKIE},
    routing::get,
};
use saml_rs::{
    BrowserInput, ClockSkew, FormField, NameIdFormat, RelayStateParam, ReplayPolicy,
    SamlValidationContext, SsoResponse, StartSso,
};
use serde::Deserialize;
use serde_json::json;
use std::time::{Instant, SystemTime};
use subtle::ConstantTimeEq;

const COOKIE: &str = "__Host-devoption_saml";
const COOKIE_ATTRIBUTES: &str = "Path=/; Secure; HttpOnly; SameSite=None";

#[cfg(test)]
mod tests;

#[derive(Clone)]
struct SamlState {
    auth: AuthState,
    origin: Option<String>,
}
pub(super) fn routes() -> Router<AuthState> {
    Router::new().route("/auth/methods", get(methods))
}
pub(super) fn configured_routes(state: AuthState, origin: super::super::RequestOrigin) -> Router {
    let public = match &origin {
        super::super::RequestOrigin::Public(o) => Some(o.as_str().to_owned()),
        _ => None,
    };
    let saml_state = SamlState {
        auth: state,
        origin: public,
    };
    let normal = Router::new()
        .route("/auth/saml/settings", get(settings).post(save))
        .route("/auth/saml/start", post(start))
        .route("/auth/saml/metadata", get(metadata))
        .with_state(saml_state.clone());
    let callback_origin = origin.clone();
    let callback = Router::new()
        .route("/auth/saml/acs", post(acs))
        .with_state(saml_state)
        .layer(middleware::from_fn(
            move |request: Request, next: axum::middleware::Next| {
                let allowed = callback_origin.check_saml_callback(&request);
                async move {
                    match allowed {
                        Ok(()) => next.run(request).await,
                        Err(status) => browser_session::empty(status),
                    }
                }
            },
        ));
    protect_origin(normal, origin)
        .merge(callback)
        .layer(middleware::map_response(
            |mut response: Response| async move {
                response
                    .headers_mut()
                    .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
                response
                    .headers_mut()
                    .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
                response
            },
        ))
}
async fn methods(State(state): State<AuthState>) -> Response {
    match state.auth.saml() {
        Some(saml) => match saml.state.try_lock() {
            Ok(s) => accounts::json_response(
                json!({"saml": s.configuration.enabled, "local_auth": s.configuration.local_auth}),
            ),
            Err(_) => browser_session::empty(StatusCode::SERVICE_UNAVAILABLE),
        },
        None => accounts::json_response(json!({"saml": false, "local_auth": true})),
    }
}
async fn administrator(
    state: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<(), StatusCode> {
    let id = if mutation {
        browser_session::authenticate(headers, &state.session)
    } else {
        browser_session::authenticate_read(headers, &state.session)
    };
    let Some(id) = id else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    match accounts::is_owner(state, id).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(StatusCode::FORBIDDEN),
        Err(_) => Err(StatusCode::SERVICE_UNAVAILABLE),
    }
}
async fn settings(State(state): State<SamlState>, headers: HeaderMap) -> Response {
    if let Err(response) = administrator(&state.auth, &headers, false).await {
        return browser_session::empty(response);
    }
    let Some(store) = state.auth.auth.saml() else {
        return browser_session::empty(StatusCode::SERVICE_UNAVAILABLE);
    };
    let Ok(s) = store.state.try_lock() else {
        return browser_session::empty(StatusCode::SERVICE_UNAVAILABLE);
    };
    let mut configuration = s.configuration.clone();
    configuration.verified = configuration.verified_for(state.origin.as_deref());
    accounts::json_response(
        json!({"configuration": configuration, "origin": state.origin, "session_minutes": 15}),
    )
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
enum Command {
    Save {
        revision: u64,
        enabled: bool,
        local_auth: bool,
        idp_entity_id: String,
        idp_metadata: String,
        group_attribute: String,
        required_group: String,
    },
    Revoke,
}
async fn save(State(state): State<SamlState>, request: Request) -> Response {
    if let Err(response) = administrator(&state.auth, request.headers(), true).await {
        return browser_session::empty(response);
    }
    let Some(store) = state.auth.auth.saml() else {
        return browser_session::empty(StatusCode::SERVICE_UNAVAILABLE);
    };
    let request_headers = request.headers().clone();
    let body = match super::super::input::text_body(request, 100_000).await {
        Ok(body) => body,
        Err(s) => return browser_session::empty(s),
    };
    let command: Command = match serde_json::from_str(&body) {
        Ok(c) => c,
        Err(_) => return browser_session::empty(StatusCode::BAD_REQUEST),
    };
    let headers = request_headers;
    let slot = state.auth.session.clone();
    match state
        .auth
        .auth
        .accounts(false, move |owner, _| {
            let Some(id) = browser_session::authenticate(&headers, &slot) else {
                return Ok(false);
            };
            if !slot.saml_administrator(id)
                && !owner
                    .load()
                    .map_err(|_| invalid())?
                    .is_some_and(|r| r.identity() == id)
            {
                return Ok(false);
            }
            let mut s = store.state.lock().map_err(|_| invalid())?;
            if let Command::Save {
                revision,
                enabled,
                local_auth,
                idp_entity_id,
                idp_metadata,
                group_attribute,
                required_group,
            } = command
            {
                let old = &s.configuration;
                let changed = old.idp_entity_id != idp_entity_id
                    || old.idp_metadata != idp_metadata
                    || old.group_attribute != group_attribute
                    || old.required_group != required_group
                    || old.enabled != enabled;
                if old.revision != revision
                    || (enabled && state.origin.is_none())
                    || (!local_auth && (changed || !old.verified_for(state.origin.as_deref())))
                {
                    return Ok(false);
                }
                let next = Configuration {
                    version: 1,
                    revision: revision.checked_add(1).ok_or_else(invalid)?,
                    enabled,
                    local_auth,
                    idp_entity_id,
                    idp_metadata,
                    group_attribute,
                    required_group,
                    verified: !changed && old.verified_for(state.origin.as_deref()),
                    verified_origin: if !changed && old.verified_for(state.origin.as_deref()) {
                        old.verified_origin.clone()
                    } else {
                        None
                    },
                };
                if next.validate().is_err() {
                    return Ok(false);
                }
                // Revoke before writing; an uncertain write must not retain SSO sessions.
                slot.revoke_saml();
                s.flows.clear();
                store.save(&next)?;
                s.configuration = next;
            } else {
                slot.revoke_saml();
                s.flows.clear();
            }
            Ok(true)
        })
        .await
    {
        Ok(true) => browser_session::empty(StatusCode::NO_CONTENT),
        Ok(false) => browser_session::empty(StatusCode::CONFLICT),
        Err(e) => service_error(e),
    }
}
async fn metadata(State(state): State<SamlState>) -> Response {
    let Some(origin) = state.origin else {
        return browser_session::empty(StatusCode::SERVICE_UNAVAILABLE);
    };
    match core::sp(&origin) {
        Ok(sp) => (
            [
                (CACHE_CONTROL, "no-store"),
                (CONTENT_TYPE, "application/samlmetadata+xml"),
            ],
            sp.metadata_xml().to_owned(),
        )
            .into_response(),
        Err(_) => browser_session::empty(StatusCode::SERVICE_UNAVAILABLE),
    }
}
async fn start(State(state): State<SamlState>, headers: HeaderMap) -> Response {
    if !state.auth.preauth.consume(&headers) {
        return browser_session::empty(StatusCode::FORBIDDEN);
    }
    let (Some(origin), Some(store)) = (state.origin, state.auth.auth.saml()) else {
        return browser_session::empty(StatusCode::SERVICE_UNAVAILABLE);
    };
    match state
        .auth
        .auth
        .accounts(true, move |_, _| {
            let mut s = store.state.lock().map_err(|_| invalid())?;
            if !s.configuration.enabled {
                return Err(invalid());
            }
            s.flows.retain(|f| Instant::now() < f.expires);
            if s.flows.len() >= 16 {
                return Err(invalid());
            }
            let relay = browser_session::encode(&core::random()?);
            let cookie = core::random()?;
            let sp = core::sp(&origin)?;
            let idp = s.configuration.idp()?;
            let options = StartSso::redirect().relay_state(
                RelayStateParam::try_from_option(Some(relay.clone())).map_err(|_| invalid())?,
            );
            let started = sp.start_sso(&idp, options).map_err(|_| invalid())?;
            let destination = started
                .outbound
                .redirect_url()
                .map_err(|_| invalid())?
                .to_string();
            let url = url::Url::parse(&destination).map_err(|_| invalid())?;
            if url.scheme() != "https"
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
            {
                return Err(invalid());
            }
            s.flows.push(Flow {
                relay,
                cookie,
                pending: started.pending,
                expires: Instant::now() + core::FLOW_LIFETIME,
            });
            Ok((destination, cookie))
        })
        .await
    {
        Ok((destination, cookie)) => {
            let mut response = accounts::json_response(json!({"redirect": destination}));
            response.headers_mut().insert(
                SET_COOKIE,
                browser_session::sensitive(format!(
                    "{COOKIE}={}; {COOKIE_ATTRIBUTES}; Max-Age=300",
                    browser_session::encode(&cookie)
                )),
            );
            response
        }
        Err(e) => service_error(e),
    }
}
fn fields(body: &str) -> std::io::Result<(String, Vec<FormField>)> {
    let mut relay = None;
    let mut response = None;
    for (name, value) in url::form_urlencoded::parse(body.as_bytes()) {
        match name.as_ref() {
            "RelayState" if relay.is_none() => relay = Some(value.into_owned()),
            "SAMLResponse" if response.is_none() => response = Some(value.into_owned()),
            _ => return Err(invalid()),
        }
    }
    let relay = relay.ok_or_else(invalid)?;
    if relay.len() != 64 || !relay.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    Ok((
        relay.clone(),
        vec![
            FormField::new("RelayState", relay),
            FormField::new("SAMLResponse", response.ok_or_else(invalid)?),
        ],
    ))
}
async fn acs(State(state): State<SamlState>, request: Request) -> Response {
    let Some(cookie) = browser_session::named_session(request.headers(), COOKIE) else {
        return denied();
    };
    if request.headers().get_all(CONTENT_TYPE).iter().count() != 1
        || request
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_none_or(|v| v.split(';').next() != Some("application/x-www-form-urlencoded"))
    {
        return denied();
    }
    if request.headers().contains_key("content-encoding") {
        return denied();
    }
    let body = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        axum::body::to_bytes(request.into_body(), 256 * 1024),
    )
    .await
    {
        Ok(Ok(b)) => match String::from_utf8(b.to_vec()) {
            Ok(b) => b,
            Err(_) => return denied(),
        },
        _ => return denied(),
    };
    let (relay, fields) = match fields(&body) {
        Ok(f) => f,
        Err(_) => return denied(),
    };
    let (Some(origin), Some(store)) = (state.origin, state.auth.auth.saml()) else {
        return denied();
    };
    let slot = state.auth.session.clone();
    match state
        .auth
        .auth
        .accounts(false, move |_, _| {
            let mut s = store.state.lock().map_err(|_| invalid())?;
            if !s.configuration.enabled {
                return Err(invalid());
            }
            s.flows.retain(|f| Instant::now() < f.expires);
            let index = s
                .flows
                .iter()
                .position(|f| {
                    bool::from(f.cookie.ct_eq(&cookie))
                        && bool::from(f.relay.as_bytes().ct_eq(relay.as_bytes()))
                })
                .ok_or_else(invalid)?;
            // Consume before signature work, even when verification fails.
            let flow = s.flows.swap_remove(index);
            let idp = s.configuration.idp()?;
            let sp = core::sp(&origin)?;
            let validation = SamlValidationContext::new(
                SystemTime::now(),
                ReplayPolicy::RequireCache(&mut s.replay),
            )
            .with_clock_skew(ClockSkew::from_millis(-30_000, 0));
            let session = sp
                .finish_sso(
                    &idp,
                    &flow.pending,
                    BrowserInput::<SsoResponse>::post(fields),
                    validation,
                )
                .map_err(|_| invalid())?;
            let subject = session.name_id();
            if subject.format() != Some(&NameIdFormat::Persistent)
                || subject.value().is_empty()
                || subject.value().len() > 1024
                || subject.value().chars().any(char::is_control)
            {
                return Err(invalid());
            }
            let c = &s.configuration;
            if !c.required_group.is_empty()
                && !session.attributes().as_slice().iter().any(|a| {
                    a.name() == c.group_attribute
                        && a.values().iter().any(|v| v.as_str() == c.required_group)
                })
            {
                return Err(invalid());
            }
            let mut lifetime = core::SESSION_LIFETIME;
            for authn in session.authn_sessions() {
                if let Some(deadline) = authn.not_on_or_after() {
                    let parsed = time::OffsetDateTime::parse(
                        deadline.as_str(),
                        &time::format_description::well_known::Rfc3339,
                    )
                    .map_err(|_| invalid())?;
                    let deadline: SystemTime = parsed.into();
                    lifetime = lifetime.min(
                        deadline
                            .duration_since(SystemTime::now())
                            .map_err(|_| invalid())?,
                    );
                }
            }
            let identity = core::identity(&c.idp_entity_id, subject.value());
            let mut verified = c.clone();
            verified.verified = true;
            verified.verified_origin = Some(origin);
            let credentials = crate::owner_auth::service::issue_saml(identity, &slot, lifetime)
                .map_err(|_| invalid())?;
            if let Err(error) = store.save(&verified) {
                slot.revoke_identity(identity);
                return Err(error);
            }
            s.configuration = verified;
            Ok(credentials)
        })
        .await
    {
        Ok(credentials) => {
            let mut response = browser_session::activated(credentials);
            *response.status_mut() = StatusCode::SEE_OTHER;
            response
                .headers_mut()
                .insert(LOCATION, HeaderValue::from_static("/sign-in"));
            response.headers_mut().append(
                SET_COOKIE,
                browser_session::sensitive(format!("{COOKIE}=; {COOKIE_ATTRIBUTES}; Max-Age=0")),
            );
            response
        }
        Err(_) => denied(),
    }
}
fn denied() -> Response {
    (StatusCode::UNAUTHORIZED, [(CACHE_CONTROL, "no-store"), (CONTENT_TYPE, "text/html; charset=utf-8")], "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Sign-in unsuccessful</title><h1>Sign-in unsuccessful</h1><p>Your SSO response could not be accepted. Start a new sign-in or contact your administrator.</p><a href=\"/sign-in\">Return to sign in</a></html>").into_response()
}
