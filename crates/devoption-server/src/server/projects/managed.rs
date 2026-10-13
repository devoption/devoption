//! Create a repository only with the separately configured managed-root capability.
use super::*;

pub(super) fn routes(projects: Projects) -> Router {
    Router::new().route("/projects/{project}/repositories/create", axum::routing::post(move |project: Result<RoutePath<String>, PathRejection>, request: Request| async move {
        let result = async {
            let project = project.ok().and_then(|RoutePath(value)| project_id(&value)).ok_or(StatusCode::NOT_FOUND)?;
            let root = projects.managed.clone().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
            let name = super::text_body(request, 64).await?;
            let repository = projects.access(move |directory| directory.writer()?.create_managed_repository(project, &root, &name)).await.map_err(|error| match error {
                Error::InvalidName => StatusCode::BAD_REQUEST,
                Error::UnknownProject => StatusCode::NOT_FOUND,
                Error::RepositoryExists | Error::Capacity => StatusCode::CONFLICT,
                _ => StatusCode::SERVICE_UNAVAILABLE,
            })?;
            let hex = |bytes: [u8; 16]| bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            Ok::<_, StatusCode>(format!("/projects/{}/code#repository-{}", hex(project.to_bytes()), hex(repository.to_bytes())))
        }.await;
        match result {
            Ok(location) => (StatusCode::NO_CONTENT, [(CACHE_CONTROL, "no-store"), (LOCATION, location.as_str())]).into_response(),
            Err(status) => (status, [(CACHE_CONTROL, "no-store")], match status {
                StatusCode::BAD_REQUEST => "Use a repository name with letters, numbers, hyphens or underscores, starting with a letter or number.",
                StatusCode::CONFLICT => "That path already exists or the catalog is full. Existing files were not changed. Reload Code to inspect saved state.",
                StatusCode::NOT_FOUND => "Project does not exist.",
                StatusCode::PAYLOAD_TOO_LARGE => "Repository name is too long.",
                StatusCode::UNSUPPORTED_MEDIA_TYPE => "Use one plain UTF-8 repository name.",
                StatusCode::REQUEST_TIMEOUT => "Repository request timed out.",
                _ => "Creation could not be confirmed. Reload Code and inspect repository storage before retrying.",
            }).into_response(),
        }
    }))
}
