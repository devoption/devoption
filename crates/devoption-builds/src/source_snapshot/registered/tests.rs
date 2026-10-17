use super::*;
use devoption_projects::{CatalogStore, worktree::InspectionError};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
};

const MANIFEST: &str =
    "[package]\nname = \"fixture-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = false\n";
const LOCK: &str = "version = 4\n[[package]]\nname = \"fixture-app\"\nversion = \"0.1.0\"\n";

struct Fixture {
    _temp: tempfile::TempDir,
    base: PathBuf,
    root: PermittedRoot,
    catalog: Catalog,
    project: ProjectId,
    repository: RepositoryId,
    other: ProjectId,
    selected: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        for path in ["data", "repositories", "repositories/app"] {
            fs::create_dir(base.join(path)).unwrap();
            fs::set_permissions(base.join(path), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let app = base.join("repositories/app");
        git(&app, &["init", "--initial-branch=main"]);
        fs::create_dir(app.join("src")).unwrap();
        for (path, bytes) in [
            ("Cargo.toml", MANIFEST.as_bytes()),
            ("Cargo.lock", LOCK.as_bytes()),
            ("src/main.rs", b"compile_error!(\"never run\");\n"),
            ("src/helper.rs", b"mod helper {}\n"),
            ("asset.bin", &[0, 255]),
        ] {
            fs::write(app.join(path), bytes).unwrap();
        }
        git(&app, &["add", "--all"]);
        git(&app, &["commit", "-m", "Public registered source fixture"]);
        let selected = git(&app, &["rev-parse", "HEAD"]).trim().to_owned();
        let root = PermittedRoot::open_absolute(&base.join("repositories")).unwrap();
        let mut store = CatalogStore::open_absolute(&base.join("data")).unwrap();
        let project = store.create_project("Public fixture").unwrap();
        let other = store.create_project("Other fixture").unwrap();
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
            selected,
        }
    }
    fn app(&self) -> PathBuf {
        self.base.join("repositories/app")
    }
    fn capture(&self) -> Result<SourceSnapshot, Error> {
        capture(
            &self.catalog,
            self.project,
            self.repository,
            &self.root,
            &self.selected,
            RegistryId::new("fixture-registry").unwrap(),
        )
    }
    fn commit(&mut self) {
        git(&self.app(), &["add", "--all"]);
        git(&self.app(), &["commit", "-m", "Another fixture commit"]);
        self.selected = git(&self.app(), &["rev-parse", "HEAD"]).trim().to_owned();
    }
}
fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
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
    String::from_utf8(output.stdout).unwrap()
}
fn inspection(error: Error, expected: InspectionError) {
    assert!(
        matches!(error, Error::Repository(devoption_projects::Error::Inspection(actual)) if actual == expected)
    );
}

#[test]
fn reads_exact_committed_binary_and_nested_bytes_without_modifying_storage() {
    let f = Fixture::new();
    let paths = [
        f.base.join("data/projects.bin"),
        f.app().join(".git/index"),
        f.app().join(".git/config"),
        f.app().join("src/main.rs"),
        f.app().join("asset.bin"),
    ];
    let state = || {
        paths
            .iter()
            .map(|p| {
                (
                    fs::read(p).unwrap(),
                    fs::metadata(p).unwrap().modified().unwrap(),
                )
            })
            .collect::<Vec<_>>()
    };
    let before = state();
    let snapshot = f.capture().unwrap();
    assert_eq!(before, state());
    assert!(!f.app().join("target").exists());
    assert_eq!(snapshot.commit(), f.selected);
    assert_eq!(snapshot.binary(), Some("fixture-app"));
    assert_eq!(snapshot.files().len(), 5);
    for file in snapshot.files() {
        assert_eq!(file.bytes(), fs::read(f.app().join(file.path())).unwrap());
    }
    fs::write(f.app().join("src/main.rs"), "edited later\n").unwrap();
    assert!(
        snapshot
            .files()
            .iter()
            .find(|p| p.path() == "src/main.rs")
            .unwrap()
            .bytes()
            .starts_with(b"compile_error!")
    );
    assert!(f.capture().is_err());
}

#[test]
fn packed_history_and_linked_checkout_keep_the_selected_snapshot() {
    let mut f = Fixture::new();
    let old = f.selected.clone();
    fs::write(f.app().join("src/helper.rs"), "changed committed helper\n").unwrap();
    f.commit();
    let expected = f.capture().unwrap();
    git(&f.app(), &["repack", "-ad"]);
    assert!(f.capture().unwrap() == expected);
    assert!(
        capture(
            &f.catalog,
            f.project,
            f.repository,
            &f.root,
            &old,
            RegistryId::new("fixture-registry").unwrap()
        )
        .is_err()
    );
    git(
        &f.app(),
        &["worktree", "add", "--detach", "../linked", &f.selected],
    );
    let mut store = CatalogStore::open_absolute(&f.base.join("data")).unwrap();
    let linked = store.create_project("Linked fixture").unwrap();
    let repository = store
        .attach_repository(linked, &f.root, Path::new("linked"))
        .unwrap();
    let catalog = store.load().unwrap().unwrap();
    assert_eq!(repository, f.repository);
    let actual = capture(
        &catalog,
        linked,
        repository,
        &f.root,
        &f.selected,
        RegistryId::new("fixture-registry").unwrap(),
    )
    .unwrap();
    assert!(actual == expected);
}

#[test]
fn dirty_staged_untracked_and_assume_unchanged_source_are_rejected() {
    for mode in ["modified", "staged", "untracked", "assume-unchanged"] {
        let f = Fixture::new();
        if mode == "assume-unchanged" {
            git(
                &f.app(),
                &["update-index", "--assume-unchanged", "src/main.rs"],
            );
        }
        let path = if mode == "untracked" {
            "untracked.txt"
        } else {
            "src/main.rs"
        };
        fs::write(f.app().join(path), "changed fixture content\n").unwrap();
        if mode == "staged" {
            git(&f.app(), &["add", "src/main.rs"]);
        }
        if mode == "assume-unchanged" {
            assert_eq!(
                f.catalog
                    .repository_status(f.project, f.repository, &f.root)
                    .unwrap()
                    .changes,
                devoption_projects::worktree::ChangeCounts::default()
            );
        }
        inspection(
            f.capture().err().unwrap(),
            InspectionError::UncommittedSource,
        );
    }
}

#[test]
fn foreign_changed_and_missing_sources_never_produce_a_snapshot() {
    let f = Fixture::new();
    assert!(matches!(
        capture(
            &f.catalog,
            f.other,
            f.repository,
            &f.root,
            &f.selected,
            RegistryId::new("fixture-registry").unwrap()
        ),
        Err(Error::Repository(
            devoption_projects::Error::UnknownRepository
        ))
    ));
    for selected in [
        "HEAD",
        "0000000000000000000000000000000000000000",
        "ffffffffffffffffffffffffffffffffffffffff",
    ] {
        assert!(
            capture(
                &f.catalog,
                f.project,
                f.repository,
                &f.root,
                selected,
                RegistryId::new("fixture-registry").unwrap()
            )
            .is_err()
        );
    }
    fs::rename(f.app(), f.base.join("repositories/old")).unwrap();
    fs::create_dir(f.app()).unwrap();
    assert!(f.capture().is_err());
    let f = Fixture::new();
    fs::remove_file(f.app().join("src/main.rs")).unwrap();
    assert!(f.capture().is_err());
    let f = Fixture::new();
    fs::rename(
        f.app().join(".git/objects"),
        f.app().join(".git/old-objects"),
    )
    .unwrap();
    fs::create_dir(f.app().join(".git/objects")).unwrap();
    assert!(f.capture().is_err());
}

#[test]
fn unsupported_or_oversized_committed_inputs_fail_without_execution() {
    for path in [
        "src/lib.rs",
        "src/bin/other.rs",
        "nested/.cargo/config.toml",
    ] {
        let mut f = Fixture::new();
        let file = f.app().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, "unsupported fixture\n").unwrap();
        f.commit();
        assert!(f.capture().is_err());
    }
    let mut f = Fixture::new();
    fs::write(f.app().join("asset.bin"), vec![1; 512 * 1024 + 1]).unwrap();
    f.commit();
    inspection(f.capture().err().unwrap(), InspectionError::LimitExceeded);
    let mut f = Fixture::new();
    symlink("main.rs", f.app().join("src/link.rs")).unwrap();
    f.commit();
    assert!(f.capture().is_err());
}

#[test]
fn raw_comparison_rejects_normalized_bytes_and_ignored_executable_mode() {
    let f = Fixture::new();
    git(&f.app(), &["config", "core.autocrlf", "true"]);
    let path = f.app().join("src/main.rs");
    let text = fs::read_to_string(&path).unwrap().replace('\n', "\r\n");
    fs::write(&path, text).unwrap();
    // Record normalized content and its updated stat cache. Git should see the
    // original committed blob even though the checkout now contains CRLF bytes.
    git(&f.app(), &["add", "--", "src/main.rs"]);
    assert!(git(&f.app(), &["status", "--porcelain"]).is_empty());
    assert_eq!(
        f.catalog
            .repository_status(f.project, f.repository, &f.root)
            .unwrap()
            .changes,
        devoption_projects::worktree::ChangeCounts::default()
    );
    inspection(
        f.capture().err().unwrap(),
        InspectionError::UncommittedSource,
    );
    let f = Fixture::new();
    git(&f.app(), &["config", "core.filemode", "false"]);
    fs::set_permissions(
        f.app().join("src/main.rs"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert_eq!(
        f.catalog
            .repository_status(f.project, f.repository, &f.root)
            .unwrap()
            .changes,
        devoption_projects::worktree::ChangeCounts::default()
    );
    inspection(
        f.capture().err().unwrap(),
        InspectionError::UncommittedSource,
    );
}

#[test]
fn final_comparison_rechecks_revision_files_and_binding_after_collection() {
    for change in ["source", "revision", "checkout"] {
        let mut f = Fixture::new();
        let snapshot = f.capture().unwrap();
        // The two production boundaries can be separated by external writes.
        // Reusing already collected exact bytes must never bypass the final check.
        match change {
            "source" => {
                git(
                    &f.app(),
                    &["update-index", "--assume-unchanged", "src/main.rs"],
                );
                fs::write(
                    f.app().join("src/main.rs"),
                    "hidden edit after collection\n",
                )
                .unwrap();
            }
            "revision" => {
                fs::write(f.app().join("src/helper.rs"), "new revision\n").unwrap();
                f.commit();
            }
            _ => {
                fs::rename(f.app(), f.base.join("repositories/moved")).unwrap();
                fs::create_dir(f.app()).unwrap();
            }
        }
        let files: Vec<_> = snapshot
            .files()
            .iter()
            .map(|file| (file.path(), file.bytes()))
            .collect();
        assert!(
            f.catalog
                .check_source_files(f.project, f.repository, &f.root, snapshot.commit(), &files)
                .is_err()
        );
    }
    let f = Fixture::new();
    git(&f.app(), &["checkout", "--orphan", "unborn"]);
    inspection(
        f.capture().err().unwrap(),
        InspectionError::SelectedCommitMismatch,
    );
}

#[test]
fn registered_cdylib_capture_retains_fresh_checkout_checks() {
    let mut f = Fixture::new();
    fs::rename(f.app().join("src/main.rs"), f.app().join("src/lib.rs")).unwrap();
    fs::write(
        f.app().join("Cargo.toml"),
        format!("{MANIFEST}\n[lib]\nname = 'fixture_app'\ncrate-type = ['cdylib']\n"),
    )
    .unwrap();
    f.commit();
    let snapshot = f.capture().unwrap();
    assert_eq!(snapshot.target_path(), "src/lib.rs");
    assert_eq!(snapshot.binary(), None);
    assert_eq!(
        snapshot.target(),
        &devoption_registry::cargo_inputs::SourceTarget::Cdylib {
            name: "fixture_app".into()
        }
    );
    let original = fs::read(f.app().join("src/lib.rs")).unwrap();
    assert_eq!(
        snapshot
            .files()
            .iter()
            .find(|file| file.path() == "src/lib.rs")
            .unwrap()
            .bytes(),
        original
    );
    fs::write(f.app().join("src/lib.rs"), "changed source\n").unwrap();
    assert!(f.capture().is_err());
}
