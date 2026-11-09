//! Explicit disposable Linux acceptance; never enabled by ordinary workspace tests.
#![cfg(unix)]
use devoption_builds::{
    approvals::{Approvals, OwnerId},
    execution::{self, Configuration},
    history::History,
    jobs::{Failure, State},
    source_snapshot::{self, registered},
};
use devoption_projects::{CatalogStore, ProjectId, RepositoryId, worktree::PermittedRoot};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .current_dir(path)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

struct Fixture {
    base: PathBuf,
    catalog: CatalogStore,
    root: PermittedRoot,
    project: ProjectId,
    repository: RepositoryId,
    config: Configuration,
    history: History,
}
impl Fixture {
    fn capture(&self) -> source_snapshot::SourceSnapshot {
        let catalog = self.catalog.load().unwrap().unwrap();
        registered::capture(
            &catalog,
            self.project,
            self.repository,
            &self.root,
            &git(
                &self.base.join("repositories/library"),
                &["rev-parse", "HEAD"],
            ),
            execution::registry(),
        )
        .unwrap()
    }
    fn commit(&self, source: &str) {
        let path = self.base.join("repositories/library");
        fs::write(path.join("src/lib.rs"), source).unwrap();
        git(&path, &["add", "--all"]);
        git(&path, &["commit", "-m", "Trusted fixture revision"]);
    }
    fn run(&mut self, stale: bool, cancel_after_start: bool) -> (State, PathBuf) {
        let source = self.capture();
        let catalog = self.catalog.load().unwrap().unwrap();
        // The app adapter must refuse this library before creating an attempt.
        let before = self.history.load().unwrap().len();
        assert!(
            execution::prepare(
                &self.config,
                &catalog,
                self.project,
                self.repository,
                source.clone(),
                &mut self.history
            )
            .is_err()
        );
        assert_eq!(before, self.history.load().unwrap().len());
        let review = execution::prepare_library(
            &self.config,
            &catalog,
            self.project,
            self.repository,
            source.clone(),
            &mut self.history,
        )
        .unwrap();
        assert_eq!(review.job.plan().library_commands().len(), 3);
        let artifact = self
            .config
            .work_root()
            .join(review.token())
            .join("artifact/source.tar");
        if stale {
            self.commit("pub fn changed() {}\n");
        }
        let fresh = self.capture();
        let mut observed = State::Queued;
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let mut cancellation = None;
        execution::execute(
            review,
            fresh,
            OwnerId::from_bytes([7; 16]),
            &Approvals::default(),
            cancel,
            |job, output| {
                self.history
                    .checkpoint(job, output)
                    .map_err(std::io::Error::other)?;
                observed = job.state().clone();
                if cancel_after_start && observed == State::Running && cancellation.is_none() {
                    let signal = signal.clone();
                    cancellation = Some(std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(1));
                        signal.store(true, std::sync::atomic::Ordering::SeqCst);
                    }));
                }
                Ok(())
            },
        )
        .unwrap();
        if let Some(thread) = cancellation {
            thread.join().unwrap();
        }
        self.config.stop_previous().unwrap();
        (observed, artifact)
    }
}

#[test]
#[ignore = "requires explicit disposable Linux broker/configuration from check_library_build.py"]
fn approved_library_build_test_package_and_failures() {
    assert_eq!(
        std::env::consts::OS,
        "linux",
        "explicit Linux acceptance only"
    );
    let base = PathBuf::from(
        std::env::var_os("DEVOPTION_LIBRARY_ACCEPTANCE_ROOT").expect("explicit disposable root"),
    );
    assert!(base.is_absolute() && base.canonicalize().unwrap() == base);
    assert_ne!(rustix::process::geteuid().as_raw(), 0);
    let root = PermittedRoot::open_absolute(&base.join("repositories")).unwrap();
    let mut catalog = CatalogStore::open_absolute(&base.join("data")).unwrap();
    let project = catalog
        .create_project("Trusted library acceptance")
        .unwrap();
    let repository = catalog
        .attach_repository(project, &root, Path::new("library"))
        .unwrap();
    let config = Configuration::open().unwrap();
    config
        .check_storage(
            &base.join("data"),
            &base.join("repositories"),
            &base.join("history"),
        )
        .unwrap();
    config.stop_previous().unwrap();
    let history = History::open(base.join("history").to_str().unwrap()).unwrap();
    let mut fixture = Fixture {
        base: base.clone(),
        catalog,
        root,
        project,
        repository,
        config,
        history,
    };
    let source_before = fixture.capture();
    let (state, archive) = fixture.run(false, false);
    assert!(matches!(state, State::Succeeded(_)), "{state:?}");
    assert!(source_before == fixture.capture());
    assert_eq!(
        fs::read(&archive).unwrap(),
        devoption_builds::library::source_package(&source_before).unwrap()
    );
    let receipt: toml::Table = fs::read_to_string(archive.parent().unwrap().join("receipt.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(receipt["build_policy"].as_str(), Some("LibraryBuildV1"));
    assert_eq!(receipt["artifact"].as_str(), Some("source.tar"));
    assert_eq!(receipt["package"].as_str(), Some("reusable-fixture"));
    assert_eq!(receipt["package_version"].as_str(), Some("0.1.0"));
    assert_eq!(receipt["commit"].as_str(), Some(source_before.commit()));
    assert_eq!(
        receipt["artifact_bytes"].as_integer(),
        Some(fs::metadata(&archive).unwrap().len() as i64)
    );
    assert!(!archive.parent().unwrap().join("component.wasm").exists());
    fs::copy(&archive, base.join("verified-source.tar")).unwrap();
    // Turn the real completed broker attempt into a durable private candidate.
    // No new workload or publication is authorized by this conversion.
    let candidate = {
        let _retention = fixture.config.retention_lock().unwrap();
        let attempt = fixture
            .history
            .load()
            .unwrap()
            .into_iter()
            .find(|r| matches!(r.state(), State::Succeeded(_)))
            .unwrap()
            .id();
        let destination = devoption_registry::HostedRegistry::new(
            devoption_registry::RegistryId::new("team").unwrap(),
            "sparse+https://packages.example.test/index/",
        )
        .unwrap();
        fixture
            .history
            .library_candidate(
                attempt,
                fixture.config.work_root(),
                destination,
                &source_before,
            )
            .unwrap()
            .unwrap()
    };
    assert_eq!(
        candidate.build_provenance().unwrap().receipt(),
        fs::read(archive.parent().unwrap().join("receipt.toml")).unwrap()
    );
    let candidate_path = base.join("candidates");
    fs::create_dir(&candidate_path).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&candidate_path, fs::Permissions::from_mode(0o700)).unwrap();
    let mut candidates = devoption_builds::library::package_store::PackageStore::open(
        candidate_path.to_str().unwrap(),
    )
    .unwrap();
    candidates.stage(&candidate).unwrap();
    drop(candidates);
    let mut candidates = devoption_builds::library::package_store::PackageStore::open(
        candidate_path.to_str().unwrap(),
    )
    .unwrap();
    assert!(
        candidates
            .read(candidate.source().coordinates())
            .unwrap()
            .unwrap()
            == candidate
    );

    fixture.commit("pub fn answer() -> u32 { 43 }\n");
    let (state, second) = fixture.run(false, false);
    assert!(matches!(state, State::Succeeded(_)), "{state:?}");
    assert_ne!(fs::read(second).unwrap(), fs::read(archive).unwrap());
    {
        let _retention = fixture.config.retention_lock().unwrap();
        let attempt = fixture
            .history
            .load()
            .unwrap()
            .into_iter()
            .find(|r| r.commit() == fixture.capture().commit())
            .unwrap()
            .id();
        let updated = fixture
            .history
            .library_candidate(
                attempt,
                fixture.config.work_root(),
                candidate.source().coordinates().registry().clone(),
                &fixture.capture(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            candidates.stage(&updated).err(),
            Some(devoption_builds::library::staging::Error::Conflict)
        );
        assert!(
            candidates
                .read(candidate.source().coordinates())
                .unwrap()
                .unwrap()
                == candidate
        );
    }

    let (state, artifact) = fixture.run(true, false);
    assert_eq!(state, State::Failed(Failure::Preparation));
    assert!(!artifact.exists());
    fixture
        .commit("pub fn answer() -> u32 { 42 }\n#[test] fn fails() { assert_eq!(answer(), 0); }\n");
    let (state, artifact) = fixture.run(false, false);
    assert_eq!(state, State::Failed(Failure::ExitFailure));
    assert!(!artifact.exists());
    fixture.commit("#[test] fn waits() { loop { std::thread::sleep(std::time::Duration::from_millis(10)); } }\n");
    let (state, artifact) = fixture.run(false, true);
    assert_eq!(state, State::Cancelled);
    assert!(!artifact.exists());
    assert!(
        fixture
            .history
            .load()
            .unwrap()
            .iter()
            .all(|r| !matches!(r.state(), State::Queued | State::Running))
    );
    for record in fixture.history.load().unwrap() {
        if !matches!(record.state(), State::Succeeded(_)) {
            assert!(
                fixture
                    .history
                    .library_candidate(
                        record.id(),
                        fixture.config.work_root(),
                        candidate.source().coordinates().registry().clone(),
                        &source_before
                    )
                    .unwrap()
                    .is_none()
            );
        }
    }
    println!(
        "Library native build/unit tests/WASIp2, exact source archive and build-bound Cargo candidate, immutable version conflict, revision change, stale review, failed tests and cancellation passed."
    );
}
