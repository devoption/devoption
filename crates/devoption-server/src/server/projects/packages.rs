//! Authenticated local library artifacts, separate from registry distribution.
use super::*;
use axum::http::header;
use devoption_builds::jobs::JobId;

pub(super) fn routes(projects: Projects) -> Router {
    let receipt = projects.clone();
    Router::new()
        .route(
            "/projects/{project}/builds/attempts/{attempt}/package",
            get(move |RoutePath((p, a)): RoutePath<(String, String)>| {
                download(projects, p, a, false)
            }),
        )
        .route(
            "/projects/{project}/builds/attempts/{attempt}/package/receipt",
            get(move |RoutePath((p, a)): RoutePath<(String, String)>| {
                download(receipt, p, a, true)
            }),
        )
}

async fn download(
    projects: Projects,
    project: String,
    attempt: String,
    receipt: bool,
) -> axum::response::Response {
    let ids = project_id(&project).zip(project_id(&attempt));
    let Some((project, attempt)) = ids else {
        return unavailable(StatusCode::NOT_FOUND);
    };
    let attempt = JobId::from_bytes(attempt.to_bytes());
    let config = match projects.execution.as_ref() {
        Some(engine) => Some(engine.lock().await.config.clone()),
        None => None,
    };
    let history = match projects.history.clone() {
        Some(history) => Some(history.lock_owned().await),
        None => None,
    };
    let catalog_guard = projects.gate.clone().lock_owned().await;
    let result = tokio::task::spawn_blocking(move || {
        let _catalog_guard = catalog_guard;
        let catalog = projects
            .directory
            .load()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
            .unwrap_or_default();
        let selected = catalog
            .projects()
            .iter()
            .find(|p| p.id() == project)
            .ok_or(StatusCode::NOT_FOUND)?;
        let history = history.ok_or(StatusCode::NOT_FOUND)?;
        let records = history
            .load()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let record = records
            .iter()
            .find(|r| r.id() == attempt && r.project() == project.to_bytes())
            .ok_or(StatusCode::NOT_FOUND)?;
        if !matches!(record.state(), devoption_builds::jobs::State::Succeeded(_)) {
            return Err(StatusCode::NOT_FOUND);
        }
        let repository = devoption_projects::RepositoryId::from_bytes(record.repository());
        if !selected.repositories().contains(&repository) {
            return Err(StatusCode::NOT_FOUND);
        }
        let config = config.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        let _retention = config
            .retention_lock()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let package = history
            .library_package(attempt, config.work_root())
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
            .ok_or(StatusCode::NOT_FOUND)?;
        Ok(if receipt {
            package.receipt
        } else {
            package.source
        })
    })
    .await;
    match result {
        Ok(Ok(bytes)) => (
            [
                (CACHE_CONTROL, "no-store"),
                (
                    header::CONTENT_TYPE,
                    if receipt {
                        "text/plain; charset=utf-8"
                    } else {
                        "application/x-tar"
                    },
                ),
                (
                    header::CONTENT_DISPOSITION,
                    if receipt {
                        "attachment; filename=library-build-receipt.toml"
                    } else {
                        "attachment; filename=library-source.tar"
                    },
                ),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            bytes,
        )
            .into_response(),
        Ok(Err(status)) => unavailable(status),
        Err(_) => unavailable(StatusCode::SERVICE_UNAVAILABLE),
    }
}

fn unavailable(status: StatusCode) -> axum::response::Response {
    (
        status,
        [(CACHE_CONTROL, "no-store")],
        "Library package is unavailable for this attempt.",
    )
        .into_response()
}
