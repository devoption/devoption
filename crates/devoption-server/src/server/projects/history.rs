//! Read-only saved attempts. Fresh catalog association checks precede log reads.
use super::*;
use devoption_builds::{
    jobs::{Failure as JobFailure, JobId, State},
    output::Stream,
};
use devoption_ui::{BuildAttemptView, BuildHistoryView, BuildLogView, HistoryAvailability};

pub(super) fn routes(projects: Projects) -> Router {
    Router::new().route(
        "/projects/{project}/builds/attempts/{attempt}",
        get(
            move |ids: Result<RoutePath<(String, String)>, PathRejection>| async move {
                let ids = ids.ok().and_then(|RoutePath((p, a))| {
                    Some((
                        project_id(&p)?,
                        JobId::from_bytes(project_id(&a)?.to_bytes()),
                    ))
                });
                match ids {
                    Some((project, attempt)) => show(projects, project, Some(attempt)).await,
                    None => missing(),
                }
            },
        ),
    )
}
fn missing() -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        [(CACHE_CONTROL, "no-store")],
        "Build history is unavailable for this project.",
    )
        .into_response()
}
pub(super) async fn show(
    projects: Projects,
    project: ProjectId,
    attempt: Option<JobId>,
) -> axum::response::Response {
    // Match startup lock order: history, then catalog. Move guards into the
    // blocking task so disconnected requests cannot release serialization early.
    let history = match projects.history.clone() {
        Some(store) => Some(store.lock_owned().await),
        None => None,
    };
    let catalog_guard = projects.gate.clone().lock_owned().await;
    let result = tokio::task::spawn_blocking(move || {
        let _guard = catalog_guard;
        let catalog = projects
            .directory
            .load()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
            .unwrap_or_default();
        let entry = catalog
            .projects()
            .iter()
            .find(|p| p.id() == project)
            .ok_or(StatusCode::NOT_FOUND)?;
        let mut view = BuildHistoryView {
            attempts: Vec::new(),
            availability: HistoryAvailability::Unconfigured,
            selected: attempt.map(JobId::to_bytes),
            log: BuildLogView::Missing,
            source_available: projects.root.is_some(),
        };
        let mut status = StatusCode::OK;
        if let Some(history) = history {
            match history.load() {
                Err(_) => {
                    view.availability = HistoryAvailability::Unavailable;
                    status = StatusCode::SERVICE_UNAVAILABLE;
                }
                Ok(records) => {
                    view.availability = HistoryAvailability::Available;
                    // Filter by the project before interpreting an attempt or reading its log.
                    for record in records.iter().filter(|r| r.project() == project.to_bytes()) {
                        let repository =
                            devoption_projects::RepositoryId::from_bytes(record.repository());
                        if !entry.repositories().contains(&repository) {
                            return Err(StatusCode::SERVICE_UNAVAILABLE);
                        }
                        let repository = catalog
                            .repositories()
                            .iter()
                            .find(|r| r.id() == repository)
                            .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
                        let (state, explanation) = state(record.state());
                        view.attempts.push(BuildAttemptView {
                            id: record.id().to_bytes(),
                            repository: repository.name().to_owned(),
                            commit: record.commit().to_owned(),
                            source: record
                                .source_sha256()
                                .iter()
                                .map(|b| format!("{b:02x}"))
                                .collect(),
                            state,
                            explanation,
                        });
                    }
                    if let Some(id) = attempt {
                        if !view.attempts.iter().any(|r| r.id == id.to_bytes()) {
                            return Err(StatusCode::NOT_FOUND);
                        }
                        view.log = match history.load_output(id) {
                            Ok(None) => BuildLogView::Missing,
                            Ok(Some(output)) => BuildLogView::Saved {
                                stdout: output.display_text(Stream::Stdout),
                                stderr: output.display_text(Stream::Stderr),
                                bytes: output.retained_bytes(),
                                truncated: output.truncated(),
                                limit_reached: output.limit_reached(),
                            },
                            Err(_) => {
                                status = StatusCode::SERVICE_UNAVAILABLE;
                                BuildLogView::Unavailable
                            }
                        };
                    }
                }
            }
        } else if attempt.is_some() {
            return Err(StatusCode::NOT_FOUND);
        }
        // If history itself could not be read, no partial detail is presented.
        if attempt.is_some() && matches!(view.availability, HistoryAvailability::Unavailable) {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        let summaries = summaries(&catalog);
        let project = summaries
            .iter()
            .find(|p| p.id == project.to_bytes())
            .expect("validated project");
        Ok((devoption_ui::render_build_history(project, &view), status))
    })
    .await;
    match result {
        Ok(Ok((html, status))) => {
            (status, [(CACHE_CONTROL, "no-store")], Html(html)).into_response()
        }
        Ok(Err(StatusCode::NOT_FOUND)) => missing(),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            [(CACHE_CONTROL, "no-store")],
            "Build history could not be loaded. Reload to try again.",
        )
            .into_response(),
    }
}
fn state(state: &State) -> (&'static str, &'static str) {
    match state {
        State::Queued => (
            "Queued",
            "Saved as queued. This record does not indicate that execution was approved or started.",
        ),
        State::Running => (
            "Running",
            "Saved as running. This is a historical checkpoint, not confirmation that a process is currently alive.",
        ),
        State::Succeeded(_) => (
            "Succeeded",
            "Recorded as succeeded. Artifact bytes and provenance are not independently verified by this page.",
        ),
        State::Cancelled => (
            "Cancelled",
            "This attempt was recorded as cancelled. It will not resume.",
        ),
        State::Interrupted => (
            "Interrupted",
            "The service lost track of this attempt. It will not resume. Cleanup of any remaining processes has not been confirmed.",
        ),
        State::Failed(reason) => (
            "Failed",
            match reason {
                JobFailure::Preparation => "Preparation failed before execution started.",
                JobFailure::ExitFailure => "The recorded process exit was unsuccessful.",
                JobFailure::TimedOut => "The attempt was recorded as exceeding its time limit.",
                JobFailure::Terminated => "The attempt was recorded as terminated.",
                JobFailure::ArtifactRejected => "The selected artifact was rejected.",
            },
        ),
    }
}
