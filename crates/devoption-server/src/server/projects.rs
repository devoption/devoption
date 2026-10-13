//! Authenticated catalog adapter. Routes must be inside the page/session and
//! Host/Origin/CSRF guards. Filesystem work never runs on the async event loop.

use super::input::text_body;
use super::*;
use axum::{
    extract::{Path as RoutePath, rejection::PathRejection},
    http::header::LOCATION,
};
use devoption_projects::{Catalog, CatalogDirectory, Error, MAX_NAME_BYTES};
use devoption_projects::{
    ProjectId,
    worktree::{InspectionError, PermittedRoot},
};
use tokio::sync::Mutex;

mod pages;

#[cfg(test)]
mod startup_tests;

#[derive(Clone)]
pub(super) struct Projects {
    directory: Arc<CatalogDirectory>,
    gate: Arc<Mutex<()>>,
    root: Option<Arc<PermittedRoot>>,
}

impl Projects {
    pub(super) fn open_with_root(
        owner: &crate::owner_store::OwnerStore,
        root: Option<&Path>,
    ) -> Result<Self, Error> {
        let directory = CatalogDirectory::from_directory(owner.duplicate_directory()?)?;
        let catalog = directory.load()?.unwrap_or_default();
        let root = root.map(PermittedRoot::open_absolute).transpose()?;
        if let Some(root) = &root {
            if !directory.is_disjoint(root)? {
                return Err(Error::RootMismatch);
            }
            catalog.check_repository_root(root)?;
        }
        Ok(Self {
            directory: Arc::new(directory),
            gate: Arc::new(Mutex::new(())),
            root: root.map(Arc::new),
        })
    }

    async fn access<T: Send + 'static>(
        self,
        operation: impl FnOnce(&CatalogDirectory) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let guard = self.gate.lock_owned().await;
        // Keep serialization even if the HTTP request disconnects while saving.
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            operation(&self.directory)
        })
        .await
        .map_err(|_| Error::Io(std::io::Error::other("catalog operation failed")))?
    }
}

pub(super) fn routes(projects: Projects) -> Router {
    let reader = projects.clone();
    let attachment = projects.clone();
    let options = projects.clone();
    let status_reader = projects.clone();
    let renamer = projects.clone();
    Router::new()
        .merge(pages::routes(projects.clone()))
        .route(
            "/projects/{project}/name",
            axum::routing::post(move |project: Result<RoutePath<String>, PathRejection>, request: Request| async move {
                let result = async {
                    let id = project.ok().and_then(|RoutePath(value)| project_id(&value)).ok_or(StatusCode::NOT_FOUND)?;
                    let name = name(request).await?;
                    renamer.access(move |directory| directory.writer()?.rename_project(id, &name))
                        .await.map_err(|error| Failure::from(error).status)
                }.await;
                (result.map_or_else(|status| status, |()| StatusCode::NO_CONTENT), [(CACHE_CONTROL, "no-store")]).into_response()
            }),
        )
        .route(
            "/projects/{project}/repositories/{repository}/status",
            get(move |ids: Result<RoutePath<(String, String)>, PathRejection>| async move {
                let ids = ids.ok().and_then(|RoutePath((project, repository))| Some((project_id(&project)?, devoption_projects::RepositoryId::from_bytes(project_id(&repository)?.to_bytes()))));
                let Some((project, repository)) = ids else {
                    return (StatusCode::NOT_FOUND, [(CACHE_CONTROL, "no-store")], "Repository is unavailable.").into_response();
                };
                let Some(root) = status_reader.root.clone() else {
                    return (StatusCode::SERVICE_UNAVAILABLE, [(CACHE_CONTROL, "no-store")], "Repository root is not configured.").into_response();
                };
                match status_reader.access(move |directory| directory.load()?.unwrap_or_default().repository_status(project, repository, &root)).await {
                    Ok(status) => {
                        let changes = status.changes;
                        ([(CACHE_CONTROL, "no-store")], format!("{}\n{}\n{}\n{}\n{}\n{}", status.branch.unwrap_or_default(), status.commit.unwrap_or_default(), changes.staged, changes.modified, changes.untracked, changes.conflicted)).into_response()
                    }
                    Err(error) => {
                        let (status, code) = match error {
                            Error::UnknownProject | Error::UnknownRepository => (StatusCode::NOT_FOUND, "missing"),
                            Error::StaleCheckout | Error::RootMismatch | Error::Inspection(InspectionError::Changed) => (StatusCode::CONFLICT, "changed"),
                            Error::Inspection(InspectionError::UnsupportedLayout) => (StatusCode::SERVICE_UNAVAILABLE, "unsupported"),
                            Error::Inspection(InspectionError::LimitExceeded) => (StatusCode::SERVICE_UNAVAILABLE, "limited"),
                            _ => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
                        };
                        (status, [("cache-control", "no-store"), ("x-devoption-error", code)], "Repository status is unavailable.").into_response()
                    }
                }
            }),
        )
        .route(
            "/repository-options",
            get(move || async move {
                let Some(root) = options.root.clone() else {
                    return (StatusCode::SERVICE_UNAVAILABLE, [(CACHE_CONTROL, "no-store")], "Repository attachment is not configured.").into_response();
                };
                let result = options.access(move |directory| {
                    let catalog = directory.load()?.unwrap_or_default();
                    catalog.check_repository_root(&root)?;
                    Ok(root.checkout_options()?)
                }).await;
                match result {
                    Ok(choices) => ([("cache-control", "no-store"), ("x-devoption-list-limited", if choices.limited { "true" } else { "false" })], choices.paths.join("\n")).into_response(),
                    Err(_) => (StatusCode::SERVICE_UNAVAILABLE, [(CACHE_CONTROL, "no-store")], "Checkouts could not be loaded.").into_response(),
                }
            }),
        )
        .route(
            "/projects",
            get(move || async move {
                let attachment_available = reader.root.is_some();
                match reader
                    .access(|directory| Ok(directory.load()?.unwrap_or_default()))
                    .await
                {
                    Ok(catalog) => {
                        ([(CACHE_CONTROL, "no-store")], Html(render(&catalog, attachment_available))).into_response()
                    }
                    Err(error) => (
                        Failure::from(error).status,
                        [(CACHE_CONTROL, "no-store")],
                        "Projects could not be loaded. Reload to try again.",
                    )
                        .into_response(),
                }
            })
            .post(move |request: Request| async move {
                let result = match name(request).await {
                    Ok(name) => projects
                        .access(move |directory| {
                            directory.writer()?.create_project(&name)
                        })
                        .await
                        .map_err(|error| Failure::from(error).status),
                    Err(status) => Err(status),
                };
                let mut response = match result {
                    Ok(id) => {
                        let id: String = id.to_bytes().iter().map(|byte| format!("{byte:02x}")).collect();
                        (StatusCode::NO_CONTENT, [(LOCATION, format!("/projects/{id}"))]).into_response()
                    }
                    Err(status) => status.into_response(),
                };
                response.headers_mut().insert(CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store"));
                response
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
        let root = projects.root.clone().ok_or(Failure {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "root-unconfigured",
        })?;
        let candidate = text_body(request, 4096).await?;
        projects
            .access(move |directory| {
                directory
                    .writer()?
                    .attach_repository(project, &root, Path::new(&candidate))
            })
            .await?;
        Ok::<_, Failure>(())
    }
    .await;
    let code = result.as_ref().err().map(|failure| failure.code);
    let (status, message) = match result {
        Ok(()) => (StatusCode::NO_CONTENT, ""),
        Err(failure) => (
            failure.status,
            match failure.status {
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
    let mut response = (status, [(CACHE_CONTROL, "no-store")], message).into_response();
    if let Some(code) = code {
        response.headers_mut().insert(
            "x-devoption-error",
            axum::http::HeaderValue::from_static(code),
        );
    }
    response
}

struct Failure {
    status: StatusCode,
    code: &'static str,
}

impl From<StatusCode> for Failure {
    fn from(status: StatusCode) -> Self {
        Self {
            status,
            code: "invalid-request",
        }
    }
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        let (status, code) = match error {
            Error::AlreadyAssociated => (StatusCode::CONFLICT, "already-associated"),
            Error::StaleCheckout
            | Error::RootMismatch
            | Error::Inspection(InspectionError::Changed) => {
                (StatusCode::CONFLICT, "checkout-changed")
            }
            Error::Capacity => (StatusCode::CONFLICT, "catalog-full"),
            Error::UnknownProject => (StatusCode::NOT_FOUND, "project-missing"),
            Error::InvalidName | Error::Inspection(_) => {
                (StatusCode::BAD_REQUEST, "invalid-checkout")
            }
            _ => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
        };
        Self { status, code }
    }
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

fn render(catalog: &Catalog, attachment_available: bool) -> String {
    devoption_ui::render_projects(&summaries(catalog), attachment_available)
}

fn summaries(catalog: &Catalog) -> Vec<devoption_ui::ProjectSummary<'_>> {
    let repositories: std::collections::HashMap<_, _> = catalog
        .repositories()
        .iter()
        .map(|repository| (repository.id(), repository))
        .collect();
    let checkouts: std::collections::HashMap<_, _> = catalog
        .default_checkouts()
        .iter()
        .map(|checkout| ((checkout.project(), checkout.repository()), checkout))
        .collect();
    catalog
        .projects()
        .iter()
        .map(|project| devoption_ui::ProjectSummary {
            id: project.id().to_bytes(),
            name: project.name(),
            repositories: project
                .repositories()
                .iter()
                .map(|id| {
                    let repository = repositories
                        .get(id)
                        .expect("validated repository reference");
                    let checkout = checkouts.get(&(project.id(), *id));
                    devoption_ui::RepositorySummary {
                        id: repository.id().to_bytes(),
                        name: repository.name(),
                        default_checkout: checkout
                            .and_then(|checkout| checkout.relative_path().to_str()),
                    }
                })
                .collect(),
        })
        .collect()
}

// One plain UTF-8 name, not a map of fields, paths or client-supplied identities.
async fn name(request: Request) -> Result<String, StatusCode> {
    text_body(request, MAX_NAME_BYTES).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, Bytes},
        http::header::CONTENT_TYPE,
    };
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
