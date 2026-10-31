use super::*;
use axum::routing::post;
use devoption_builds::{
    approvals::{Approvals, OwnerId},
    execution::{Configuration, Review},
    source_snapshot::registered,
};
use devoption_projects::RepositoryId;
use std::sync::atomic::{AtomicBool, Ordering};

type Admission = (
    Review,
    devoption_builds::source_snapshot::SourceSnapshot,
    OwnerId,
    Approvals,
    Arc<AtomicBool>,
);

pub(super) struct Engine {
    config: Configuration,
    owner: OwnerId,
    approvals: Approvals,
    pending: Option<Review>,
    active: Option<(ProjectId, RepositoryId)>,
    cancel: Arc<AtomicBool>,
    blocked: bool,
}
impl Engine {
    pub(super) fn new(config: Configuration, owner: IdentityId) -> Self {
        Self {
            config,
            owner: OwnerId::from_bytes(owner.to_bytes()),
            approvals: Approvals::default(),
            pending: None,
            active: None,
            cancel: Arc::new(AtomicBool::new(false)),
            blocked: false,
        }
    }
}
fn ids(p: &str, r: &str) -> Option<(ProjectId, RepositoryId)> {
    Some((project_id(p)?, RepositoryId::from_hex(r)?))
}
fn failure(status: StatusCode) -> axum::response::Response {
    (status,[(CACHE_CONTROL,"no-store")],"Build action unavailable. Refresh the build page and check the saved attempt before retrying.").into_response()
}

pub(super) fn routes(projects: Projects) -> Router {
    let page = projects.clone();
    let prepare = projects.clone();
    let approve = projects.clone();
    Router::new()
        .route(
            "/projects/{project}/builds/{repository}/run",
            get(
                move |RoutePath((p, r)): RoutePath<(String, String)>| async move {
                    match ids(&p, &r) {
                        Some((p, r)) => show(page, p, r).await,
                        None => failure(StatusCode::NOT_FOUND),
                    }
                },
            ),
        )
        .route(
            "/projects/{project}/builds/{repository}/prepare",
            post(
                move |RoutePath((p, r)): RoutePath<(String, String)>, request: Request| async move {
                    action(prepare, &p, &r, request, false).await
                },
            ),
        )
        .route(
            "/projects/{project}/builds/{repository}/approve",
            post(
                move |RoutePath((p, r)): RoutePath<(String, String)>, request: Request| async move {
                    action(approve, &p, &r, request, true).await
                },
            ),
        )
        .route(
            "/projects/{project}/builds/{repository}/cancel",
            post(
                move |RoutePath((p, r)): RoutePath<(String, String)>, request: Request| async move {
                    let Some((p, r)) = ids(&p, &r) else {
                        return failure(StatusCode::NOT_FOUND);
                    };
                    if text_body(request, 0).await.is_err() {
                        return failure(StatusCode::BAD_REQUEST);
                    }
                    let Some(engine) = projects.execution else {
                        return failure(StatusCode::SERVICE_UNAVAILABLE);
                    };
                    let engine = engine.lock().await;
                    if engine.active != Some((p, r)) {
                        return failure(StatusCode::CONFLICT);
                    }
                    engine.cancel.store(true, Ordering::SeqCst);
                    StatusCode::ACCEPTED.into_response()
                },
            ),
        )
}

async fn show(
    projects: Projects,
    project: ProjectId,
    repository: RepositoryId,
) -> axum::response::Response {
    let loaded = projects
        .clone()
        .access(move |directory| {
            let catalog = directory.load()?.unwrap_or_default();
            if !catalog
                .projects()
                .iter()
                .any(|p| p.id() == project && p.repositories().contains(&repository))
            {
                return Err(Error::UnknownRepository);
            }
            Ok(catalog)
        })
        .await;
    let Ok(catalog) = loaded else {
        return failure(StatusCode::NOT_FOUND);
    };
    let summaries = summaries(&catalog);
    let project_view = summaries
        .iter()
        .find(|p| p.id == project.to_bytes())
        .expect("checked project");
    let mut view = devoption_ui::BuildExecutionView {
        repository: repository.to_bytes(),
        token: None,
        review: Vec::new(),
        running: false,
        available: projects.execution.is_some(),
        message: "Install and explicitly enable the Linux build runner to build this app.".into(),
    };
    if let Some(engine) = projects.execution {
        let engine = engine.lock().await;
        view.running = engine.active == Some((project, repository));
        view.available = !engine.blocked && engine.active.is_none();
        view.message = if engine.blocked {
            "Build continuity needs inspection. Restart the service to reconcile the previous unit."
        } else if view.running {
            "Build running. Open build history for saved output, or request cancellation."
        } else if engine.active.is_some() {
            "Another build is running. Only one build can run at a time."
        } else {
            "Prepare the committed web app for review. Preparation does not run Cargo."
        }
        .into();
        if let Some(review) = engine.pending.as_ref().filter(|r| {
            r.job.plan().details().project == project
                && r.job.plan().details().repository == repository
        }) && !review.expired()
        {
            view.message =
                "The source snapshot is prepared. Review the details before approving.".into();
            view.token = Some(review.token().into());
            let plan = review.job.plan();
            let context = review.context().details();
            view.review=vec![("Build limits".into(),"1 CPU · 512 MiB memory · 60-second limit".into()),("Network access".into(),"Off".into()),("Commit".into(),plan.details().commit.clone()),("Command inside isolated worker".into(),format!("{} {}",plan.details().cargo_executable,plan.cargo_arguments().join(" "))),("Working directory".into(),"/source (read-only owned snapshot)".into()),("Artifact".into(),plan.artifact_path()),("Policy".into(),"WasmBuildV1: 60 seconds; 512 MiB RAM; no swap; one CPU; 64 tasks; 256 MiB job storage (240 MiB scratch; 16 MiB reserved for staged inputs, artifact and logs); 1 MiB logs; 4 MiB component. No network. Target scratch permits dependency build scripts.".into()),("Toolchain SHA-256".into(),hex(context.toolchain_manifest.bytes())),("Dependency bundle SHA-256".into(),hex(context.dependency_bundle.bytes())),("Source SHA-256".into(),hex(plan.details().source.sha256().bytes())),("Configuration SHA-256".into(),hex(context.configuration.bytes())),("Execution UID".into(),context.execution_uid.to_string()),("Environment (all values)".into(),plan.details().environment.iter().map(|e|format!("{}={}",e.name,e.value)).collect::<Vec<_>>().join("\n"))];
        }
    }
    (
        [(CACHE_CONTROL, "no-store")],
        Html(devoption_ui::render_build_execution(project_view, &view)),
    )
        .into_response()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn action(
    projects: Projects,
    p: &str,
    r: &str,
    request: Request,
    approve: bool,
) -> axum::response::Response {
    let Some((project, repository)) = ids(p, r) else {
        return failure(StatusCode::NOT_FOUND);
    };
    let body = match text_body(request, 32).await {
        Ok(body) => body,
        Err(status) => return failure(status),
    };
    if (!approve && !body.is_empty())
        || (approve
            && (body.len() != 32
                || !body
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())))
    {
        return failure(StatusCode::BAD_REQUEST);
    }
    let (Some(engine), Some(history), Some(root)) = (
        projects.execution.clone(),
        projects.history.clone(),
        projects.root.clone(),
    ) else {
        return failure(StatusCode::SERVICE_UNAVAILABLE);
    };
    let mut engine_guard = engine.clone().lock_owned().await;
    if engine_guard.active.is_some() || engine_guard.blocked {
        return failure(StatusCode::CONFLICT);
    }
    let mut history_guard = history.clone().lock_owned().await;
    let catalog_guard = projects.gate.clone().lock_owned().await;
    let result = tokio::task::spawn_blocking(move || -> Result<Option<Admission>, StatusCode> {
        let _catalog_guard = catalog_guard;
        let catalog = projects
            .directory
            .load()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
            .unwrap_or_default();
        let observed = catalog
            .source_inputs(project, repository, &root)
            .map_err(|_| StatusCode::CONFLICT)?;
        let commit = observed.status.commit.ok_or(StatusCode::CONFLICT)?;
        let source = registered::capture(
            &catalog,
            project,
            repository,
            &root,
            &commit,
            devoption_builds::execution::registry(),
        )
        .map_err(|_| StatusCode::CONFLICT)?;
        if approve {
            let review = engine_guard.pending.as_ref().ok_or(StatusCode::CONFLICT)?;
            let plan = review.job.plan().details();
            let binding = catalog
                .selected_binding_bytes(project, repository)
                .map_err(|_| StatusCode::CONFLICT)?;
            if !review.matches_binding(&binding)
                || review.token() != body
                || review.expired()
                || plan.project != project
                || plan.repository != repository
                || plan.source != source
            {
                return Err(StatusCode::CONFLICT);
            }
            let review = engine_guard.pending.take().expect("checked pending");
            engine_guard.active = Some((project, repository));
            engine_guard.cancel = Arc::new(AtomicBool::new(false));
            Ok(Some((
                review,
                source,
                engine_guard.owner,
                engine_guard.approvals.clone(),
                engine_guard.cancel.clone(),
            )))
        } else {
            if let Some(previous) = engine_guard.pending.take() {
                previous
                    .discard(&mut history_guard)
                    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            }
            let review = devoption_builds::execution::prepare(
                &engine_guard.config,
                &catalog,
                project,
                repository,
                source,
                &mut history_guard,
            )
            .map_err(|e| {
                eprintln!("Build preparation failed: {e}");
                StatusCode::CONFLICT
            })?;
            engine_guard.pending = Some(review);
            Ok(None)
        }
    })
    .await;
    match result {
        Ok(Ok(Some((review, source, owner, approvals, cancel)))) => {
            tokio::task::spawn_blocking(move || {
                let result = devoption_builds::execution::execute(
                    review,
                    source,
                    owner,
                    &approvals,
                    cancel,
                    |job, output| {
                        let mut history = history.blocking_lock();
                        history.checkpoint(job, output).map_err(io::Error::other)
                    },
                );
                let mut engine = engine.blocking_lock();
                engine.active = None;
                if let Err(error) = &result {
                    eprintln!("Build supervision stopped: {error}");
                }
                engine.blocked = result.is_err();
            });
            StatusCode::ACCEPTED.into_response()
        }
        Ok(Ok(None)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(status)) => failure(status),
        Err(_) => failure(StatusCode::SERVICE_UNAVAILABLE),
    }
}
