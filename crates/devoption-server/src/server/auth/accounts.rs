//! Account routes use the existing origin, CSRF and bounded authentication worker.
use super::*;
use crate::{identity::IdentityId, users::Status};
use axum::routing::get;
use serde::Deserialize;
use serde_json::{Value, json};

pub(super) fn routes() -> Router<AuthState> {
    Router::new()
        .route(
            "/join",
            get(|| async {
                (
                    [(CACHE_CONTROL, "no-store")],
                    axum::response::Html(devoption_ui::render_join()),
                )
            }),
        )
        .route("/settings/users", get(users_page))
        .route("/auth/account", get(account))
        .route("/auth/registration", get(registration))
        .route("/auth/users", get(list).post(manage))
        .route("/auth/user-sign-in", post(sign_in))
        .route("/auth/register", post(register))
}
fn json_response(value: Value) -> Response {
    (
        [
            (CACHE_CONTROL, "no-store"),
            (axum::http::header::CONTENT_TYPE, "application/json"),
        ],
        value.to_string(),
    )
        .into_response()
}
async fn input<T: serde::de::DeserializeOwned>(request: Request) -> Result<T, StatusCode> {
    let body = super::super::input::text_body(request, 8192).await?;
    serde_json::from_str(&body).map_err(|_| StatusCode::BAD_REQUEST)
}
async fn registration(State(state): State<AuthState>) -> Response {
    match state
        .auth
        .accounts(false, |_, users| Ok(users.load()?.registration))
        .await
    {
        Ok(enabled) => json_response(json!({"registration": enabled})),
        Err(error) => service_error(error),
    }
}
async fn account(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    let Some(id) = browser_session::authenticate_read(&headers, &state.session) else {
        return browser_session::empty(StatusCode::UNAUTHORIZED);
    };
    match state
        .auth
        .accounts(false, move |owner, _| {
            Ok(owner
                .load()
                .map_err(std::io::Error::other)?
                .is_some_and(|record| record.identity() == id))
        })
        .await
    {
        Ok(owner) => json_response(json!({"role": if owner { "Owner" } else { "Member" }})),
        Err(error) => service_error(error),
    }
}
async fn is_owner(state: &AuthState, id: IdentityId) -> Result<bool, ServiceError> {
    state
        .auth
        .accounts(false, move |owner, _| {
            Ok(owner
                .load()
                .map_err(std::io::Error::other)?
                .is_some_and(|record| record.identity() == id))
        })
        .await
}
async fn users_page(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    let Some(id) = browser_session::authenticate_read(&headers, &state.session) else {
        return (
            StatusCode::SEE_OTHER,
            [
                ("location", "/sign-in"),
                (CACHE_CONTROL.as_str(), "no-store"),
            ],
        )
            .into_response();
    };
    match is_owner(&state, id).await {
        Ok(true) => (
            [(CACHE_CONTROL, "no-store")],
            axum::response::Html(devoption_ui::render_users()),
        )
            .into_response(),
        Ok(false) => browser_session::empty(StatusCode::FORBIDDEN),
        Err(error) => service_error(error),
    }
}
async fn list(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    let Some(id) = browser_session::authenticate_read(&headers, &state.session) else {
        return browser_session::empty(StatusCode::UNAUTHORIZED);
    };
    match state.auth.accounts(false, move |owner, users| {
        if !owner.load().map_err(std::io::Error::other)?.is_some_and(|record| record.identity() == id) { return Ok(None); }
        let accounts = users.load()?;
        Ok(Some(json!({"registration": accounts.registration, "users": accounts.users.iter().map(|user| json!({"email": user.email, "status": user.status})).collect::<Vec<_>>()})))
    }).await {
        Ok(Some(value)) => json_response(value), Ok(None) => browser_session::empty(StatusCode::FORBIDDEN), Err(error) => service_error(error),
    }
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
enum Management {
    Registration { enabled: bool },
    Invite { email: String },
    Approve { email: String },
    Disable { email: String },
    Remove { email: String },
}
async fn manage(State(state): State<AuthState>, request: Request) -> Response {
    let Some(id) = browser_session::authenticate(request.headers(), &state.session) else {
        return browser_session::empty(StatusCode::UNAUTHORIZED);
    };
    // Reject member requests before reading their body or performing password work.
    match is_owner(&state, id).await {
        Ok(true) => (),
        Ok(false) => return browser_session::empty(StatusCode::FORBIDDEN),
        Err(error) => return service_error(error),
    }
    let command: Management = match input(request).await {
        Ok(v) => v,
        Err(e) => return browser_session::empty(e),
    };
    let session = Arc::clone(&state.session);
    match state
        .auth
        .accounts(false, move |owner, users| {
            // Recheck server-owned identity inside the serialized mutation.
            if !owner
                .load()
                .map_err(std::io::Error::other)?
                .is_some_and(|record| record.identity() == id)
            {
                return Ok(None);
            }
            if let Management::Invite { email } = command {
                return users
                    .invite(&email)
                    .map(|token| Some(json!({"invitation": token})));
            }
            let mut accounts = users.load()?;
            match command {
                Management::Registration { enabled } => accounts.registration = enabled,
                Management::Approve { email } => {
                    let Some(user) = accounts
                        .users
                        .iter_mut()
                        .find(|u| u.email == email && u.status == Status::Pending)
                    else {
                        return Ok(None);
                    };
                    user.status = Status::Active;
                }
                Management::Disable { email } => {
                    let Some(user) = accounts.users.iter_mut().find(|u| u.email == email) else {
                        return Ok(None);
                    };
                    user.status = Status::Disabled;
                    // Revoke before publication: uncertain writes must not retain access.
                    session.revoke_identity(IdentityId::from_bytes(user.id));
                }
                Management::Remove { email } => {
                    let Some(index) = accounts
                        .users
                        .iter()
                        .position(|u| u.email == email && u.status != Status::Active)
                    else {
                        return Ok(None);
                    };
                    session.revoke_identity(IdentityId::from_bytes(accounts.users[index].id));
                    accounts.users.remove(index);
                }
                Management::Invite { .. } => unreachable!(),
            }
            users.save(&accounts)?;
            Ok(Some(json!({"saved": true})))
        })
        .await
    {
        Ok(Some(value)) => json_response(value),
        Ok(None) => browser_session::empty(StatusCode::CONFLICT),
        Err(error) => service_error(error),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignIn {
    email: String,
    password: String,
}
async fn sign_in(State(state): State<AuthState>, request: Request) -> Response {
    if !state.preauth.consume(request.headers()) {
        return browser_session::empty(StatusCode::FORBIDDEN);
    }
    let form: SignIn = match input(request).await {
        Ok(v) => v,
        Err(e) => return browser_session::empty(e),
    };
    if !crate::password::valid_length(&form.password) {
        return browser_session::empty(StatusCode::BAD_REQUEST);
    }
    match state
        .auth
        .sign_in_user(form.email, form.password, state.session)
        .await
    {
        Ok(Some(credentials)) => browser_session::activated(credentials),
        Ok(None) => browser_session::empty(StatusCode::UNAUTHORIZED),
        Err(error) => service_error(error),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    email: String,
    password: String,
    invitation: String,
}
async fn register(State(state): State<AuthState>, request: Request) -> Response {
    if !state.preauth.consume(request.headers()) {
        return browser_session::empty(StatusCode::FORBIDDEN);
    }
    let form: Registration = match input(request).await {
        Ok(v) => v,
        Err(e) => return browser_session::empty(e),
    };
    match state
        .auth
        .accounts(true, move |_, users| {
            users.enroll(&form.email, &form.password, &form.invitation)
        })
        .await
    {
        // Same response for duplicate, closed and unknown applications. Never issue a session.
        Ok(_) => browser_session::empty(StatusCode::ACCEPTED),
        Err(error) => service_error(error),
    }
}
