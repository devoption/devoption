//! Authenticated catalog adapter. Routes must be inside the page/session and
//! Host/Origin/CSRF guards. Filesystem work never runs on the async event loop.

use super::*;
use axum::{
    body::to_bytes,
    extract::{Path as RoutePath, rejection::PathRejection},
    http::header::CONTENT_TYPE,
};
use devoption_projects::{Catalog, CatalogStore, Error, MAX_NAME_BYTES, read_catalog};
use devoption_projects::{
    ProjectId,
    worktree::{InspectionError, PermittedRoot},
};
use std::{error::Error as _, path::PathBuf, time::Duration};
use tokio::sync::Mutex;

#[derive(Clone)]
pub(super) struct Projects {
    path: Arc<PathBuf>,
    gate: Arc<Mutex<()>>,
    root: Option<Arc<PermittedRoot>>,
}

impl Projects {
    pub(super) fn open_with_root(path: &Path, root: Option<&Path>) -> Result<Self, Error> {
        let catalog = read_catalog(path)?.unwrap_or_default();
        let root = root.map(PermittedRoot::open_absolute).transpose()?;
        if let Some(root) = &root {
            if !root.is_disjoint(&PermittedRoot::open_absolute(path)?)? {
                return Err(Error::RootMismatch);
            }
            catalog.check_repository_root(root)?;
        }
        Ok(Self {
            path: Arc::new(path.into()),
            gate: Arc::new(Mutex::new(())),
            root: root.map(Arc::new),
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
            Error::Inspection(InspectionError::Changed) => StatusCode::CONFLICT,
            Error::InvalidName | Error::Inspection(_) => StatusCode::BAD_REQUEST,
            Error::UnknownProject => StatusCode::NOT_FOUND,
            Error::Capacity
            | Error::AlreadyAssociated
            | Error::RootMismatch
            | Error::StaleCheckout => StatusCode::CONFLICT,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        })
    }
}

pub(super) fn routes(projects: Projects) -> Router {
    let reader = projects.clone();
    let attachment = projects.clone();
    Router::new()
        .route(
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
                        .access(move |path| {
                            CatalogStore::open_absolute(path)?.create_project(&name)
                        })
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
        .route(
            "/projects/{project}/repositories",
            axum::routing::post(
                move |project: Result<RoutePath<String>, PathRejection>,
                      request: Request| async move {
                    let project = project
                        .map(|RoutePath(project)| project)
                        .map_err(|_| StatusCode::BAD_REQUEST);
                    attach(attachment, project, request).await
                },
            ),
        )
}

async fn attach(
    projects: Projects,
    project: Result<String, StatusCode>,
    request: Request,
) -> axum::response::Response {
    let result = async {
        let project = project_id(&project?).ok_or(StatusCode::BAD_REQUEST)?;
        let root = projects
            .root
            .clone()
            .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        let candidate = text_body(request, 4096).await?;
        projects
            .access(move |path| {
                CatalogStore::open_absolute(path)?.attach_repository(
                    project,
                    &root,
                    Path::new(&candidate),
                )
            })
            .await?;
        Ok::<_, StatusCode>(())
    }
    .await;
    let (status, message) = match result {
        Ok(()) => (StatusCode::NO_CONTENT, ""),
        Err(status) => (
            status,
            match status {
                StatusCode::BAD_REQUEST => "Repository checkout could not be validated.",
                StatusCode::NOT_FOUND => "Project does not exist.",
                StatusCode::CONFLICT => {
                    "Repository association conflicts with saved state or catalog capacity."
                }
                StatusCode::PAYLOAD_TOO_LARGE => "Repository path is too long.",
                StatusCode::UNSUPPORTED_MEDIA_TYPE => "Use one plain UTF-8 repository path.",
                StatusCode::REQUEST_TIMEOUT => "Repository request timed out.",
                _ => "Repository attachment is unavailable. Inspect saved state before retrying.",
            },
        ),
    };
    (status, [(CACHE_CONTROL, "no-store")], message).into_response()
}

fn project_id(value: &str) -> Option<ProjectId> {
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0; 16];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(ProjectId::from_bytes(bytes))
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
    text_body(request, MAX_NAME_BYTES).await
}

async fn text_body(request: Request, limit: usize) -> Result<String, StatusCode> {
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
    let bytes = tokio::time::timeout(Duration::from_secs(5), to_bytes(request.into_body(), limit))
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

    #[test]
    fn project_identifiers_require_exact_hex_without_numeric_signs() {
        for invalid in [
            "",
            &"0".repeat(31),
            &"0".repeat(33),
            &"+0".repeat(16),
            &"é".repeat(16),
            &"gg".repeat(16),
        ] {
            assert!(project_id(invalid).is_none());
        }
        assert_eq!(project_id(&"aF".repeat(16)).unwrap().to_bytes(), [0xaf; 16]);
    }

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
