//! Opt-in owner-only registry JSON controls inside page/session/origin guards.
//! This exposes only selected staged candidates; no upload, build or public Cargo API.
use super::*;
use axum::{extract::Extension, http::header, routing::post};
use devoption_builds::{
    approvals::OwnerId,
    library::{
        package_store::PackageStore,
        publication::{self, Change, Publications, RecoveryReview, Review},
        staging::Coordinates,
    },
};
use devoption_registry::{HostedRegistry, RegistryId, Sha256};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
mod ingestion;
mod storage;
use storage::Root;

pub(in crate::server) struct Engine {
    root: Root,
    candidates: PackageStore,
    policy: Option<Publications>,
    pending: Option<Pending>,
    owner: OwnerId,
}
enum Decision {
    Normal(Review),
    Recovery(RecoveryReview),
    Ingestion(Box<ingestion::Review>),
}
struct Pending {
    token: String,
    expires: Instant,
    project: Option<ProjectId>,
    coordinates: Option<Coordinates>,
    decision: Decision,
}
impl Engine {
    pub(in crate::server) fn open(
        path: &Path,
        owner: IdentityId,
        catalog_directory: &CatalogDirectory,
        forbidden: &[std::path::PathBuf],
    ) -> io::Result<Self> {
        let work = || -> Result<Self, ()> {
            let root = Root::open(path, catalog_directory, forbidden)?;
            let candidates = PackageStore::open(&root.candidates).map_err(|_| ())?;
            let owner = OwnerId::from_bytes(owner.to_bytes());
            let catalog = catalog_directory
                .load()
                .map_err(|_| ())?
                .unwrap_or_default();
            let policy = match Publications::open(&root.policy, owner) {
                Ok(policy) => {
                    policy.check_registry(&root.binding).map_err(|_| ())?;
                    Some(policy)
                }
                Err(_) => {
                    let recovery = Publications::review_recovery(
                        &root.policy,
                        owner,
                        owner,
                        &catalog,
                        &candidates,
                    )
                    .map_err(|_| ())?;
                    if recovery.check_registry(&root.binding).is_err() {
                        return Err(());
                    }
                    None
                }
            };
            root.verify()?;
            Ok(Self {
                root,
                candidates,
                policy,
                pending: None,
                owner,
            })
        };
        work()
            .map_err(|_| io::Error::other("package registry configuration or storage unavailable"))
    }
    fn source(
        &self,
        catalog: &Catalog,
        project: ProjectId,
        c: &Coordinates,
    ) -> Result<(), StatusCode> {
        let package = self
            .candidates
            .read(c)
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
            .ok_or(StatusCode::NOT_FOUND)?;
        if package.source().project() != project.to_bytes()
            || !catalog.projects().iter().any(|p| p.id() == project)
        {
            return Err(StatusCode::NOT_FOUND);
        }
        Ok(())
    }
    fn save(
        &mut self,
        project: Option<ProjectId>,
        coordinates: Option<Coordinates>,
        decision: Decision,
    ) -> Result<Value, StatusCode> {
        let mut token = [0u8; 16];
        getrandom::fill(&mut token).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let token = hex(&token);
        let (review, linked) = match &decision {
            Decision::Normal(r) => (r, None),
            Decision::Recovery(r) => (r.decision(), Some(r.final_link_present())),
            Decision::Ingestion(_) => return Err(StatusCode::CONFLICT),
        };
        let candidate = review.candidate();
        let source = candidate.source();
        let c = source.coordinates();
        let change = match review.change() {
            Change::Publish { readers } => {
                json!({"action":"publish","readers":readers.iter().map(|id|hex(&id.to_bytes())).collect::<Vec<_>>()})
            }
            Change::Readers { readers } => {
                json!({"action":"readers","readers":readers.iter().map(|id|hex(&id.to_bytes())).collect::<Vec<_>>()})
            }
            Change::Yank { yanked } => json!({"action":"yank","yanked":yanked}),
        };
        let body = json!({"review_token":token,"expires_in_seconds":300,"registry":c.registry().id().as_str(),"index":c.registry().index(),"name":c.name(),"version":c.version(),"source_project":hex(&source.project()),"repository":hex(&source.repository()),"commit":source.commit(),"source_sha256":hex(source.source_sha256().bytes()),"candidate_sha256":hex(review.candidate_sha256().bytes()),"archive_sha256":hex(candidate.archive_sha256().bytes()),"archive_bytes":candidate.archive().len(),"change":change,"sequence":review.sequence(),"remaining_events_after_approval":review.remaining_capacity(),"final_link_present":linked});
        self.pending = Some(Pending {
            token,
            expires: Instant::now() + Duration::from_secs(300),
            project,
            coordinates,
            decision,
        });
        Ok(body)
    }
    fn reopen(&mut self) {
        self.policy = None;
        self.policy = Publications::open(&self.root.policy, self.owner)
            .ok()
            .filter(|policy| policy.check_registry(&self.root.binding).is_ok());
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn failure(status: StatusCode) -> axum::response::Response {
    (
        status,
        [
            (CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        "Registry action unavailable. Refresh status before retrying.",
    )
        .into_response()
}
fn mapped(e: publication::Error) -> StatusCode {
    match e {
        publication::Error::Denied => StatusCode::NOT_FOUND,
        publication::Error::Invalid => StatusCode::BAD_REQUEST,
        publication::Error::Changed | publication::Error::Conflict => StatusCode::CONFLICT,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
enum Input {
    Publish { readers: Vec<String> },
    Readers { readers: Vec<String> },
    Yank { yanked: bool },
}
impl Input {
    fn change(self) -> Result<Change, StatusCode> {
        let parse = |values: Vec<String>| {
            values
                .into_iter()
                .map(|id| project_id(&id).ok_or(StatusCode::BAD_REQUEST))
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(match self {
            Self::Publish { readers } => Change::Publish {
                readers: parse(readers)?,
            },
            Self::Readers { readers } => Change::Readers {
                readers: parse(readers)?,
            },
            Self::Yank { yanked } => Change::Yank { yanked },
        })
    }
}
#[derive(Clone, Copy)]
enum Action {
    Status,
    Prepare,
    Approve,
    Index,
    Archive,
    RecoveryPrepare,
    RecoveryApprove,
}
pub(super) fn routes(projects: Projects) -> Router {
    let mut router = Router::new();
    for (suffix, action, write) in [
        ("status", Action::Status, false),
        ("prepare", Action::Prepare, true),
        ("approve", Action::Approve, true),
        ("index", Action::Index, false),
    ] {
        let p = projects.clone();
        let path = format!("/projects/{{project}}/registry/{{name}}/{{version}}/{suffix}");
        let handler =
            move |Extension(identity): Extension<IdentityId>,
                  RoutePath((project, name, version)): RoutePath<(String, String, String)>,
                  request: Request| {
                handle(
                    p,
                    identity,
                    Some((project, name, version)),
                    None,
                    action,
                    request,
                )
            };
        router = router.route(&path, if write { post(handler) } else { get(handler) });
    }
    let p = projects.clone();
    router = router.route(
        "/projects/{project}/registry/{name}/{version}/archive/{checksum}",
        get(
            move |Extension(identity): Extension<IdentityId>,
                  RoutePath((project, name, version, checksum)): RoutePath<(
                String,
                String,
                String,
                String,
            )>,
                  request: Request| {
                handle(
                    p,
                    identity,
                    Some((project, name, version)),
                    Some(checksum),
                    Action::Archive,
                    request,
                )
            },
        ),
    );
    for (path, action) in [
        (
            "/package-registry/recovery/prepare",
            Action::RecoveryPrepare,
        ),
        (
            "/package-registry/recovery/approve",
            Action::RecoveryApprove,
        ),
    ] {
        let p = projects.clone();
        router = router.route(
            path,
            post(
                move |Extension(identity): Extension<IdentityId>, request: Request| {
                    handle(p, identity, None, None, action, request)
                },
            ),
        );
    }
    router.merge(ingestion::routes(projects))
}
async fn handle(
    projects: Projects,
    identity: IdentityId,
    coordinates: Option<(String, String, String)>,
    checksum: Option<String>,
    action: Action,
    request: Request,
) -> axum::response::Response {
    if projects.owner != Some(identity) {
        return failure(StatusCode::FORBIDDEN);
    }
    let (Some(engine), Some(session)) = (projects.registry, projects.registry_session) else {
        return failure(StatusCode::SERVICE_UNAVAILABLE);
    };
    let Ok(mut engine) = engine.try_lock_owned() else {
        return failure(StatusCode::CONFLICT);
    };
    let headers = request.headers().clone();
    let write = matches!(
        action,
        Action::Prepare | Action::Approve | Action::RecoveryPrepare | Action::RecoveryApprove
    );
    let body = if write {
        match text_body(request, 8192).await {
            Ok(body) => body,
            Err(status) => return failure(status),
        }
    } else {
        String::new()
    };
    let Ok(catalog_gate) = projects.gate.try_lock_owned() else {
        return failure(StatusCode::CONFLICT);
    };
    let result = tokio::task::spawn_blocking(
        move || -> Result<(Vec<u8>, &'static str, bool), StatusCode> {
            let _gate = catalog_gate;
            let live = if write {
                crate::browser_session::authenticate(&headers, &session)
            } else {
                crate::browser_session::authenticate_read(&headers, &session)
            };
            if live != Some(identity) {
                return Err(StatusCode::UNAUTHORIZED);
            }
            engine
                .root
                .verify()
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            if engine
                .pending
                .as_ref()
                .is_some_and(|p| p.expires <= Instant::now())
            {
                engine.pending = None;
            }
            // Hold the cross-process catalog writer lease as well as the HTTP gate.
            let catalog_writer = projects
                .directory
                .writer()
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            let catalog = catalog_writer
                .load()
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
                .unwrap_or_default();
            operate(&mut engine, &catalog, coordinates, checksum, action, &body)
        },
    )
    .await;
    match result {
        Ok(Ok((bytes, kind, attachment))) => {
            let mut response = (
                [
                    (CACHE_CONTROL, "no-store"),
                    (header::CONTENT_TYPE, kind),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                ],
                bytes,
            )
                .into_response();
            if attachment {
                response.headers_mut().insert(
                    header::CONTENT_DISPOSITION,
                    header::HeaderValue::from_static("attachment; filename=package.crate"),
                );
            }
            response
        }
        Ok(Err(status)) => failure(status),
        Err(_) => failure(StatusCode::SERVICE_UNAVAILABLE),
    }
}
fn operate(
    engine: &mut Engine,
    catalog: &Catalog,
    coordinates: Option<(String, String, String)>,
    checksum: Option<String>,
    action: Action,
    body: &str,
) -> Result<(Vec<u8>, &'static str, bool), StatusCode> {
    let coordinates = coordinates
        .map(|(p, n, v)| {
            let project = project_id(&p).ok_or(StatusCode::NOT_FOUND)?;
            let c = Coordinates::new(engine.root.binding.clone(), &n, &v)
                .map_err(|_| StatusCode::NOT_FOUND)?;
            Ok::<_, StatusCode>((project, c))
        })
        .transpose()?;
    let data = match action {
        Action::Prepare => {
            engine.pending = None;
            let (project, c) = coordinates.as_ref().ok_or(StatusCode::BAD_REQUEST)?;
            engine.source(catalog, *project, c)?;
            let change = serde_json::from_str::<Input>(body)
                .map_err(|_| StatusCode::BAD_REQUEST)?
                .change()?;
            let review = engine
                .policy
                .as_ref()
                .ok_or(StatusCode::SERVICE_UNAVAILABLE)?
                .review(engine.owner, catalog, &engine.candidates, c, change)
                .map_err(mapped)?;
            engine.save(Some(*project), Some(c.clone()), Decision::Normal(review))?
        }
        Action::RecoveryPrepare => {
            engine.pending = None;
            if !body.is_empty() {
                return Err(StatusCode::BAD_REQUEST);
            }
            if engine.policy.is_some() {
                return Err(StatusCode::CONFLICT);
            }
            let review = Publications::review_recovery(
                &engine.root.policy,
                engine.owner,
                engine.owner,
                catalog,
                &engine.candidates,
            )
            .map_err(mapped)?;
            if review.decision().coordinates().registry() != &engine.root.binding {
                return Err(StatusCode::CONFLICT);
            }
            engine.save(None, None, Decision::Recovery(review))?
        }
        Action::Approve | Action::RecoveryApprove => {
            let pending = engine.pending.take().ok_or(StatusCode::CONFLICT)?;
            if body != pending.token.as_str()
                || pending.project != coordinates.as_ref().map(|(p, _)| *p)
                || pending.coordinates.as_ref() != coordinates.as_ref().map(|(_, c)| c)
            {
                return Err(StatusCode::CONFLICT);
            }
            let owner = engine.owner;
            let Engine {
                policy,
                candidates,
                root,
                ..
            } = &mut *engine;
            let result = match (action, pending.decision) {
                (Action::Approve, Decision::Normal(review)) => policy
                    .as_mut()
                    .ok_or(StatusCode::SERVICE_UNAVAILABLE)?
                    .approve(owner, catalog, candidates, review),
                (Action::RecoveryApprove, Decision::Recovery(review)) => {
                    Publications::approve_recovery(
                        &root.policy,
                        owner,
                        owner,
                        catalog,
                        candidates,
                        review,
                    )
                }
                _ => return Err(StatusCode::CONFLICT),
            };
            engine.reopen();
            result.map_err(mapped)?;
            json!({"applied":true})
        }
        Action::Status => {
            let (project, c) = coordinates.as_ref().ok_or(StatusCode::BAD_REQUEST)?;
            engine.source(catalog, *project, c)?;
            let s = engine
                .policy
                .as_ref()
                .ok_or(StatusCode::SERVICE_UNAVAILABLE)?
                .status(engine.owner, catalog, &engine.candidates, c)
                .map_err(mapped)?;
            json!({"registry":c.registry().id().as_str(),"index":c.registry().index(),"name":c.name(),"version":c.version(),"source_project":hex(&s.source.to_bytes()),"repository":hex(&s.repository.to_bytes()),"commit":s.commit,"source_sha256":hex(s.source_sha256.bytes()),"candidate_sha256":hex(s.candidate_sha256.bytes()),"archive_sha256":hex(s.archive_sha256.bytes()),"archive_bytes":s.archive_bytes,"readers":s.readers.iter().map(|id|hex(&id.to_bytes())).collect::<Vec<_>>(),"yanked":s.yanked,"events_used":s.events_used,"event_capacity":s.event_capacity})
        }
        Action::Index | Action::Archive => {
            let (project, c) = coordinates.as_ref().ok_or(StatusCode::BAD_REQUEST)?;
            let policy = engine
                .policy
                .as_ref()
                .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
            if matches!(action, Action::Index) {
                return Ok((
                    policy
                        .index_line(engine.owner, *project, catalog, &engine.candidates, c)
                        .map_err(mapped)?,
                    "application/json",
                    false,
                ));
            }
            let expected = Sha256::from_hex(checksum.as_deref().ok_or(StatusCode::BAD_REQUEST)?)
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            return Ok((
                policy
                    .archive(
                        engine.owner,
                        *project,
                        catalog,
                        &engine.candidates,
                        c,
                        expected,
                    )
                    .map_err(mapped)?,
                "application/octet-stream",
                true,
            ));
        }
    };
    Ok((
        serde_json::to_vec(&data).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?,
        "application/json",
        false,
    ))
}

#[cfg(test)]
mod tests;
