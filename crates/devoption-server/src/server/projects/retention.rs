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
    reconcile_with(history, config, catalog, selected, release_references);
}

fn reconcile_with(
    history: &mut History,
    config: Option<&Configuration>,
    catalog: &Catalog,
    selected: Option<ProjectId>,
    references: impl FnOnce() -> Result<Vec<JobId>, devoption_builds::history::Error>,
) {
    let projects: Vec<_> = catalog
        .projects()
        .iter()
        .filter(|p| selected.is_none_or(|id| p.id() == id))
        .collect();
    if projects.is_empty() {
        return;
    }
    let lock = config.map(Configuration::retention_lock).transpose();
    let refs = match &lock {
        Ok(_) => references(),
        Err(_) => Err(devoption_builds::history::Error::Unavailable),
    };
    for project in projects {
        let result = match &refs {
            Ok(refs) => history.reconcile_retention(
                project.id(),
                config.map(Configuration::work_root),
                || Ok(refs.clone()),
            ),
            Err(error) => Err(*error),
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
                .ok(),
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

pub(in crate::server) async fn reconcile_projects(projects: Projects, selected: Option<ProjectId>) {
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
                selected,
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
                    view.retained = status.retained;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn reconcile_reads_references_once_across_projects_and_respects_selection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("history");
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut history = History::open(path.to_str().unwrap()).unwrap();
        let mut catalog = Catalog::default();
        let first = catalog.create_project("First").unwrap();
        let second = catalog.create_project("Second").unwrap();
        let third = catalog.create_project("Third").unwrap();

        let mut calls = 0;
        reconcile_with(&mut history, None, &catalog, None, || {
            calls += 1;
            Ok(Vec::new())
        });
        assert_eq!(calls, 1);
        for project in [first, second, third] {
            assert_eq!(
                history.retention_status(project).unwrap(),
                Some(RetentionStatus {
                    retained: Some(0),
                    protected: Some(0),
                    ..Default::default()
                })
            );
        }

        // Selecting an unknown project skips reference lookup entirely.
        let mut skipped_calls = 0;
        reconcile_with(
            &mut history,
            None,
            &catalog,
            Some(ProjectId::from_bytes([0xff; 16])),
            || {
                skipped_calls += 1;
                Ok(Vec::new())
            },
        );
        assert_eq!(skipped_calls, 0);

        // Selecting one project updates only that project's status.
        let mut selected_calls = 0;
        reconcile_with(&mut history, None, &catalog, Some(second), || {
            selected_calls += 1;
            Err(devoption_builds::history::Error::Unavailable)
        });
        assert_eq!(selected_calls, 1);
        assert!(!history.retention_status(first).unwrap().unwrap().failed);
        assert!(history.retention_status(second).unwrap().unwrap().failed);
        assert!(!history.retention_status(third).unwrap().unwrap().failed);
    }
}
