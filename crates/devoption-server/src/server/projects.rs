//! Authenticated catalog adapter. Routes must be inside the page/session and
//! Host/Origin/CSRF guards. Filesystem work never runs on the async event loop.

use super::*;
use axum::{body::to_bytes, http::header::CONTENT_TYPE};
use devoption_projects::{Catalog, CatalogStore, Error, MAX_NAME_BYTES, read_catalog};
use std::{error::Error as _, path::PathBuf, time::Duration};
use tokio::sync::Mutex;

#[derive(Clone)]
pub(super) struct Projects {
    path: Arc<PathBuf>,
    gate: Arc<Mutex<()>>,
}

impl Projects {
    pub(super) fn open(path: &Path) -> Result<Self, Error> {
        read_catalog(path)?;
        Ok(Self {
            path: Arc::new(path.into()),
            gate: Arc::new(Mutex::new(())),
        })
    }

    async fn access<T: Send + 'static>(
        self,
        operation: impl FnOnce(&Path) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, StatusCode> {
        let guard = self.gate.lock_owned().await;
        // Keep serialization even if the HTTP request disconnects while saving.
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            operation(&self.path)
        })
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .map_err(|error| match error {
            Error::InvalidName => StatusCode::BAD_REQUEST,
            Error::Capacity => StatusCode::CONFLICT,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        })
    }
}

pub(super) fn routes(projects: Projects) -> Router {
    let reader = projects.clone();
    Router::new().route(
        "/projects",
        get(move || async move {
            match reader
                .access(|path| Ok(read_catalog(path)?.unwrap_or_default()))
                .await
            {
                Ok(catalog) => {
                    ([(CACHE_CONTROL, "no-store")], Html(render(&catalog))).into_response()
                }
                Err(status) => (
                    status,
                    [(CACHE_CONTROL, "no-store")],
                    "Projects could not be loaded. Reload to try again.",
                )
                    .into_response(),
            }
        })
        .post(move |request: Request| async move {
            let result = match name(request).await {
                Ok(name) => projects
                    .access(move |path| CatalogStore::open_absolute(path)?.create_project(&name))
                    .await
                    .map(|_| StatusCode::NO_CONTENT),
                Err(status) => Err(status),
            };
            (
                result.unwrap_or_else(|status| status),
                [(CACHE_CONTROL, "no-store")],
            )
                .into_response()
        }),
    )
}

fn render(catalog: &Catalog) -> String {
    let summaries: Vec<_> = catalog
        .projects()
        .iter()
        .map(|project| devoption_ui::ProjectSummary {
            name: project.name(),
            repository_count: project.repositories().len(),
        })
        .collect();
    devoption_ui::render_projects(&summaries)
}

// One plain UTF-8 name, not a map of fields, paths or client-supplied identities.
async fn name(request: Request) -> Result<String, StatusCode> {
    let mut media = request.headers().get_all(CONTENT_TYPE).iter();
    let accepted = media
        .next()
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("text/plain")
                || value.eq_ignore_ascii_case("text/plain;charset=UTF-8")
        });
    if !accepted || media.next().is_some() || request.headers().contains_key("content-encoding") {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let bytes = tokio::time::timeout(
        Duration::from_secs(5),
        to_bytes(request.into_body(), MAX_NAME_BYTES),
    )
    .await
    .map_err(|_| StatusCode::REQUEST_TIMEOUT)?
    .map_err(|error| {
        if error
            .source()
            .is_some_and(|source| source.is::<http_body_util::LengthLimitError>())
        {
            StatusCode::PAYLOAD_TOO_LARGE
        } else {
            StatusCode::BAD_REQUEST
        }
    })?;
    String::from_utf8(bytes.to_vec()).map_err(|_| StatusCode::BAD_REQUEST)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, Bytes};
    use std::{
        convert::Infallible,
        pin::Pin,
        task::{Context, Poll},
    };

    struct PendingBody;
    impl http_body::Body for PendingBody {
        type Data = Bytes;
        type Error = Infallible;
        fn poll_frame(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<http_body::Frame<Bytes>, Infallible>>> {
            Poll::Pending
        }
    }

    #[test]
    fn name_body_rejects_invalid_utf8_oversize_and_stalled_uploads() {
        Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap()
            .block_on(async {
                for (body, status) in [
                    (Body::from(vec![0xff]), StatusCode::BAD_REQUEST),
                    (Body::from("x".repeat(129)), StatusCode::PAYLOAD_TOO_LARGE),
                    (Body::new(PendingBody), StatusCode::REQUEST_TIMEOUT),
                ] {
                    let request = Request::builder()
                        .header(CONTENT_TYPE, "text/plain")
                        .body(body)
                        .unwrap();
                    assert_eq!(name(request).await.unwrap_err(), status);
                }
                let mut duplicate = Request::builder()
                    .header(CONTENT_TYPE, "text/plain")
                    .body(Body::from("Example"))
                    .unwrap();
                duplicate
                    .headers_mut()
                    .append(CONTENT_TYPE, "text/plain".parse().unwrap());
                assert_eq!(
                    name(duplicate).await.unwrap_err(),
                    StatusCode::UNSUPPORTED_MEDIA_TYPE
                );
            });
    }
}
