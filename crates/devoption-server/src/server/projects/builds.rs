//! Read-only source preparation. No approval, mutation or execution route.
use super::*;
use devoption_builds::preparation::{self, Outcome};
use devoption_projects::RepositoryId;
use devoption_ui::{PreparationCheck, PreparationView};

pub(super) fn routes(projects: Projects) -> Router {
    Router::new().route("/projects/{project}/builds/{repository}", get(move |ids: Result<RoutePath<(String, String)>, PathRejection>| async move {
        let ids = ids.ok().and_then(|RoutePath((p, r))| Some((project_id(&p)?, RepositoryId::from_bytes(project_id(&r)?.to_bytes()))));
        let Some((project, repository)) = ids else { return missing(); };
        let root = projects.root.clone();
        let execution_available = projects.execution.is_some();
        let loaded = projects.access(move |directory| {
            let catalog = directory.load()?.unwrap_or_default();
            let record = catalog.projects().iter().find(|p| p.id() == project).ok_or(Error::UnknownProject)?;
            if !record.repositories().contains(&repository) { return Err(Error::UnknownRepository); }
            let result = root.as_ref().ok_or(Error::CheckoutRequired).and_then(|root| preparation::inspect(&catalog, project, repository, root));
            Ok((catalog, result))
        }).await;
        let (catalog, result) = match loaded {
            Ok(value) => value,
            Err(Error::UnknownProject | Error::UnknownRepository) => return missing(),
            Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, [(CACHE_CONTROL, "no-store")], "Source checks could not be loaded.").into_response(),
        };
        let summaries = summaries(&catalog);
        let project = summaries.iter().find(|p| p.id == project.to_bytes()).expect("validated project");
        let repository = project.repositories.iter().find(|r| r.id == repository.to_bytes()).expect("validated association");
        let (status, unavailable) = match &result {
            Ok(_) => (StatusCode::OK, None),
            Err(Error::StaleCheckout | Error::RootMismatch | Error::Inspection(InspectionError::Changed)) => (StatusCode::CONFLICT, Some("The checkout changed during inspection. Open Code to inspect its attachment, then check again.")),
            Err(Error::Inspection(InspectionError::LimitExceeded)) => (StatusCode::SERVICE_UNAVAILABLE, Some("The repository exceeds the source inspection limits. At most 256 tracked paths can be inspected.")),
            Err(Error::Inspection(InspectionError::UnsupportedLayout)) => (StatusCode::SERVICE_UNAVAILABLE, Some("This Git layout or configuration is not supported for source checks.")),
            Err(_) => (StatusCode::SERVICE_UNAVAILABLE, Some("Repository storage or the saved checkout is unavailable. Open Code to inspect its attachment.")),
        };
        let report = result.as_ref().ok();
        let view = PreparationView {
            repository_id: repository.id,
            repository_name: repository.name,
            headline: match report.map(|r| r.outcome()) { Some(Outcome::Passed) => "Source checks passed", Some(Outcome::ChangesNeeded) => "Changes needed", _ => "Source checks unavailable" },
            commit: report.and_then(|r| r.status.commit.as_deref()),
            branch: report.and_then(|r| r.status.branch.as_deref()),
            execution_available,
            package: report.and_then(|r| r.cargo.as_ref()).map(|c| (c.package(), c.version(), c.dependency_count())),
            checks: report.map(|r| r.checks.iter().map(|c| PreparationCheck { label: c.label, outcome: match c.outcome { Outcome::Passed => "Passed", Outcome::ChangesNeeded => "Needs changes", Outcome::Unavailable => "Unavailable" }, detail: &c.detail }).collect()).unwrap_or_default(),
            unavailable,
        };
        (status, [(CACHE_CONTROL, "no-store")], Html(devoption_ui::render_build_preparation(project, &view))).into_response()
    }))
}

fn missing() -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        [(CACHE_CONTROL, "no-store")],
        "Project or repository does not exist.",
    )
        .into_response()
}
