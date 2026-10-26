//! Values enter only mutation bodies. Reads render metadata, never secret bytes.
use super::*;
use crate::project_secrets::{Environment, Error as SecretError, MAX_VALUE, Store};
use devoption_ui::{EnvironmentSecretsView, SecretEventView, SecretMetadataView};
use serde::Deserialize;
use zeroize::Zeroizing;

pub(super) fn routes(projects: Projects) -> Router {
    let reader = projects.clone();
    let writer = projects.clone();
    Router::new()
        .route(
            "/projects/{project}/environments/{environment}",
            get(
                move |path: Result<RoutePath<(String, String)>, PathRejection>| async move {
                    let Some((project, environment)) = path.ok().and_then(|RoutePath((p, e))| {
                        Some((project_id(&p)?, Environment::parse(&e)?))
                    }) else {
                        return missing();
                    };
                    show(reader, project, environment).await
                },
            ),
        )
        .route(
            "/projects/{project}/environments/{environment}/secrets/{name}",
            axum::routing::post(
                move |path: Result<RoutePath<(String, String, String)>, PathRejection>,
                      request: Request| async move {
                    mutate(writer, path, request, false).await
                },
            ),
        )
        .route(
            "/projects/{project}/environments/{environment}/secrets/{name}/remove",
            axum::routing::post(
                move |path: Result<RoutePath<(String, String, String)>, PathRejection>,
                      request: Request| async move {
                    mutate(projects, path, request, true).await
                },
            ),
        )
}
async fn show(projects: Projects, id: ProjectId, env: Environment) -> axum::response::Response {
    let store = match projects.secrets.clone() {
        Some(s) => Some(s.lock_owned().await),
        None => None,
    };
    let result = projects
        .access(move |directory| {
            let catalog = directory.load()?.unwrap_or_default();
            let summaries = summaries(&catalog);
            let project = summaries
                .iter()
                .find(|p| p.id == id.to_bytes())
                .ok_or(Error::UnknownProject)?;
            let mut view = EnvironmentSecretsView {
                environment: env.segment().into(),
                label: env.label().into(),
                configured: store.is_some(),
                unavailable: false,
                entries: vec![],
                events: vec![],
            };
            if let Some(store) = store {
                match store.list(id, env) {
                    Ok(list) => {
                        view.entries = list
                            .entries
                            .into_iter()
                            .map(|e| SecretMetadataView {
                                name: e.name,
                                revision: e.revision,
                                updated: e.updated,
                            })
                            .collect();
                        view.events = list
                            .events
                            .into_iter()
                            .map(|e| SecretEventView {
                                name: e.name,
                                action: e.action,
                                revision: e.revision,
                                updated: e.updated,
                            })
                            .collect();
                    }
                    Err(_) => view.unavailable = true,
                }
            }
            Ok((
                view.unavailable,
                devoption_ui::render_project_secrets(project, &view),
            ))
        })
        .await;
    match result {
        Ok((unavailable, html)) => (
            if unavailable {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::OK
            },
            [(CACHE_CONTROL, "no-store")],
            Html(html),
        )
            .into_response(),
        Err(Error::UnknownProject) => missing(),
        Err(_) => reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "Secrets could not be loaded.",
        ),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    value: Zeroizing<String>,
    revision: u64,
}
async fn mutate(
    projects: Projects,
    path: Result<RoutePath<(String, String, String)>, PathRejection>,
    request: Request,
    remove: bool,
) -> axum::response::Response {
    let Some((id, env, name)) = path
        .ok()
        .and_then(|RoutePath((p, e, n))| Some((project_id(&p)?, Environment::parse(&e)?, n)))
    else {
        return missing();
    };
    if !crate::project_secrets::valid_name(&name) {
        return reply(StatusCode::BAD_REQUEST, "Secret name is invalid.");
    }
    let body = match text_body(request, 6 * MAX_VALUE + 256).await {
        Ok(v) => Zeroizing::new(v),
        Err(status) => return reply(status, "Secret change was rejected."),
    };
    let input = if remove {
        match body.parse::<u64>() {
            Ok(revision) => Input {
                value: Zeroizing::new(String::new()),
                revision,
            },
            Err(_) => return reply(StatusCode::BAD_REQUEST, "Secret revision is invalid."),
        }
    } else {
        match serde_json::from_str::<Input>(&body) {
            Ok(v) => v,
            Err(_) => return reply(StatusCode::BAD_REQUEST, "Secret input is invalid."),
        }
    };
    // Check even unconfigured projects against a fresh catalog; unknown IDs disclose nothing.
    let store = match projects.secrets.clone() {
        Some(s) => Some(s.lock_owned().await),
        None => None,
    };
    let result = projects
        .access(move |directory| {
            let writer = directory.writer()?;
            let catalog = writer.load()?.unwrap_or_default();
            if !catalog.projects().iter().any(|p| p.id() == id) {
                return Err(Error::UnknownProject);
            }
            let Some(store) = store else {
                return Ok(Err(SecretError::Unavailable));
            };
            Ok(apply(&store, id, env, &name, input, remove))
        })
        .await;
    match result {
        Ok(Ok(())) => reply(StatusCode::NO_CONTENT, ""),
        Ok(Err(e)) => {
            let status = match e {
                SecretError::Invalid => StatusCode::BAD_REQUEST,
                SecretError::Conflict | SecretError::Capacity => StatusCode::CONFLICT,
                _ => StatusCode::SERVICE_UNAVAILABLE,
            };
            reply(status, &e.to_string())
        }
        Err(Error::UnknownProject) => missing(),
        Err(_) => reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "Secret storage is unavailable.",
        ),
    }
}
fn apply(
    store: &Store,
    id: ProjectId,
    env: Environment,
    name: &str,
    input: Input,
    remove: bool,
) -> Result<(), SecretError> {
    if remove {
        store.remove(id, env, name, input.revision)
    } else {
        store.set(id, env, name, input.value, input.revision)
    }
}
fn reply(status: StatusCode, message: &str) -> axum::response::Response {
    (status, [(CACHE_CONTROL, "no-store")], message.to_owned()).into_response()
}
fn missing() -> axum::response::Response {
    reply(
        StatusCode::NOT_FOUND,
        "Project or environment does not exist.",
    )
}
#[cfg(test)]
mod tests;
