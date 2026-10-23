//! Opt-in Linux build integration. Operator-installed broker is the OS boundary.
mod inputs;
use crate::{
    BuildPlan, EnvironmentEntry, Limits, PlanInput,
    approvals::{Approval, Approvals, OwnerId},
    history::History,
    jobs::{BuildJob, Failure, State},
    output::Stream,
    review_context::{self, DirectoryRole, ReviewContext},
    source_snapshot::SourceSnapshot,
    supervision::{Decision, Supervision},
};
use devoption_projects::{Catalog, ProjectId, RepositoryId};
use devoption_registry::{CargoMaterialization, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const BROKER: &str = "/usr/local/libexec/devoption-build-broker";
const WORKER: &str = "/usr/local/libexec/devoption-build-worker";
const CONFIG: &str = "/etc/devoption/build.toml";

#[derive(Clone)]
pub struct Configuration {
    work: PathBuf,
    toolchain: PathBuf,
    vendor: PathBuf,
    owner_data: PathBuf,
    repositories: PathBuf,
    history: PathBuf,
    toolchain_hash: Sha256,
    vendor_hash: Sha256,
    lock_hash: Sha256,
    digest: Sha256,
    uid: u32,
    _lock: Arc<fs::File>,
}

fn error() -> io::Error {
    io::Error::other("build configuration or prepared inputs are unavailable or changed")
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn installed_digest() -> io::Result<Sha256> {
    let mut manifest = Vec::new();
    for name in [CONFIG, BROKER, WORKER] {
        let path = Path::new(name);
        for parent in path.ancestors().skip(1) {
            let m = fs::symlink_metadata(parent)?;
            if !m.is_dir() || m.uid() != 0 || m.mode() & 0o022 != 0 {
                return Err(error());
            }
        }
        let m = fs::symlink_metadata(path)?;
        if !m.is_file() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err(error());
        }
        manifest.extend_from_slice(Sha256::digest(&inputs::read_regular(path, 65536)?).bytes());
        manifest.extend_from_slice(&(m.mode() & 0o777).to_be_bytes());
    }
    Ok(Sha256::digest(&manifest))
}

impl Configuration {
    /// Selecting the installed config explicitly enables real build operations.
    pub fn open() -> io::Result<Self> {
        if !cfg!(target_os = "linux") {
            return Err(io::Error::other("build execution requires Linux"));
        }
        let installed = installed_digest()?;
        let bytes = inputs::read_regular(Path::new(CONFIG), 16384)?;
        let table = std::str::from_utf8(&bytes)
            .map_err(|_| error())?
            .parse::<toml::Table>()
            .map_err(|_| error())?;
        let string = |key: &str| {
            table
                .get(key)
                .and_then(toml::Value::as_str)
                .ok_or_else(error)
        };
        let path = |key: &str| -> io::Result<PathBuf> {
            let value = string(key)?;
            if !crate::absolute_path(value)
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/_-.".contains(&b))
            {
                return Err(error());
            }
            let path = PathBuf::from(value);
            if fs::canonicalize(&path)? != path {
                return Err(error());
            }
            Ok(path)
        };
        let uid = rustix::process::geteuid().as_raw();
        if uid == 0 || table.get("uid").and_then(toml::Value::as_integer) != Some(uid.into()) {
            return Err(error());
        }
        let work_path = path("work")?;
        let work_meta = fs::metadata(&work_path)?;
        if work_meta.uid() != uid || work_meta.mode() & 0o777 != 0o700 {
            return Err(error());
        }
        let lock = fs::File::from(rustix::fs::open(
            work_path.join("execution.lock"),
            rustix::fs::OFlags::RDWR
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )?);
        let meta = lock.metadata()?;
        if !meta.is_file() || meta.uid() != uid || meta.mode() & 0o777 != 0o600 || meta.nlink() != 1
        {
            return Err(error());
        }
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive)?;
        let result = Self {
            work: path("work")?,
            toolchain: path("toolchain")?,
            vendor: path("vendor")?,
            owner_data: path("owner_data")?,
            repositories: path("repositories")?,
            history: path("history")?,
            toolchain_hash: Sha256::from_hex(string("toolchain_sha256")?).map_err(|_| error())?,
            vendor_hash: Sha256::from_hex(string("vendor_sha256")?).map_err(|_| error())?,
            lock_hash: Sha256::from_hex(string("lock_sha256")?).map_err(|_| error())?,
            digest: installed,
            uid,
            _lock: Arc::new(lock),
        };
        let work = fs::metadata(&result.work)?;
        if work.uid() != uid || work.mode() & 0o777 != 0o700 {
            return Err(error());
        }
        result.revalidate()?;
        Ok(result)
    }
    pub fn check_storage(
        &self,
        owner: &Path,
        repositories: &Path,
        history: &Path,
    ) -> io::Result<()> {
        if fs::canonicalize(owner)? != self.owner_data
            || fs::canonicalize(repositories)? != self.repositories
            || fs::canonicalize(history)? != self.history
        {
            return Err(error());
        }
        let roots = [
            &self.work,
            &self.owner_data,
            &self.repositories,
            &self.history,
            &self.toolchain,
            &self.vendor,
        ];
        for (i, left) in roots.iter().enumerate() {
            for right in &roots[i + 1..] {
                if left.starts_with(right) || right.starts_with(left) {
                    return Err(error());
                }
            }
        }
        Ok(())
    }
    fn revalidate(&self) -> io::Result<()> {
        if installed_digest()? != self.digest
            || inputs::digest_tree(&self.toolchain)? != self.toolchain_hash
            || inputs::digest_tree(&self.vendor)? != self.vendor_hash
        {
            return Err(error());
        }
        for path in [
            &self.work,
            &self.toolchain,
            &self.vendor,
            &self.owner_data,
            &self.repositories,
            &self.history,
        ] {
            if fs::canonicalize(path)? != *path {
                return Err(error());
            }
        }
        Ok(())
    }
    pub fn stop_previous(&self) -> io::Result<()> {
        let mut child = Command::new("/usr/bin/sudo")
            .args(["-n", BROKER, "stop"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(12);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                return Err(io::Error::other("build cleanup deadline reached"));
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(
                "previous build cgroup could not be stopped",
            ))
        }
    }
}

/// Setup utility: hash an already prepared local tree; never runs its files.
pub fn input_digest(path: &Path) -> io::Result<String> {
    Ok(hex(inputs::digest_tree(path)?.bytes()))
}

pub struct Review {
    pub job: BuildJob,
    context: ReviewContext,
    configuration: Configuration,
    directory: PathBuf,
    token: String,
    created: Instant,
    staged_digest: Sha256,
}
impl Review {
    pub fn matches_binding(&self, bytes: &[u8]) -> bool {
        self.context.details().catalog_binding == Sha256::digest(bytes)
    }
    pub fn token(&self) -> &str {
        &self.token
    }
    pub fn context(&self) -> &ReviewContext {
        &self.context
    }
    pub fn expired(&self) -> bool {
        self.created.elapsed() >= Duration::from_secs(300)
    }
    pub fn discard(mut self, history: &mut History) -> io::Result<()> {
        self.job.cancel_queued().map_err(io::Error::other)?;
        history.record(&self.job).map_err(io::Error::other)?;
        fs::remove_dir_all(&self.directory)
    }
}

pub fn prepare(
    config: &Configuration,
    catalog: &Catalog,
    project: ProjectId,
    repository: RepositoryId,
    source: SourceSnapshot,
    history: &mut History,
) -> io::Result<Review> {
    config.revalidate()?;
    if source.request().lockfile_sha256() != config.lock_hash {
        return Err(io::Error::other(
            "source lockfile does not match prepared dependencies",
        ));
    }
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|_| error())?;
    let token = hex(&random);
    let directory = config.work.join(&token);
    fs::DirBuilder::new().create(&directory)?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    let result = (|| {
        let mut directories = Vec::new();
        for (role, name) in [
            (DirectoryRole::Work, ""),
            (DirectoryRole::Source, "source"),
            (DirectoryRole::Home, "home"),
            (DirectoryRole::CargoHome, "cargo"),
            (DirectoryRole::Target, "target"),
            (DirectoryRole::Temporary, "tmp"),
            (DirectoryRole::Artifact, "artifact"),
        ] {
            let path = if name.is_empty() {
                directory.clone()
            } else {
                directory.join(name)
            };
            if !name.is_empty() {
                fs::create_dir(&path)?;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
            }
            let m = fs::metadata(&path)?;
            directories.push(review_context::Directory {
                role,
                path: path.to_str().ok_or_else(error)?.into(),
                device: m.dev(),
                inode: m.ino(),
                owner_uid: m.uid(),
                mode: m.mode() & 0o777,
            });
        }
        for file in source.files() {
            let path = directory.join("source").join(file.path());
            fs::create_dir_all(path.parent().ok_or_else(error)?)?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            output.write_all(file.bytes())?;
            output.sync_all()?;
        }
        let environment = [
            ("PATH", "/toolchain/bin:/usr/bin"),
            ("RUSTC", "/toolchain/bin/rustc"),
            ("HOME", "/job/home"),
            ("CARGO_HOME", "/job/cargo"),
            ("CARGO_TARGET_DIR", "/job/target"),
            ("TMPDIR", "/job/tmp"),
            ("LANG", "C.UTF-8"),
            ("CARGO_NET_OFFLINE", "true"),
            ("CARGO_INCREMENTAL", "0"),
            ("CARGO_BUILD_JOBS", "1"),
            ("RUSTC_WRAPPER", ""),
            ("RUSTC_WORKSPACE_WRAPPER", ""),
        ]
        .into_iter()
        .map(|(name, value)| EnvironmentEntry {
            name: name.into(),
            value: value.into(),
        })
        .collect();
        let dependency = CargoMaterialization::new(
            source.request().clone(),
            config.vendor_hash,
            inputs::describe_tree(&config.vendor)?.1,
        );
        let package = source.package().to_owned();
        let binary = match source.target() {
            devoption_registry::cargo_inputs::SourceTarget::Cdylib { name } => name.clone(),
            _ => return Err(io::Error::other("build execution requires a Wasm library")),
        };
        let plan = BuildPlan::validate(
            PlanInput {
                project,
                repository,
                commit: source.commit().into(),
                source,
                dependencies: dependency,
                rust_version: "1.99.0".into(),
                cargo_executable: "/toolchain/bin/cargo".into(),
                work_directory: directory.to_str().ok_or_else(error)?.into(),
                package,
                binary,
                environment,
                limits: Limits {
                    elapsed_seconds: 60,
                    log_bytes: 1048576,
                },
            },
            catalog,
        )
        .map_err(io::Error::other)?;
        let binding = catalog
            .selected_binding_bytes(project, repository)
            .map_err(io::Error::other)?;
        let context = ReviewContext::validate(
            &plan,
            review_context::Input {
                policy: review_context::Policy::WasmBuildV1,
                rust_version: "1.99.0".into(),
                cargo: review_context::Executable {
                    path: "/toolchain/bin/cargo".into(),
                    sha256: Sha256::digest(&inputs::read_regular(
                        &config.toolchain.join("bin/cargo"),
                        256 * 1024 * 1024,
                    )?),
                },
                rustc: review_context::Executable {
                    path: "/toolchain/bin/rustc".into(),
                    sha256: Sha256::digest(&inputs::read_regular(
                        &config.toolchain.join("bin/rustc"),
                        256 * 1024 * 1024,
                    )?),
                },
                toolchain_manifest: config.toolchain_hash,
                catalog_binding: Sha256::digest(&binding),
                source_snapshot: plan.details().source.sha256(),
                dependency_bundle: config.vendor_hash,
                configuration: config.digest,
                execution_uid: config.uid,
                directories,
            },
        )
        .map_err(io::Error::other)?;
        let job = history.queue(plan).map_err(io::Error::other)?;
        Ok(Review {
            job,
            context,
            configuration: config.clone(),
            directory: directory.clone(),
            token,
            created: Instant::now(),
            staged_digest: inputs::digest_tree(&directory.join("source"))?,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&directory);
    }
    result
}

/// Only this adapter issues runner events, after approval and OS observations.
/// The callback must durably checkpoint the job/output; failure stops the unit.
pub fn execute(
    mut review: Review,
    fresh: SourceSnapshot,
    owner: OwnerId,
    approvals: &Approvals,
    cancel: Arc<AtomicBool>,
    mut checkpoint: impl FnMut(&BuildJob, &crate::output::BuildOutput) -> io::Result<()>,
) -> io::Result<()> {
    let mut supervision = Supervision::for_plan(review.job.plan()).map_err(io::Error::other)?;
    let admission = (|| -> io::Result<()> {
        if review.expired() || fresh != review.job.plan().details().source {
            return Err(io::Error::other("build review expired or source changed"));
        }
        review.configuration.revalidate()?;
        if inputs::digest_tree(&review.directory.join("source"))? != review.staged_digest {
            return Err(error());
        }
        for directory in &review.context.details().directories {
            let path = Path::new(&directory.path);
            let meta = fs::symlink_metadata(path)?;
            if !meta.is_dir()
                || fs::canonicalize(path)? != path
                || meta.dev() != directory.device
                || meta.ino() != directory.inode
                || meta.uid() != directory.owner_uid
                || meta.mode() & 0o777 != directory.mode
            {
                return Err(error());
            }
        }
        // Compare the staged copy before admission; only the verified owned snapshot runs.
        for file in fresh.files() {
            if inputs::read_regular(
                &review.directory.join("source").join(file.path()),
                crate::source_snapshot::MAX_FILE_BYTES as u64,
            )? != file.bytes()
            {
                return Err(error());
            }
        }
        let approval: Approval = approvals
            .approve(owner, &review.job, review.context.clone())
            .map_err(io::Error::other)?;
        approvals
            .consume(&approval, owner, &review.job, &review.context)
            .map_err(io::Error::other)?;
        Ok(())
    })();
    if admission.is_err() {
        review
            .job
            .apply(crate::jobs::RunnerEvent::observed(
                &review.job,
                State::Failed(Failure::Preparation),
            ))
            .map_err(io::Error::other)?;
        return checkpoint(&review.job, supervision.output());
    }
    supervision = Supervision::for_plan(review.job.plan()).map_err(io::Error::other)?;
    if cancel.load(Ordering::SeqCst) {
        review.job.cancel_queued().map_err(io::Error::other)?;
        return checkpoint(&review.job, supervision.output());
    }
    let mut child = match Command::new("/usr/bin/sudo")
        .args(["-n", BROKER, "run", &review.token])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            review
                .job
                .apply(crate::jobs::RunnerEvent::observed(
                    &review.job,
                    State::Failed(Failure::Preparation),
                ))
                .map_err(io::Error::other)?;
            checkpoint(&review.job, supervision.output())?;
            return Err(e);
        }
    };
    review
        .job
        .apply(crate::jobs::RunnerEvent::observed(
            &review.job,
            State::Running,
        ))
        .map_err(io::Error::other)?;
    let (tx, rx) = mpsc::sync_channel(8);
    enum Message {
        Artifact(Vec<u8>),
        Log(Vec<u8>),
        Closed,
    }
    let mut out = child.stdout.take().ok_or_else(error)?;
    let mut err = child.stderr.take().ok_or_else(error)?;
    let sender = tx.clone();
    let stdout = std::thread::spawn(move || {
        let mut buffer = [0; 8192];
        while let Ok(n) = out.read(&mut buffer) {
            if n == 0 {
                break;
            }
            if sender
                .send(Message::Artifact(buffer[..n].to_vec()))
                .is_err()
            {
                return;
            }
        }
        let _ = sender.send(Message::Closed);
    });
    let stderr = std::thread::spawn(move || {
        let mut buffer = [0; 8192];
        while let Ok(n) = err.read(&mut buffer) {
            if n == 0 {
                break;
            }
            if tx.send(Message::Log(buffer[..n].to_vec())).is_err() {
                return;
            }
        }
        let _ = tx.send(Message::Closed);
    });
    let mut artifact = Vec::new();
    let mut closed = 0;
    let mut stopped = false;
    let mut failed_checkpoint = false;
    let mut reason = None;
    let mut last_checkpoint = Instant::now();
    let cleanup_deadline = Instant::now() + Duration::from_secs(90);
    if checkpoint(&review.job, supervision.output()).is_err() {
        failed_checkpoint = true;
    }
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Message::Artifact(bytes)) => {
                if artifact.len() + bytes.len() > 4 * 1024 * 1024 {
                    reason = Some(Failure::ArtifactRejected);
                } else {
                    artifact.extend_from_slice(&bytes);
                }
            }
            Ok(Message::Log(bytes)) => {
                supervision.append(Stream::Stderr, &bytes);
            }
            Ok(Message::Closed) => closed += 1,
            Err(_) => {}
        }
        if cancel.load(Ordering::SeqCst) {
            supervision.cancel();
        }
        if let Decision::Terminate(cause) | Decision::ForceKill(cause) = supervision.poll() {
            reason.get_or_insert(match cause {
                crate::supervision::Reason::TimedOut => Failure::TimedOut,
                _ => Failure::Terminated,
            });
        }
        if last_checkpoint.elapsed() >= Duration::from_millis(250) {
            if checkpoint(&review.job, supervision.output()).is_err() {
                failed_checkpoint = true;
            }
            last_checkpoint = Instant::now();
        }
        if !stopped && (reason.is_some() || failed_checkpoint || cancel.load(Ordering::SeqCst)) {
            if review.configuration.stop_previous().is_err() {
                let _ = child.kill();
                review.job.interrupt().map_err(io::Error::other)?;
                let _ = checkpoint(&review.job, supervision.output());
                return Err(io::Error::other(
                    "build cleanup is unconfirmed; restart and reconcile before retry",
                ));
            }
            stopped = true;
        }
        if Instant::now() >= cleanup_deadline {
            let _ = review.configuration.stop_previous();
            let _ = child.kill();
            review.job.interrupt().map_err(io::Error::other)?;
            let _ = checkpoint(&review.job, supervision.output());
            return Err(io::Error::other(
                "build supervisor cleanup deadline reached",
            ));
        }
        if closed == 2 {
            std::thread::sleep(Duration::from_millis(25));
        }
        if child.try_wait()?.is_some() && closed == 2 {
            break;
        }
    }
    let exit = child.wait()?;
    let _ = stdout.join();
    let _ = stderr.join();
    // Even successful completion must establish that the complete owned unit stopped.
    review.configuration.stop_previous()?;
    let state = if cancel.load(Ordering::SeqCst) {
        State::Cancelled
    } else if let Some(failure) = reason {
        State::Failed(failure)
    } else if failed_checkpoint {
        State::Failed(Failure::Preparation)
    } else if !exit.success() {
        State::Failed(match exit.code() {
            Some(124) => Failure::TimedOut,
            Some(125) => Failure::Terminated,
            Some(126) => Failure::ArtifactRejected,
            _ => Failure::ExitFailure,
        })
    } else if artifact.len() < 8 || artifact[..8] != *b"\0asm\x0d\0\x01\0" {
        State::Failed(Failure::ArtifactRejected)
    } else {
        let digest = Sha256::digest(&artifact);
        let path = review.directory.join("artifact/component.wasm");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(&artifact)?;
        file.sync_all()?;
        let receipt = format!(
            "version = 2\njob = \"{}\"\nproject = \"{}\"\nrepository = \"{}\"\ncommit = \"{}\"\nsource_sha256 = \"{}\"\nreview_sha256 = \"{}\"\nartifact_sha256 = \"{}\"\nartifact_bytes = {}\nartifact = \"component.wasm\"\n",
            hex(&review.job.id().to_bytes()),
            hex(&review.job.plan().details().project.to_bytes()),
            hex(&review.job.plan().details().repository.to_bytes()),
            review.job.plan().details().commit,
            hex(review.job.plan().details().source.sha256().bytes()),
            hex(review.context.sha256().bytes()),
            hex(digest.bytes()),
            artifact.len()
        );
        let context = review.context.details();
        let receipt = format!(
            "{receipt}toolchain_sha256 = \"{}\"\ndependency_sha256 = \"{}\"\nconfiguration_sha256 = \"{}\"\nbuild_policy = \"WasmBuildV1\"\ntarget = \"wasm32-wasip2\"\nabi = \"wasi:http/proxy@0.2.4\"\n",
            hex(context.toolchain_manifest.bytes()),
            hex(context.dependency_bundle.bytes()),
            hex(context.configuration.bytes())
        );
        let mut receipt_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(review.directory.join("artifact/receipt.toml"))?;
        receipt_file.set_permissions(fs::Permissions::from_mode(0o600))?;
        receipt_file.write_all(receipt.as_bytes())?;
        receipt_file.sync_all()?;
        fs::File::open(review.directory.join("artifact"))?.sync_all()?;
        State::Succeeded(
            crate::jobs::ArtifactMetadata::new(digest, artifact.len() as u64)
                .map_err(io::Error::other)?,
        )
    };
    review
        .job
        .apply(crate::jobs::RunnerEvent::observed(&review.job, state))
        .map_err(io::Error::other)?;
    checkpoint(&review.job, supervision.output())
}

pub fn registry() -> devoption_registry::RegistryId {
    devoption_registry::RegistryId::new("operator-prepared-wasi").expect("fixed registry identity")
}
