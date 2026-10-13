//! Authenticated read-only working-tree previews; no caller-selected root.
use super::*;
use devoption_projects::RepositoryId;
use devoption_projects::worktree::{FileContent, RepositoryFiles};
use devoption_ui::{FileEntry, FilePreview, RepositoryFilesView};

pub(super) fn routes(projects: Projects) -> Router {
    let list = projects.clone();
    Router::new()
        .route(
            "/projects/{project}/repositories/{repository}/files",
            get(
                move |ids: Result<RoutePath<(String, String)>, PathRejection>| async move {
                    match ids {
                        Ok(RoutePath((project, repository))) => {
                            show(list, project, repository, None).await
                        }
                        Err(_) => missing(),
                    }
                },
            ),
        )
        .route(
            "/projects/{project}/repositories/{repository}/files/{*path}",
            get(
                move |ids: Result<RoutePath<(String, String, String)>, PathRejection>| async move {
                    match ids {
                        Ok(RoutePath((project, repository, path))) => {
                            show(projects, project, repository, Some(path)).await
                        }
                        Err(_) => missing(),
                    }
                },
            ),
        )
}

async fn show(
    projects: Projects,
    project: String,
    repository: String,
    path: Option<String>,
) -> axum::response::Response {
    let Some((project, repository)) = project_id(&project)
        .zip(project_id(&repository).map(|id| RepositoryId::from_bytes(id.to_bytes())))
    else {
        return missing();
    };
    let root = projects.root.clone();
    let loaded = projects
        .access(move |directory| {
            let catalog = directory.load()?.unwrap_or_default();
            let record = catalog
                .projects()
                .iter()
                .find(|p| p.id() == project)
                .ok_or(Error::UnknownProject)?;
            if !record.repositories().contains(&repository) {
                return Err(Error::UnknownRepository);
            }
            let files = root
                .as_ref()
                .ok_or(Error::CheckoutRequired)
                .and_then(|root| {
                    catalog.repository_files(project, repository, root, path.as_deref())
                });
            Ok((catalog, files))
        })
        .await;
    let (catalog, result) = match loaded {
        Ok(value) => value,
        Err(Error::UnknownProject | Error::UnknownRepository) => return missing(),
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                [(CACHE_CONTROL, "no-store")],
                "Files could not be loaded.",
            )
                .into_response();
        }
    };
    let projects = summaries(&catalog);
    let Some(project) = projects.iter().find(|p| p.id == project.to_bytes()) else {
        return missing();
    };
    let Some(repository) = project
        .repositories
        .iter()
        .find(|r| r.id == repository.to_bytes())
    else {
        return missing();
    };
    let (status, unavailable) = match &result {
        Ok(_) => (StatusCode::OK, None),
        Err(Error::Inspection(InspectionError::InvalidPath | InspectionError::OutsideRoot)) => {
            return missing();
        }
        Err(
            Error::RootMismatch
            | Error::StaleCheckout
            | Error::Inspection(InspectionError::Changed),
        ) => (
            StatusCode::CONFLICT,
            Some(
                "The checkout changed. Return to Code and inspect its attachment before retrying.",
            ),
        ),
        Err(Error::Inspection(InspectionError::LimitExceeded)) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Some("Files exceed the inspection limits. At most 256 tracked paths can be listed."),
        ),
        Err(Error::Inspection(InspectionError::UnsupportedLayout)) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Some("This repository layout or index is not supported for file browsing."),
        ),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Some("Files could not be loaded. Check repository storage and try again."),
        ),
    };
    let empty = RepositoryFiles {
        entries: Vec::new(),
        selected: None,
    };
    let files = result.as_ref().unwrap_or(&empty);
    let selected = files.selected.as_ref().map(|(path, content)| {
        let (text, message) = match content {
            FileContent::Text(text) => (Some(text.as_str()), ""),
            FileContent::Binary => (None, "Binary files and text with unsupported control characters cannot be displayed."),
            FileContent::TooLarge => (None, "This file is too large to display. Text previews are limited to 64 KiB."),
            FileContent::Unsupported => (None, "Symlinks, submodules and sparse directory entries cannot be opened."),
            FileContent::Unavailable => (None, "This tracked file is missing, changed or unavailable. Its contents were not displayed."),
        };
        FilePreview { path, text, message }
    });
    let view = RepositoryFilesView {
        repository_id: repository.id,
        repository_name: repository.name,
        entries: files
            .entries
            .iter()
            .map(|entry| FileEntry { path: &entry.path })
            .collect(),
        selected,
        unavailable,
    };
    (
        status,
        [(CACHE_CONTROL, "no-store")],
        Html(devoption_ui::render_repository_files(project, &view)),
    )
        .into_response()
}

fn missing() -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        [(CACHE_CONTROL, "no-store")],
        "Tracked file or repository does not exist.",
    )
        .into_response()
}
