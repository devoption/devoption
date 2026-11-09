//! Explicit owner review of a successful retained build before private staging.
use super::*;
use devoption_builds::{
    history::History,
    jobs::JobId,
    library::{cargo_package::CargoPackage, staging::StageOutcome},
    source_snapshot::registered,
};
use devoption_projects::RepositoryId;

pub(super) struct Review {
    candidate: CargoPackage,
    catalog: Catalog,
    attempt: JobId,
}

pub(super) fn routes(projects: Projects) -> Router {
    let mut router = Router::new();
    for (suffix, approve) in [("prepare", false), ("approve", true)] {
        let p = projects.clone();
        router = router.route(
            &format!("/projects/{{project}}/builds/attempts/{{attempt}}/registry/{suffix}"),
            post(
                move |Extension(identity): Extension<IdentityId>,
                      RoutePath((project, attempt)): RoutePath<(String, String)>,
                      request: Request| {
                    handle(p, identity, project, attempt, request, approve)
                },
            ),
        );
    }
    router
}

async fn handle(
    projects: Projects,
    identity: IdentityId,
    project: String,
    attempt: String,
    request: Request,
    approve: bool,
) -> axum::response::Response {
    if projects.owner != Some(identity) {
        return failure(StatusCode::FORBIDDEN);
    }
    let Some((project, attempt)) = project_id(&project).zip(project_id(&attempt)) else {
        return failure(StatusCode::NOT_FOUND);
    };
    let attempt = JobId::from_bytes(attempt.to_bytes());
    let (Some(execution), Some(history), Some(engine), Some(root), Some(session)) = (
        projects.execution.clone(),
        projects.history.clone(),
        projects.registry.clone(),
        projects.root.clone(),
        projects.registry_session.clone(),
    ) else {
        return failure(StatusCode::SERVICE_UNAVAILABLE);
    };
    // Match build/retention lock order, rejecting contention before reading a body.
    // Guards move into the blocking operation and survive client disconnects.
    let Ok(execution) = execution.try_lock_owned() else {
        return failure(StatusCode::CONFLICT);
    };
    if !execution.staging_available() {
        return failure(StatusCode::CONFLICT);
    }
    let Ok(history) = history.try_lock_owned() else {
        return failure(StatusCode::CONFLICT);
    };
    let Ok(mut engine) = engine.try_lock_owned() else {
        return failure(StatusCode::CONFLICT);
    };
    let headers = request.headers().clone();
    let body = match text_body(request, 32).await {
        Ok(body) => body,
        Err(status) => return failure(status),
    };
    let Ok(gate) = projects.gate.clone().try_lock_owned() else {
        return failure(StatusCode::CONFLICT);
    };
    let result = tokio::task::spawn_blocking(move || -> Result<Value, StatusCode> {
        let _gate = gate;
        if crate::browser_session::authenticate(&headers, &session) != Some(identity) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        engine
            .root
            .verify()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let writer = projects.directory.writer().map_err(|error| match error {
            devoption_projects::Error::Busy => StatusCode::CONFLICT,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        })?;
        let catalog = writer
            .load()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
            .unwrap_or_default();
        let _retention = execution.config.retention_lock().map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                StatusCode::CONFLICT
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            }
        })?;
        Selection {
            catalog: &catalog,
            root: &root,
            history: &history,
            work: execution.config.work_root(),
            project,
            attempt,
        }
        .operate(&mut engine, approve, &body, || {
            crate::browser_session::authenticate(&headers, &session) == Some(identity)
        })
    })
    .await;
    match result {
        Ok(Ok(value)) => {
            let Ok(bytes) = serde_json::to_vec(&value) else {
                return failure(StatusCode::SERVICE_UNAVAILABLE);
            };
            (
                [
                    (CACHE_CONTROL, "no-store"),
                    (header::CONTENT_TYPE, "application/json"),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                ],
                bytes,
            )
                .into_response()
        }
        Ok(Err(status)) => failure(status),
        Err(_) => failure(StatusCode::SERVICE_UNAVAILABLE),
    }
}

struct Selection<'a> {
    catalog: &'a Catalog,
    root: &'a PermittedRoot,
    history: &'a History,
    work: &'a Path,
    project: ProjectId,
    attempt: JobId,
}
impl Selection<'_> {
    fn candidate(&self, engine: &Engine) -> Result<CargoPackage, StatusCode> {
        let Self {
            catalog,
            root,
            history,
            work,
            project,
            attempt,
        } = *self;
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
        let repository = RepositoryId::from_bytes(record.repository());
        catalog
            .selected_binding_bytes(project, repository)
            .map_err(|_| StatusCode::NOT_FOUND)?;
        // No reset/fetch/checkout: staging an older attempt needs its exact clean
        // registered revision. The package then remains independent of that checkout.
        let source = registered::capture(
            catalog,
            project,
            repository,
            root,
            record.commit(),
            devoption_builds::execution::registry(),
        )
        .map_err(|_| StatusCode::CONFLICT)?;
        let package = history
            .library_candidate(attempt, work, engine.root.binding.clone(), &source)
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
            .ok_or(StatusCode::NOT_FOUND)?;
        Ok(package)
    }

    fn operate(
        &self,
        engine: &mut Engine,
        approve: bool,
        body: &str,
        authenticated: impl Fn() -> bool,
    ) -> Result<Value, StatusCode> {
        let Self {
            project, attempt, ..
        } = *self;
        if !approve {
            engine.pending = None;
        }
        if engine.policy.is_none() {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        if approve {
            // Every approval attempt consumes the review, including mismatches.
            let pending = engine.pending.take().ok_or(StatusCode::CONFLICT)?;
            if pending.expires <= Instant::now()
                || body != pending.token
                || pending.project != Some(project)
            {
                return Err(StatusCode::CONFLICT);
            }
            let Decision::Ingestion(review) = pending.decision else {
                return Err(StatusCode::CONFLICT);
            };
            if review.attempt != attempt || *self.catalog != review.catalog {
                return Err(StatusCode::CONFLICT);
            }
            let current = self.candidate(engine)?;
            if current != review.candidate || pending.expires <= Instant::now() {
                return Err(StatusCode::CONFLICT);
            }
            engine
                .root
                .verify()
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            if !authenticated() {
                return Err(StatusCode::UNAUTHORIZED);
            }
            let outcome = engine.candidates.stage(&current).map_err(stage_error)?;
            return Ok(
                json!({"staged":true,"already_present":outcome == StageOutcome::AlreadyPresent}),
            );
        }
        if !body.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        let candidate = self.candidate(engine)?;
        let coordinates = candidate.source().coordinates().clone();
        if engine
            .candidates
            .read(&coordinates)
            .map_err(stage_error)?
            .is_some_and(|stored| stored != candidate)
        {
            return Err(StatusCode::CONFLICT);
        }
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let token = hex(&nonce);
        let source = candidate.source();
        let receipt = candidate
            .build_provenance()
            .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        let value = json!({
            "review_token": token, "expires_in_seconds": 300, "change": {"action":"stage"},
            "attempt": hex(&attempt.to_bytes()),
            "registry": coordinates.registry().id().as_str(), "index": coordinates.registry().index(),
            "name": coordinates.name(), "version": coordinates.version(),
            "source_project": hex(&source.project()), "repository": hex(&source.repository()),
            "commit": source.commit(), "source_sha256": hex(source.source_sha256().bytes()),
            "source_archive_sha256": hex(source.archive_sha256().bytes()),
            "archive_sha256": hex(candidate.archive_sha256().bytes()), "archive_bytes": candidate.archive().len(),
            "index_sha256": hex(Sha256::digest(candidate.index_line()).bytes()),
            "receipt_sha256": hex(Sha256::digest(receipt.receipt()).bytes()),
        });
        engine
            .root
            .verify()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if !authenticated() {
            return Err(StatusCode::UNAUTHORIZED);
        }
        engine.pending = Some(Pending {
            token,
            expires: Instant::now() + Duration::from_secs(300),
            project: Some(project),
            coordinates: Some(coordinates),
            decision: Decision::Ingestion(Box::new(Review {
                candidate,
                catalog: self.catalog.clone(),
                attempt,
            })),
        });
        Ok(value)
    }
}
fn stage_error(error: devoption_builds::library::staging::Error) -> StatusCode {
    use devoption_builds::library::staging::Error;
    match error {
        Error::Invalid => StatusCode::BAD_REQUEST,
        Error::Conflict | Error::Capacity => StatusCode::CONFLICT,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}

#[cfg(test)]
mod tests;
