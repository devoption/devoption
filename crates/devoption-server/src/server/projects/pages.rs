//! Fresh authenticated project pages; mutation routes remain in the adapter.
use super::*;
use devoption_ui::ProjectSection;

pub(super) fn routes(projects: Projects) -> Router {
    let overview = projects.clone();
    Router::new()
        .route(
            "/projects/{project}",
            get(
                move |project: Result<RoutePath<String>, PathRejection>| async move {
                    show(
                        overview,
                        project.ok().and_then(|RoutePath(value)| project_id(&value)),
                        Some(ProjectSection::Overview),
                    )
                    .await
                },
            ),
        )
        .route(
            "/projects/{project}/{section}",
            get(
                move |path: Result<RoutePath<(String, String)>, PathRejection>| async move {
                    let (project, section) = path
                        .ok()
                        .map(|RoutePath((project, section))| {
                            (project_id(&project), ProjectSection::from_segment(&section))
                        })
                        .unwrap_or_default();
                    show(projects, project, section).await
                },
            ),
        )
}

async fn show(
    projects: Projects,
    project: Option<ProjectId>,
    section: Option<ProjectSection>,
) -> axum::response::Response {
    let (Some(id), Some(section)) = (project, section) else {
        return missing();
    };
    let available = projects.root.is_some();
    match projects
        .access(|directory| Ok(directory.load()?.unwrap_or_default()))
        .await
    {
        Ok(catalog) => {
            let summaries = summaries(&catalog);
            match summaries.iter().find(|project| project.id == id.to_bytes()) {
                Some(project) => (
                    [(CACHE_CONTROL, "no-store")],
                    Html(devoption_ui::render_project_section(
                        project, available, section,
                    )),
                )
                    .into_response(),
                None => missing(),
            }
        }
        Err(error) => (
            Failure::from(error).status,
            [(CACHE_CONTROL, "no-store")],
            "Project could not be loaded. Reload to try again.",
        )
            .into_response(),
    }
}

fn missing() -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        [(CACHE_CONTROL, "no-store")],
        "Project does not exist.",
    )
        .into_response()
}
