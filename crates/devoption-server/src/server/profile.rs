//! Display metadata only. Compose inside the existing page and mutation guards.
use super::*;
use crate::profile::{DisplayName, MAX_NAME_BYTES, ProfileStore};
use axum::extract::Extension;
use tokio::sync::Mutex;

pub(super) fn routes(store: ProfileStore) -> Router {
    let reader = Arc::new(Mutex::new(store));
    let writer = Arc::clone(&reader);
    Router::new().route(
        "/account/profile",
        get(
            move |Extension(identity): Extension<IdentityId>| async move {
                let guard = reader.lock_owned().await;
                let result = tokio::task::spawn_blocking(move || guard.load(identity)).await;
                match result {
                    Ok(Ok(name)) => (
                        StatusCode::OK,
                        [(CACHE_CONTROL, "no-store")],
                        name.as_str().to_owned(),
                    )
                        .into_response(),
                    _ => unavailable(),
                }
            },
        )
        .post(
            move |Extension(identity): Extension<IdentityId>, request: Request| async move {
                let value = match super::input::text_body(request, MAX_NAME_BYTES).await {
                    Ok(value) => value,
                    Err(status) => return (status, [(CACHE_CONTROL, "no-store")]).into_response(),
                };
                let name = match DisplayName::new(&value) {
                    Ok(name) => name,
                    Err(_) => {
                        return (StatusCode::BAD_REQUEST, [(CACHE_CONTROL, "no-store")])
                            .into_response();
                    }
                };
                let guard = writer.lock_owned().await;
                match tokio::task::spawn_blocking(move || guard.save(identity, &name)).await {
                    Ok(Ok(())) => {
                        (StatusCode::NO_CONTENT, [(CACHE_CONTROL, "no-store")]).into_response()
                    }
                    _ => unavailable(),
                }
            },
        ),
    )
}

fn unavailable() -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(CACHE_CONTROL, "no-store")],
        "Profile is unavailable. Reload to check saved state before retrying.",
    )
        .into_response()
}
