use super::profile::{check, read};
use super::*;
use devoption_projects::{CatalogStore, worktree::PermittedRoot};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .current_dir(root.join("main"))
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
}
fn start(data: &Path, root: &Path) -> (Server, SocketAddr) {
    start_command(
        Command::new(env!("CARGO_BIN_EXE_devoption"))
            .args(["serve", "--port", "0", "--data-dir"])
            .arg(data)
            .arg("--repository-root")
            .arg(root),
    )
}
fn hex(bytes: [u8; 16]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn seed(data: &Path, root: &Path) -> (String, String, String) {
    let (project, other) = repositories::seed(data);
    let repository = CatalogStore::open_absolute(data)
        .unwrap()
        .attach_repository(
            project,
            &PermittedRoot::open_absolute(root).unwrap(),
            Path::new("main"),
        )
        .unwrap();
    (
        format!("/projects/{}/builds", hex(project.to_bytes())),
        format!(
            "/projects/{}/builds/{}",
            hex(project.to_bytes()),
            hex(repository.to_bytes())
        ),
        format!(
            "/projects/{}/builds/{}",
            hex(other.to_bytes()),
            hex(repository.to_bytes())
        ),
    )
}
fn source(root: &Path) {
    fs::create_dir(root.join("main/src")).unwrap();
    fs::write(root.join("main/Cargo.toml"), "[package]\nname = \"fixture-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = false\n").unwrap();
    fs::write(
        root.join("main/Cargo.lock"),
        "version = 4\n[[package]]\nname = \"fixture-app\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        root.join("main/src/main.rs"),
        "compile_error!(\"never compile this fixture\");\n",
    )
    .unwrap();
    git(root, &["add", "--all"]);
    git(root, &["commit", "-m", "Source preparation fixture"]);
}

#[test]
fn preparation_uses_real_fresh_source_and_never_creates_a_job_or_modifies_storage() {
    let (_data, data) = configured();
    let (_root, root) = repositories::root_fixture();
    source(&root);
    let (list, url, other) = seed(&data, &root);
    let before = [
        fs::read(data.join("owner.bin")).unwrap(),
        fs::read(data.join("projects.bin")).unwrap(),
        fs::read(root.join("main/.git/index")).unwrap(),
    ];
    let data_files = fs::read_dir(&data).unwrap().count();
    let (server, address) = start(&data, &root);
    for path in [&list, &url] {
        for method in ["GET", "HEAD"] {
            assert_eq!(check(&read(address, method, path, None), 303), "");
        }
    }
    let issued = sign_in(address);
    assert!(
        check(&read(address, "GET", &list, Some(&issued)), 200)
            .contains(&format!("href=\"{url}\""))
    );
    let response = read(address, "GET", &url, Some(&issued));
    let html = check(&response, 200);
    assert!(html.contains("Source checks passed") && html.contains("fixture-app 0.1.0"));
    assert!(html.contains("not approve or run a build") && html.contains("Observed HEAD"));
    assert!(html.contains("Build runner not enabled"));
    assert!(!html.contains("Continue to build review"));
    assert!(!html.contains("never compile this fixture"));
    assert_eq!(check(&read(address, "HEAD", &url, Some(&issued)), 200), "");
    for path in [
        other,
        format!("{list}/missing"),
        format!("{list}/{}", "00".repeat(16)),
    ] {
        check(&read(address, "GET", &path, Some(&issued)), 404);
    }
    fs::write(root.join("main/src/main.rs"), "changed source\n").unwrap();
    let response = read(address, "GET", &url, Some(&issued));
    let html = check(&response, 200);
    assert!(html.contains("Changes needed") && html.contains("1 modified"));
    assert!(!html.contains("Source checks passed"));
    drop(server);
    let (_server, address) = start(&data, &root);
    let issued = sign_in(address);
    assert!(check(&read(address, "GET", &url, Some(&issued)), 200).contains("Changes needed"));
    assert_eq!(data_files, fs::read_dir(&data).unwrap().count());
    assert_eq!(
        before,
        [
            fs::read(data.join("owner.bin")).unwrap(),
            fs::read(data.join("projects.bin")).unwrap(),
            fs::read(root.join("main/.git/index")).unwrap()
        ]
    );
    assert!(!root.join("main/target").exists());
}

#[test]
fn source_checks_report_unborn_missing_unsupported_and_unavailable_without_false_success() {
    let (_data, data) = configured();
    let (_root, root) = repositories::root_fixture();
    let (list, url, _) = seed(&data, &root);
    let (server, address) = start(&data, &root);
    let issued = sign_in(address);
    let response = read(address, "GET", &url, Some(&issued));
    let html = check(&response, 200);
    assert!(
        html.contains("Changes needed")
            && html.contains("No commits yet")
            && html.contains("not tracked")
    );
    source(&root);
    fs::write(root.join("main/Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    let response = read(address, "GET", &url, Some(&issued));
    let html = check(&response, 200);
    assert!(html.contains("Changes needed") && html.contains("unsupported manifest"));
    fs::remove_file(root.join("main/Cargo.toml")).unwrap();
    fs::write(root.join("outside"), "DO NOT DISPLAY OUTSIDE FIXTURE").unwrap();
    std::os::unix::fs::symlink("../outside", root.join("main/Cargo.toml")).unwrap();
    let response = read(address, "GET", &url, Some(&issued));
    let html = check(&response, 200);
    assert!(html.contains("Source checks unavailable") && !html.contains("DO NOT DISPLAY OUTSIDE"));
    fs::rename(root.join("main"), root.join("preserved")).unwrap();
    assert!(
        check(&read(address, "GET", &url, Some(&issued)), 503)
            .contains("Source checks unavailable")
    );
    drop(server);
    let (_server, address) = start_with(&data);
    let issued = sign_in(address);
    assert!(
        check(&read(address, "GET", &list, Some(&issued)), 200)
            .contains("storage is not configured")
    );
    assert!(
        check(&read(address, "GET", &url, Some(&issued)), 503)
            .contains("Source checks unavailable")
    );
}
