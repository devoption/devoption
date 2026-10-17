//! Authenticated single-node release/deployment actions. No guest code runs here.
use super::*;
use devoption_builds::jobs::State;
use serde_json::Value;
use std::{
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const BROKER: &str = "/usr/local/libexec/devoption-deployment-broker";
const CONFIG: &str = "/etc/devoption/deployment.toml";
struct Review {
    project: ProjectId,
    token: String,
    release: String,
    health: String,
    fingerprint: String,
    generation: String,
    expires: Instant,
}
pub(in crate::server) struct Engine {
    pending: Option<Review>,
}
impl Engine {
    pub(in crate::server) fn open(
        owner: &Path,
        repositories: &Path,
        history: &Path,
        catalog: &Catalog,
    ) -> io::Result<Self> {
        for name in [CONFIG, BROKER] {
            for path in Path::new(name).ancestors() {
                let meta = fs::symlink_metadata(path)?;
                if meta.file_type().is_symlink() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
                    return Err(io::Error::other("runtime installation is not protected"));
                }
            }
        }
        let config = fs::read_to_string(CONFIG)?
            .parse::<toml::Table>()
            .map_err(io::Error::other)?;
        for (key, path) in [
            ("owner_data", owner),
            ("repositories", repositories),
            ("history", history),
        ] {
            if config.get(key).and_then(toml::Value::as_str).map(Path::new)
                != Some(fs::canonicalize(path)?.as_path())
            {
                return Err(io::Error::other(
                    "runtime storage does not match selected platform storage",
                ));
            }
        }
        let state = broker(&["status"])?;
        if let Some(release) = state["deployment"]["release"].as_str() {
            let manifest = state["releases"]
                .as_array()
                .and_then(|releases| releases.iter().find(|r| r["release"] == release));
            let associated = manifest.is_some_and(|m| {
                catalog.projects().iter().any(|p| {
                    m["project"] == hex(&p.id().to_bytes())
                        && p.repositories()
                            .iter()
                            .any(|r| m["repository"] == hex(&r.to_bytes()))
                })
            });
            if !associated {
                broker(&["stop"])?;
            }
        }
        broker(&["reconcile"])?;
        Ok(Self { pending: None })
    }
}
fn broker(args: &[&str]) -> io::Result<Value> {
    let mut child = Command::new("/usr/bin/sudo")
        .args(["-n", BROKER])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let output = child.stdout.take().expect("piped stdout");
    let errors = child.stderr.take().expect("piped stderr");
    let output = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        errors.take(4097).read_to_end(&mut bytes).map(|_| bytes)
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(95) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other(
                "deployment result unknown; inspect owned unit before retry",
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let bytes = output
        .join()
        .map_err(|_| io::Error::other("broker output unavailable"))??;
    let error = errors
        .join()
        .map_err(|_| io::Error::other("broker diagnostics unavailable"))??;
    if !status.success() || bytes.len() > 1024 * 1024 {
        eprintln!("deployment broker: {}", String::from_utf8_lossy(&error));
        return Err(io::Error::other("deployment operation unavailable"));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn field<'a>(value: &'a Value, key: &str) -> io::Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| io::Error::other("invalid deployment response"))
}
fn error() -> axum::response::Response {
    (StatusCode::CONFLICT, [(CACHE_CONTROL,"no-store")], "Deployment result unavailable. Refresh Releases to inspect status; prepare a new review before retrying.").into_response()
}

pub(super) fn routes(projects: Projects) -> Router {
    Router::new().route(
        "/projects/{project}/releases/{action}",
        axum::routing::post(
            move |RoutePath((p, action)): RoutePath<(String, String)>, request: Request| {
                mutate(projects, p, action, request)
            },
        ),
    )
}

async fn mutate(
    projects: Projects,
    p: String,
    action: String,
    request: Request,
) -> axum::response::Response {
    let Some(project) = project_id(&p) else {
        return error();
    };
    let Ok(body) = text_body(request, 256).await else {
        return error();
    };
    let Some(engine) = projects.deployment.clone() else {
        return error();
    };
    let mut engine = engine.lock_owned().await;
    let Some(history) = projects.history.clone() else {
        return error();
    };
    let history = history.lock_owned().await;
    let gate = projects.gate.clone().lock_owned().await;
    let result = tokio::task::spawn_blocking(move || -> io::Result<()> {
        let _gate = gate;
        let catalog = projects
            .directory
            .load()
            .map_err(io::Error::other)?
            .unwrap_or_default();
        let entry = catalog
            .projects()
            .iter()
            .find(|p| p.id() == project)
            .ok_or_else(|| io::Error::other("unknown project"))?;
        match action.as_str() {
            "promote" => {
                let records = history.load().map_err(io::Error::other)?;
                let record = records
                    .iter()
                    .find(|r| hex(&r.id().to_bytes()) == body && r.project() == project.to_bytes())
                    .ok_or_else(|| io::Error::other("build missing"))?;
                if !entry
                    .repositories()
                    .iter()
                    .any(|r| r.to_bytes() == record.repository())
                {
                    return Err(io::Error::other("repository association changed"));
                }
                let State::Succeeded(artifact) = record.state() else {
                    return Err(io::Error::other("build did not succeed"));
                };
                broker(&[
                    "promote",
                    &body,
                    &hex(artifact.sha256().bytes()),
                    &hex(&record.project()),
                    &hex(&record.repository()),
                    record.commit(),
                    &hex(record.source_sha256()),
                    &artifact.bytes().to_string(),
                ])?;
            }
            "prepare" => {
                let (release, health) = body
                    .split_once('\n')
                    .ok_or_else(|| io::Error::other("missing health expectation"))?;
                if release.len() != 64
                    || health.is_empty()
                    || health.len() > 128
                    || !health.bytes().all(|b| (32..=126).contains(&b))
                {
                    return Err(io::Error::other("invalid review"));
                }
                let state = broker(&["status"])?;
                let manifest = state["releases"]
                    .as_array()
                    .and_then(|r| {
                        r.iter().find(|r| {
                            r["release"] == release && r["project"] == hex(&project.to_bytes())
                        })
                    })
                    .ok_or_else(|| io::Error::other("release missing"))?;
                if !entry
                    .repositories()
                    .iter()
                    .any(|r| hex(&r.to_bytes()) == manifest["repository"])
                {
                    return Err(io::Error::other("release association changed"));
                }
                let mut bytes = [0; 16];
                getrandom::fill(&mut bytes).map_err(io::Error::other)?;
                engine.pending = Some(Review {
                    project,
                    token: hex(&bytes),
                    release: release.into(),
                    health: health.into(),
                    fingerprint: field(&state, "fingerprint")?.into(),
                    generation: field(&state, "generation")?.into(),
                    expires: Instant::now() + Duration::from_secs(300),
                });
            }
            "approve" => {
                let review = engine
                    .pending
                    .take()
                    .ok_or_else(|| io::Error::other("review missing"))?;
                if review.project != project
                    || review.token != body
                    || Instant::now() >= review.expires
                {
                    return Err(io::Error::other("review expired"));
                }
                let state = broker(&["status"])?;
                let manifest = state["releases"]
                    .as_array()
                    .and_then(|releases| releases.iter().find(|r| r["release"] == review.release))
                    .ok_or_else(|| io::Error::other("release missing"))?;
                if manifest["project"] != hex(&project.to_bytes())
                    || !entry
                        .repositories()
                        .iter()
                        .any(|r| hex(&r.to_bytes()) == manifest["repository"])
                {
                    return Err(io::Error::other("release association changed"));
                }
                // Root-owned release is rehashed by broker, and installed fingerprint
                // must still equal the exact reviewed configuration and worker bytes.
                broker(&[
                    "start",
                    &review.release,
                    &review.health,
                    &review.fingerprint,
                    &review.generation,
                ])?;
            }
            "stop" if body.is_empty() => {
                let state = broker(&["status"])?;
                if state["deployment"]["project"] != hex(&project.to_bytes()) {
                    return Err(io::Error::other("different active project"));
                }
                engine.pending = None;
                broker(&["stop"])?;
            }
            _ => return Err(io::Error::other("unsupported action")),
        }
        Ok(())
    })
    .await;
    match result {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        _ => error(),
    }
}

pub(super) async fn show(projects: Projects, project: ProjectId) -> axum::response::Response {
    let engine = match projects.deployment.clone() {
        Some(e) => Some(e.lock_owned().await),
        None => None,
    };
    let history = match projects.history.clone() {
        Some(h) => Some(h.lock_owned().await),
        None => None,
    };
    let gate = projects.gate.clone().lock_owned().await;
    let result = tokio::task::spawn_blocking(move || -> io::Result<String> {
        let _gate = gate;
        let catalog = projects
            .directory
            .load()
            .map_err(io::Error::other)?
            .unwrap_or_default();
        let summaries = summaries(&catalog);
        let project_view = summaries
            .iter()
            .find(|p| p.id == project.to_bytes())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "project missing"))?;
        let mut view = devoption_ui::DeploymentView::default();
        if let Some(engine) = engine {
            view.enabled = true;
            let state = broker(&["status"])?;
            view.port = state["port"].as_u64().unwrap_or(0) as u16;
            let deployment = &state["deployment"];
            if deployment["project"] == hex(&project.to_bytes()) {
                view.status = format!(
                    "{} · desired {}",
                    field(deployment, "observed")?,
                    field(deployment, "intent")?
                );
                view.current = Some(field(deployment, "release")?.into());
                view.can_stop = true;
                view.log = field(&state, "log")?.into();
            } else if !deployment.is_null() {
                view.status =
                    "Another project occupies this node. Deployment will replace it.".into();
            } else {
                view.status = "No app deployed yet.".into();
            }
            if state["last_action"]["project"] == hex(&project.to_bytes()) {
                view.status.push_str(" Last deployment attempt: ");
                view.status
                    .push_str(field(&state["last_action"], "message")?);
            }
            for release in state["releases"]
                .as_array()
                .ok_or_else(|| io::Error::other("invalid releases"))?
            {
                if release["project"] == hex(&project.to_bytes()) {
                    view.releases.push(devoption_ui::ReleaseView {
                        provenance: serde_json::to_string_pretty(release)
                            .map_err(io::Error::other)?,
                        id: field(release, "release")?.into(),
                        commit: field(release, "commit")?.into(),
                        job: field(release, "job")?.into(),
                        checksum: field(release, "artifact_sha256")?.into(),
                    });
                }
            }
            if let Some(review) = engine
                .pending
                .as_ref()
                .filter(|r| r.project == project && Instant::now() < r.expires)
            {
                view.review = Some((
                    review.token.clone(),
                    review.release.clone(),
                    review.health.clone(),
                    review.fingerprint.clone(),
                ));
            }
            if let Some(history) = history {
                for record in history.load().map_err(io::Error::other)? {
                    if record.project() == project.to_bytes()
                        && matches!(record.state(), State::Succeeded(_))
                    {
                        view.builds
                            .push((hex(&record.id().to_bytes()), record.commit().into()));
                    }
                }
            }
        } else {
            view.status =
                "Install and explicitly enable the Linux app runtime to promote and deploy builds."
                    .into();
        }
        Ok(devoption_ui::render_deployment(project_view, &view))
    })
    .await;
    match result {
        Ok(Ok(html)) => ([(CACHE_CONTROL, "no-store")], Html(html)).into_response(),
        Ok(Err(error)) if error.kind() == io::ErrorKind::NotFound => (
            StatusCode::NOT_FOUND,
            [(CACHE_CONTROL, "no-store")],
            "Project does not exist.",
        )
            .into_response(),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            [(CACHE_CONTROL, "no-store")],
            "Releases unavailable. Inspect the deployment service and saved state.",
        )
            .into_response(),
    }
}
