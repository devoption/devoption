use super::*;
use devoption_projects::CatalogStore;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
};

const MANIFEST: &str =
    "[package]\nname = \"fixture-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = false\n";
const LOCK: &str = "version = 4\n[[package]]\nname = \"fixture-app\"\nversion = \"0.1.0\"\n";

fn initialize(app: &Path) {
    for child in ["", ".git", ".git/objects", ".git/refs"] {
        fs::create_dir(app.join(child)).unwrap();
        fs::set_permissions(app.join(child), fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(app.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(
        app.join(".git/config"),
        "[core]\nrepositoryformatversion = 0\nbare = false\n",
    )
    .unwrap();
}

struct Fixture {
    _temp: tempfile::TempDir,
    base: PathBuf,
    root: PermittedRoot,
    catalog: Catalog,
    project: ProjectId,
    repository: RepositoryId,
    other: ProjectId,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        for dir in ["data", "repositories"] {
            fs::create_dir(base.join(dir)).unwrap();
            fs::set_permissions(base.join(dir), fs::Permissions::from_mode(0o700)).unwrap();
        }
        initialize(&base.join("repositories/app"));
        let root = PermittedRoot::open_absolute(&base.join("repositories")).unwrap();
        let mut catalog = Catalog::default();
        let project = catalog.create_project("Source fixture").unwrap();
        let other = catalog.create_project("Other").unwrap();
        let mut store = CatalogStore::open_absolute(&base.join("data")).unwrap();
        store.save(&catalog).unwrap();
        let repository = store
            .attach_repository(project, &root, Path::new("app"))
            .unwrap();
        let catalog = store.load().unwrap().unwrap();
        Self {
            _temp: temp,
            base,
            root,
            catalog,
            project,
            repository,
            other,
        }
    }
    fn app(&self) -> PathBuf {
        self.base.join("repositories/app")
    }
    fn seed(&self) {
        fs::create_dir(self.app().join("src")).unwrap();
        fs::write(self.app().join("Cargo.toml"), MANIFEST).unwrap();
        fs::write(self.app().join("Cargo.lock"), LOCK).unwrap();
        // Invalid Rust still passes a text-presence check: no compiler is run.
        fs::write(
            self.app().join("src/main.rs"),
            "compile_error!(\"fixture must never compile\");\n",
        )
        .unwrap();
    }
    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .current_dir(self.app())
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
        String::from_utf8(output.stdout).unwrap()
    }
    fn commit(&self) {
        self.git(&["add", "--all"]);
        self.git(&["commit", "-m", "Public source fixture"]);
    }
    fn inspect(&self) -> Preparation {
        inspect(&self.catalog, self.project, self.repository, &self.root).unwrap()
    }
}

#[test]
fn real_clean_source_reports_exact_observations_without_compilation_or_writes() {
    let f = Fixture::new();
    f.seed();
    f.commit();
    let paths = [
        f.app().join(".git/index"),
        f.app().join(".git/config"),
        f.base.join("data/projects.bin"),
        f.app().join("src/main.rs"),
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|p| {
            (
                fs::read(p).unwrap(),
                fs::metadata(p).unwrap().modified().unwrap(),
            )
        })
        .collect();
    let report = f.inspect();
    assert!(report.outcome() == Outcome::Passed);
    assert_eq!(
        report.status.commit.as_deref(),
        Some(f.git(&["rev-parse", "HEAD"]).trim())
    );
    let cargo = report.cargo.unwrap();
    assert_eq!(
        (cargo.package(), cargo.version(), cargo.dependency_count()),
        ("fixture-app", "0.1.0", 0)
    );
    assert!(cargo.hashes().manifest == devoption_registry::Sha256::digest(MANIFEST.as_bytes()));
    assert!(cargo.hashes().lockfile == devoption_registry::Sha256::digest(LOCK.as_bytes()));
    assert_eq!(
        before,
        paths
            .iter()
            .map(|p| (
                fs::read(p).unwrap(),
                fs::metadata(p).unwrap().modified().unwrap()
            ))
            .collect::<Vec<_>>()
    );
    assert!(!f.app().join("target").exists());
}

#[test]
fn unborn_dirty_staged_and_untracked_states_never_pass_and_each_read_is_fresh() {
    let f = Fixture::new();
    assert!(f.inspect().outcome() == Outcome::ChangesNeeded);
    f.seed();
    let untracked = f.inspect();
    assert!(untracked.cargo.is_none());
    assert!(
        untracked
            .checks
            .iter()
            .any(|c| c.label == "Cargo.toml" && c.detail.contains("not tracked"))
    );
    f.git(&["add", "--all"]);
    assert_eq!(f.inspect().status.changes.staged, 3);
    f.git(&["commit", "-m", "Fixture"]);
    assert!(f.inspect().outcome() == Outcome::Passed);
    fs::write(f.app().join("src/main.rs"), "changed current text\n").unwrap();
    fs::write(
        f.app().join("untracked"),
        "DO NOT DISPLAY THIS FIXTURE VALUE",
    )
    .unwrap();
    let report = f.inspect();
    assert!(report.outcome() == Outcome::ChangesNeeded);
    assert_eq!(report.status.changes.modified, 1);
    assert_eq!(report.status.changes.untracked, 1);
    assert!(
        report
            .checks
            .iter()
            .all(|c| !c.detail.contains("DO NOT DISPLAY"))
    );
}

#[test]
fn missing_mismatched_unsupported_binary_and_oversized_inputs_have_specific_results() {
    let f = Fixture::new();
    f.seed();
    f.commit();
    fs::write(f.app().join("Cargo.lock"), LOCK.replace("0.1.0", "0.2.0")).unwrap();
    let report = f.inspect();
    assert!(report.outcome() == Outcome::ChangesNeeded);
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.detail.contains("does not match the lockfile root"))
    );
    fs::write(f.app().join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    assert!(
        f.inspect()
            .checks
            .iter()
            .any(|c| c.detail.contains("unsupported manifest"))
    );
    fs::write(f.app().join("Cargo.toml"), [0, 255]).unwrap();
    assert!(f.inspect().outcome() == Outcome::Unavailable);
    fs::write(f.app().join("Cargo.toml"), vec![b'x'; 65537]).unwrap();
    assert!(
        f.inspect()
            .checks
            .iter()
            .any(|c| c.detail.contains("64 KiB"))
    );
    fs::remove_file(f.app().join("Cargo.toml")).unwrap();
    assert!(
        f.inspect()
            .checks
            .iter()
            .any(|c| c.label == "Cargo.toml" && c.outcome == Outcome::Unavailable)
    );
}

#[test]
fn unsafe_files_wrong_associations_and_replaced_checkouts_are_rejected() {
    let f = Fixture::new();
    f.seed();
    f.commit();
    assert!(matches!(
        inspect(&f.catalog, f.other, f.repository, &f.root),
        Err(Error::UnknownRepository)
    ));
    let outside = f.base.join("outside");
    fs::write(&outside, "OUTSIDE FIXTURE CONTENT").unwrap();
    fs::remove_file(f.app().join("Cargo.lock")).unwrap();
    symlink(&outside, f.app().join("Cargo.lock")).unwrap();
    let report = f.inspect();
    assert!(report.outcome() == Outcome::Unavailable && report.cargo.is_none());
    assert!(
        report
            .checks
            .iter()
            .all(|c| !c.detail.contains("OUTSIDE FIXTURE CONTENT"))
    );
    fs::rename(f.app(), f.app().with_file_name("preserved")).unwrap();
    initialize(&f.app());
    assert!(matches!(
        inspect(&f.catalog, f.project, f.repository, &f.root),
        Err(Error::StaleCheckout)
    ));
}

#[test]
fn supported_library_starter_uses_its_declared_source_without_requiring_a_binary() {
    let f = Fixture::new();
    fs::create_dir(f.app().join("src")).unwrap();
    fs::write(
        f.app().join("Cargo.toml"),
        include_bytes!("../../../../examples/static-site/Cargo.toml"),
    )
    .unwrap();
    fs::write(
        f.app().join("Cargo.lock"),
        include_bytes!("../../../../examples/static-site/Cargo.lock"),
    )
    .unwrap();
    fs::write(
        f.app().join("src/lib.rs"),
        include_bytes!("../../../../examples/static-site/src/lib.rs"),
    )
    .unwrap();
    f.commit();
    let report = f.inspect();
    assert!(report.outcome() == Outcome::Passed);
    assert!(
        report
            .checks
            .iter()
            .any(|check| check.label == "src/lib.rs" && check.outcome == Outcome::Passed)
    );
    assert!(
        !report
            .checks
            .iter()
            .any(|check| check.label == "src/main.rs")
    );
    fs::write(f.app().join("src/lib.rs"), "").unwrap();
    assert!(f.inspect().outcome() == Outcome::ChangesNeeded);
}
