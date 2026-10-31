//! Existing-owner HTTP authentication. No enrollment or owner creation routes.

mod accounts;

use super::{SharedSession, protect_origin};
use crate::{
    browser_session,
    owner_auth::service::{OwnerAuthService, ServiceError},
    password_form::PasswordForm,
    preauth_csrf::PreAuthCsrf,
};
use axum::{
    Router,
    extract::{FromRequest, Request, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, RETRY_AFTER},
    },
    middleware,
    response::{IntoResponse, Response},
    routing::post,
};
use std::sync::Arc;

#[derive(Clone)]
struct AuthState {
    auth: OwnerAuthService,
    preauth: Arc<PreAuthCsrf>,
    session: SharedSession,
}

// The startup caller mounts these only after finding a valid existing owner.
pub(super) fn routes(
    auth: OwnerAuthService,
    origin: impl Into<super::RequestOrigin>,
    session: SharedSession,
) -> Router {
    let app = Router::new()
        .merge(accounts::routes())
        .route("/auth/session", post(recover_session))
        .route("/auth/challenge", post(challenge))
        .route("/auth/sign-in", post(sign_in))
        .route("/auth/sign-out", post(sign_out))
        .with_state(AuthState {
            auth,
            preauth: Arc::new(PreAuthCsrf::default()),
            session,
        });
    // Include Host/Origin and method rejections in the non-cacheable boundary.
    protect_origin(app, origin).layer(middleware::map_response(
        |mut response: Response| async move {
            response
                .headers_mut()
                .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        },
    ))
}

async fn recover_session(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    // POST deliberately requires Origin through the shared outer guard even
    // though recovery only reads existing credentials. Never add CORS here.
    browser_session::recover(&headers, &state.session)
}

async fn challenge(State(state): State<AuthState>) -> Response {
    state.preauth.issue()
}

async fn sign_in(State(state): State<AuthState>, request: Request) -> Response {
    if !state.preauth.consume(request.headers()) {
        return browser_session::empty(StatusCode::FORBIDDEN);
    }
    let form = match PasswordForm::from_request(request, &()).await {
        Ok(form) => form,
        Err(error) => return error.into_response(),
    };
    match state.auth.sign_in(form.into_password()).await {
        Ok(Some(grant)) => browser_session::activate(grant, &state.session)
            .unwrap_or_else(|_| browser_session::empty(StatusCode::SERVICE_UNAVAILABLE)),
        Ok(None) => browser_session::empty(StatusCode::UNAUTHORIZED),
        Err(error) => service_error(error),
    }
}

async fn sign_out(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    // Browser logout never accepts bearer credentials or clears a newer cookie
    // after a stale request. Host/Origin have already passed the outer guard.
    browser_session::logout(&headers, &state.session)
}

fn service_error(error: ServiceError) -> Response {
    let (status, retry_after) = match error {
        ServiceError::Busy => (StatusCode::SERVICE_UNAVAILABLE, Some(1)),
        ServiceError::RateLimited { retry_after } => (
            StatusCode::TOO_MANY_REQUESTS,
            Some(
                retry_after
                    .as_secs()
                    .saturating_add(u64::from(retry_after.subsec_nanos() != 0))
                    .clamp(1, 60),
            ),
        ),
        // Never serialize storage paths, worker details, or secret inputs.
        _ => (StatusCode::SERVICE_UNAVAILABLE, None),
    };
    let mut response = browser_session::empty(status);
    if let Some(seconds) = retry_after {
        response.headers_mut().insert(RETRY_AFTER, seconds.into());
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::owner_store::StoreError;
    use std::{io, time::Duration};

    #[test]
    fn service_failures_have_bounded_retry_hints_and_no_internal_details() {
        for (error, status, retry) in [
            (
                ServiceError::Busy,
                StatusCode::SERVICE_UNAVAILABLE,
                Some("1"),
            ),
            (
                ServiceError::RateLimited {
                    retry_after: Duration::ZERO,
                },
                StatusCode::TOO_MANY_REQUESTS,
                Some("1"),
            ),
            (
                ServiceError::RateLimited {
                    retry_after: Duration::from_millis(1500),
                },
                StatusCode::TOO_MANY_REQUESTS,
                Some("2"),
            ),
            (
                ServiceError::RateLimited {
                    retry_after: Duration::MAX,
                },
                StatusCode::TOO_MANY_REQUESTS,
                Some("60"),
            ),
            (
                ServiceError::Unavailable,
                StatusCode::SERVICE_UNAVAILABLE,
                None,
            ),
            (
                ServiceError::Store(StoreError::Io(io::Error::other(
                    "private fixture storage details",
                ))),
                StatusCode::SERVICE_UNAVAILABLE,
                None,
            ),
        ] {
            let response = service_error(error);
            assert_eq!(response.status(), status);
            assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
            assert_eq!(
                response
                    .headers()
                    .get(RETRY_AFTER)
                    .map(|value| value.to_str().unwrap()),
                retry
            );
            assert!(!response.headers().contains_key("set-cookie"));
            assert!(
                !response
                    .headers()
                    .contains_key(browser_session::CSRF_HEADER)
            );
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            assert!(
                runtime
                    .block_on(axum::body::to_bytes(response.into_body(), 1))
                    .unwrap()
                    .is_empty()
            );
        }
    }
}
