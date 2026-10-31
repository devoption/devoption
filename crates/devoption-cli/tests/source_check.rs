#![cfg(unix)]

use devoption_projects::{CatalogStore, worktree::PermittedRoot};
use std::{
    ffi::OsString,
    fs,
    os::unix::{
        ffi::OsStringExt,
        fs::{PermissionsExt, symlink},
    },
    path::{Path, PathBuf},
    process::{Command, Output},
};

const MANIFEST: &str =
    "[package]\nname = \"fixture-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = false\n";
const LOCK: &str = "version = 4\n[[package]]\nname = \"fixture-app\"\nversion = \"0.1.0\"\n";

struct Fixture {
    _temp: tempfile::TempDir,
    base: PathBuf,
    project: String,
    repository: String,
    other: String,
}

fn hex(bytes: [u8; 16]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn private_directory(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        for child in [
            "data",
            "repositories",
            "repositories/app",
            "repositories/app/.git",
            "repositories/app/.git/objects",
            "repositories/app/.git/refs",
        ] {
            private_directory(&base.join(child));
        }
        fs::write(
            base.join("repositories/app/.git/HEAD"),
            "ref: refs/heads/main\n",
        )
        .unwrap();
        fs::write(
            base.join("repositories/app/.git/config"),
            "[core]\nrepositoryformatversion = 0\nbare = false\n",
        )
        .unwrap();
        let root = PermittedRoot::open_absolute(&base.join("repositories")).unwrap();
        let mut store = CatalogStore::open_absolute(&base.join("data")).unwrap();
        let project = store
            .create_project("Private project name must not appear")
            .unwrap();
        let other = store.create_project("Other").unwrap();
        let repository = store
            .attach_repository(project, &root, Path::new("app"))
            .unwrap();
        Self {
            _temp: temp,
            base,
            project: hex(project.to_bytes()),
            repository: hex(repository.to_bytes()),
            other: hex(other.to_bytes()),
        }
    }
    fn app(&self) -> PathBuf {
        self.base.join("repositories/app")
    }
    fn args(&self) -> Vec<OsString> {
        vec![
            "--data-dir".into(),
            self.base.join("data").into(),
            "--repository-root".into(),
            self.base.join("repositories").into(),
            "--project".into(),
            self.project.clone().into(),
            "--repository".into(),
            self.repository.clone().into(),
        ]
    }
    fn check(&self) -> Output {
        run(&self.args())
    }
    fn seed(&self) {
        private_directory(&self.app().join("src"));
        fs::write(self.app().join("Cargo.toml"), MANIFEST).unwrap();
        fs::write(self.app().join("Cargo.lock"), LOCK).unwrap();
        // A presence check must pass this without invoking a compiler.
        fs::write(
            self.app().join("src/main.rs"),
            "compile_error!(\"private source sentinel\");\n",
        )
        .unwrap();
        self.git(&["add", "--all"]);
        self.git(&["commit", "-m", "Public source fixture"]);
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
}

fn run(args: &[OsString]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_devoption"))
        .arg("check-source")
        .args(args)
        .output()
        .unwrap()
}

fn report(output: Output, code: i32) -> String {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("No build ran or execution was approved"));
    assert!(!text.contains("private source sentinel"));
    assert!(!text.contains("Private project name"));
    text
}

#[test]
fn clean_source_prints_observed_facts_without_owner_access_or_writes() {
    let f = Fixture::new();
    f.seed();
    // An invalid public fixture record would make HTTP owner startup fail.
    fs::write(f.base.join("data/owner.bin"), "not an owner record").unwrap();
    let paths = [
        f.base.join("data/projects.bin"),
        f.base.join("data/.projects.lock"),
        f.base.join("data/owner.bin"),
        f.app().join(".git/index"),
        f.app().join(".git/config"),
        f.app().join("src/main.rs"),
        f.app().join("Cargo.toml"),
        f.app().join("Cargo.lock"),
    ];
    let snapshot = || {
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
    let before = snapshot();
    // A concurrent catalog writer does not prevent this read-only command.
    let _writer = CatalogStore::open_absolute(&f.base.join("data")).unwrap();
    let text = report(f.check(), 0);
    assert!(text.starts_with("Source checks passed\n"));
    assert!(text.contains(&format!(
        "Observed HEAD: {}",
        f.git(&["rev-parse", "HEAD"]).trim()
    )));
    assert!(text.contains("Package: fixture-app 0.1.0\nLocked registry dependencies: 0"));
    assert!(!text.contains(f.base.to_str().unwrap()));
    assert_eq!(before, snapshot());
    assert!(!f.app().join("target").exists());
    let mut args = f.args();
    args[5] = f.project.to_uppercase().into();
    args[7] = f.repository.to_uppercase().into();
    report(run(&args), 0);
}

#[test]
fn missing_dirty_and_unsupported_inputs_have_honest_fresh_results() {
    let f = Fixture::new();
    let text = report(f.check(), 1);
    assert!(text.starts_with("Changes needed\nObserved HEAD: no commit"));
    assert!(text.contains("Cargo.toml: changes needed"));
    f.seed();
    report(f.check(), 0);
    fs::write(f.app().join("src/main.rs"), "changed source\n").unwrap();
    let text = report(f.check(), 1);
    assert!(text.contains("Working tree: changes needed"));
    fs::write(f.app().join("Cargo.toml"), "[workspace]\n").unwrap();
    let text = report(f.check(), 1);
    assert!(text.contains("Cargo inputs: changes needed"));
    assert!(!text.contains("Package:"));
    fs::write(f.app().join("Cargo.lock"), [0, 255]).unwrap();
    assert!(report(f.check(), 1).starts_with("Source checks unavailable"));
}

#[test]
fn unknown_foreign_and_changed_bindings_fail_without_source_or_paths() {
    let f = Fixture::new();
    f.seed();
    for (index, value) in [
        (5, "00".repeat(16)),
        (7, "00".repeat(16)),
        (5, f.other.clone()),
    ] {
        let mut args = f.args();
        args[index] = value.into();
        unavailable(run(&args), &f.base);
    }
    fs::rename(f.app(), f.base.join("repositories/moved")).unwrap();
    private_directory(&f.app());
    unavailable(f.check(), &f.base);
}

fn unavailable(output: Output, private_path: &Path) {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let text = String::from_utf8(output.stderr).unwrap();
    assert!(text.starts_with("Source checks unavailable:"));
    assert!(!text.contains(private_path.to_str().unwrap()));
}

#[test]
fn unsafe_missing_and_overlapping_storage_is_rejected_without_creation() {
    let f = Fixture::new();
    private_directory(&f.base.join("empty"));
    private_directory(&f.base.join("data/nested-root"));
    private_directory(&f.base.join("repositories/nested-data"));
    symlink(f.base.join("data"), f.base.join("alias")).unwrap();
    for (data, root) in [
        ("empty", "repositories"),
        ("missing", "repositories"),
        ("alias", "repositories"),
        ("data", "data"),
        ("data", "data/nested-root"),
        ("repositories/nested-data", "repositories"),
        ("data/../data", "repositories"),
    ] {
        let mut args = f.args();
        args[1] = f.base.join(data).into();
        args[3] = f.base.join(root).into();
        unavailable(run(&args), &f.base);
    }
    assert_eq!(fs::read_dir(f.base.join("empty")).unwrap().count(), 0);
    assert!(!f.base.join("missing").exists());
    fs::set_permissions(f.base.join("data"), fs::Permissions::from_mode(0o755)).unwrap();
    unavailable(f.check(), &f.base);
    fs::set_permissions(f.base.join("data"), fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn missing_duplicate_unknown_relative_and_non_hex_arguments_exit_two() {
    let f = Fixture::new();
    let original = f.args();
    let mut invalid = vec![vec![], vec!["--data-dir".into()]];
    for start in [0, 2, 4, 6] {
        let mut args = original.clone();
        args.drain(start..start + 2);
        invalid.push(args);
        let mut args = original.clone();
        args.extend_from_slice(&original[start..start + 2]);
        invalid.push(args);
    }
    for (index, value) in [
        (0, "--managed-repository-root"),
        (1, "relative"),
        (3, "relative"),
        (5, ""),
        (5, "abcdef"),
        (5, "+0000000000000000000000000000000"),
        (7, "zz000000000000000000000000000000"),
    ] {
        let mut args = original.clone();
        args[index] = value.into();
        invalid.push(args);
    }
    for index in [5, 7] {
        for value in [
            OsString::from("é".repeat(16)),
            OsString::from("0".repeat(33)),
            OsString::from(format!("-{}", "0".repeat(31))),
            OsString::from_vec(vec![255; 32]),
        ] {
            let mut args = original.clone();
            args[index] = value;
            let output = run(&args);
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
            assert_eq!(
                String::from_utf8(output.stderr).unwrap(),
                "error: project and repository IDs require exactly 32 hexadecimal characters; use --help\n"
            );
        }
    }
    let mut args = original;
    args.push("extra".into());
    invalid.push(args);
    for args in invalid {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let text = String::from_utf8(output.stderr).unwrap();
        assert!(text.contains("use --help"));
        assert!(!text.contains(f.base.to_str().unwrap()));
    }
}
