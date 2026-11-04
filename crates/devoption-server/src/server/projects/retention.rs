//! Runs under execution -> history -> catalog gates, with filesystem work off
//! the async runtime. Promotion also holds history throughout its broker call.
use super::*;
use devoption_builds::{
    execution::Configuration,
    history::{History, RetentionStatus},
    jobs::JobId,
    retention::Policy,
};

fn release_references() -> Result<Vec<JobId>, devoption_builds::history::Error> {
    use devoption_builds::history::Error as E;
    match std::fs::symlink_metadata("/etc/devoption/deployment.toml") {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(E::Unavailable),
        Ok(_) => {}
    }
    let state = deployment::broker(&["retention-references"]).map_err(|_| E::Unavailable)?;
    let mut ids = Vec::new();
    for job in state["jobs"].as_array().ok_or(E::Corrupt)? {
        let text = job.as_str().ok_or(E::Corrupt)?;
        let bytes = devoption_projects::ProjectId::from_hex(text)
            .ok_or(E::Corrupt)?
            .to_bytes();
        ids.push(JobId::from_bytes(bytes));
    }
    Ok(ids)
}

pub(super) fn reconcile(
    history: &mut History,
    config: Option<&Configuration>,
    catalog: &Catalog,
    selected: Option<ProjectId>,
) {
    let lock = config.map(Configuration::retention_lock).transpose();
    for project in catalog
        .projects()
        .iter()
        .filter(|p| selected.is_none_or(|id| p.id() == id))
    {
        let result = if lock.is_ok() {
            history.reconcile_retention(
                project.id(),
                config.map(Configuration::work_root),
                release_references,
            )
        } else {
            Err(devoption_builds::history::Error::Unavailable)
        };
        let status = result.unwrap_or_else(|_| RetentionStatus {
            failed: true,
            retained: history
                .load()
                .map(|r| {
                    r.iter()
                        .filter(|r| r.project() == project.id().to_bytes())
                        .count()
                })
                .unwrap_or(0),
            ..Default::default()
        });
        if history
            .save_retention_status(project.id(), &status)
            .is_err()
        {
            eprintln!("Build retention status could not be saved; inspect history storage.");
        }
    }
}

pub(in crate::server) async fn reconcile_projects(projects: Projects) {
    let engine = match projects.execution.clone() {
        Some(e) => Some(e.lock_owned().await),
        None => None,
    };
    // An active build may still be writing directories. Never reconcile its
    // interruption until startup's broker stop has established process cleanup.
    let Some(history) = projects.history.clone() else {
        return;
    };
    let mut history = history.lock_owned().await;
    let gate = projects.gate.clone().lock_owned().await;
    let _ = tokio::task::spawn_blocking(move || {
        let _gate = gate;
        if let Ok(Some(catalog)) = projects.directory.load() {
            reconcile(
                &mut history,
                engine.as_ref().map(|e| &e.config),
                &catalog,
                None,
            );
        }
    })
    .await;
}

pub(super) fn routes(projects: Projects) -> Router {
    Router::new().route(
        "/projects/{project}/retention",
        axum::routing::post(
            move |RoutePath(p): RoutePath<String>, request: Request| async move {
                if request
                    .extensions()
                    .get::<IdentityId>()
                    .copied()
                    .is_none_or(|id| Some(id) != projects.owner)
                {
                    return (StatusCode::FORBIDDEN, [(CACHE_CONTROL, "no-store")]).into_response();
                }
                let Some(project) = project_id(&p) else {
                    return (StatusCode::NOT_FOUND, [(CACHE_CONTROL, "no-store")]).into_response();
                };
                let Ok(body) = text_body(request, 4).await else {
                    return (StatusCode::BAD_REQUEST, [(CACHE_CONTROL, "no-store")])
                        .into_response();
                };
                let policy = if !body.is_empty() && body.bytes().all(|b| b.is_ascii_digit()) {
                    body.parse::<u16>().ok().and_then(|n| Policy::new(n).ok())
                } else {
                    None
                };
                let Some(policy) = policy else {
                    return (StatusCode::BAD_REQUEST, [(CACHE_CONTROL, "no-store")])
                        .into_response();
                };
                let engine = match projects.execution.clone() {
                    Some(e) => Some(e.lock_owned().await),
                    None => None,
                };
                let Some(history) = projects.history.clone() else {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        [(CACHE_CONTROL, "no-store")],
                    )
                        .into_response();
                };
                let mut history = history.lock_owned().await;
                let gate = projects.gate.clone().lock_owned().await;
                let result = tokio::task::spawn_blocking(move || -> Result<(), Error> {
                    let _gate = gate;
                    let catalog = projects.directory.load()?.unwrap_or_default();
                    if !catalog.projects().iter().any(|p| p.id() == project) {
                        return Err(Error::UnknownProject);
                    }
                    history
                        .set_retention_policy(project, policy)
                        .map_err(|_| Error::InvalidCatalog)?;
                    reconcile(
                        &mut history,
                        engine.as_ref().map(|e| &e.config),
                        &catalog,
                        Some(project),
                    );
                    Ok(())
                })
                .await;
                let status = match result {
                    Ok(Ok(())) => StatusCode::NO_CONTENT,
                    Ok(Err(Error::UnknownProject)) => StatusCode::NOT_FOUND,
                    _ => StatusCode::SERVICE_UNAVAILABLE,
                };
                (status, [(CACHE_CONTROL, "no-store")]).into_response()
            },
        ),
    )
}

pub(super) async fn show(projects: Projects, id: ProjectId) -> axum::response::Response {
    let history = match projects.history.clone() {
        Some(h) => Some(h.lock_owned().await),
        None => None,
    };
    let gate = projects.gate.clone().lock_owned().await;
    let result = tokio::task::spawn_blocking(move || -> Result<String, Error> {
        let _gate = gate;
        let catalog = projects.directory.load()?.unwrap_or_default();
        let summaries = summaries(&catalog);
        let project = summaries
            .iter()
            .find(|p| p.id == id.to_bytes())
            .ok_or(Error::UnknownProject)?;
        let mut view = devoption_ui::RetentionView {
            count: 10,
            configured: history.is_some(),
            ..Default::default()
        };
        if let Some(history) = history {
            match history.retention_policy(id) {
                Ok(policy) => view.count = policy.count(),
                Err(_) => view.unavailable = true,
            }
            match history.retention_status(id) {
                Ok(Some(status)) => {
                    view.retained = Some(status.retained);
                    view.protected = status.protected;
                    view.removed = status.removed;
                    view.reclaimed_bytes = status.reclaimed_bytes;
                    view.failed = status.failed;
                }
                Ok(None) => {}
                Err(_) => view.status_unavailable = true,
            }
        }
        Ok(devoption_ui::render_project_settings(project, &view))
    })
    .await;
    match result {
        Ok(Ok(html)) => ([(CACHE_CONTROL, "no-store")], Html(html)).into_response(),
        Ok(Err(Error::UnknownProject)) => {
            (StatusCode::NOT_FOUND, [(CACHE_CONTROL, "no-store")]).into_response()
        }
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            [(CACHE_CONTROL, "no-store")],
        )
            .into_response(),
    }
}
