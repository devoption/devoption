use super::profile::{check, read};
use super::*;
use devoption_projects::CatalogStore;
use devoption_projects::worktree::PermittedRoot;
use std::os::unix::fs::symlink;

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
fn seed(data: &Path, root: &Path) -> (String, String) {
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
        format!(
            "/projects/{}/repositories/{}/files",
            hex(project.to_bytes()),
            hex(repository.to_bytes())
        ),
        format!(
            "/projects/{}/repositories/{}/files",
            hex(other.to_bytes()),
            hex(repository.to_bytes())
        ),
    )
}
fn stage(root: &Path) {
    let result = Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .current_dir(root.join("main"))
        .args(["-c", "core.hooksPath=/dev/null", "add", "--all"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn authenticated_file_links_show_escaped_tracked_working_text_and_survive_restart() {
    let (_data, data) = configured();
    let (_root, root) = repositories::root_fixture();
    fs::create_dir(root.join("main/src")).unwrap();
    fs::write(root.join("main/src/main.rs"), "staged").unwrap();
    fs::write(root.join("main/résumé & notes.txt"), "hello").unwrap();
    stage(&root);
    fs::write(
        root.join("main/src/main.rs"),
        "<script>fixture_only()</script>\ncurrent working tree\n",
    )
    .unwrap();
    fs::write(
        root.join("main/untracked-secret"),
        "PRIVATE UNTRACKED FIXTURE",
    )
    .unwrap();
    let (url, other) = seed(&data, &root);
    let code = format!("{}/code", url.split("/repositories/").next().unwrap());
    let before = [
        fs::read(data.join("owner.bin")).unwrap(),
        fs::read(data.join("projects.bin")).unwrap(),
        fs::read(root.join("main/.git/index")).unwrap(),
    ];
    let (server, address) = start(&data, &root);
    for path in [&code, &url, &format!("{url}/src/main.rs")] {
        for method in ["GET", "HEAD"] {
            assert_eq!(check(&read(address, method, path, None), 303), "");
        }
    }
    let issued = sign_in(address);
    let response = read(address, "GET", &url, Some(&issued));
    let list = check(&response, 200);
    assert!(list.contains("src/main.rs") && list.contains("r%C3%A9sum%C3%A9%20%26%20notes.txt"));
    assert!(!list.contains("untracked-secret"));
    let direct = read(address, "GET", &code, Some(&issued));
    let direct = check(&direct, 200);
    assert_eq!(direct, list);
    assert!(!direct.contains("Browse files") && !direct.contains("Back to repositories"));
    assert!(direct.find("class=\"tracked-files\"") < direct.find("class=\"repository-details\""));
    assert!(direct.contains("data-repository-status-view"));
    let response = read(address, "GET", &format!("{url}/src/main.rs"), Some(&issued));
    let body = check(&response, 200);
    assert!(body.contains("&lt;script&gt;fixture_only()&lt;/script&gt;\ncurrent working tree"));
    assert!(!body.contains("<script>fixture_only()"));
    assert!(body.contains("Content may include uncommitted changes"));
    assert_eq!(
        check(
            &read(
                address,
                "HEAD",
                &format!("{url}/src/main.rs"),
                Some(&issued)
            ),
            200
        ),
        ""
    );
    check(&read(address, "GET", &other, Some(&issued)), 404);
    check(
        &read(
            address,
            "GET",
            &format!("{url}/r%C3%A9sum%C3%A9%20%26%20notes.txt"),
            Some(&issued),
        ),
        200,
    );
    for path in [
        "untracked-secret",
        ".git/config",
        "%2e%2e/outside",
        "%2Foutside",
        "src%2F..%2Fmain.rs",
    ] {
        let response = read(address, "GET", &format!("{url}/{path}"), Some(&issued));
        let body = check(&response, 404);
        assert!(!body.contains("PRIVATE UNTRACKED FIXTURE"));
    }
    drop(server);
    let (_server, address) = start(&data, &root);
    let issued = sign_in(address);
    check(
        &read(address, "GET", &format!("{url}/src/main.rs"), Some(&issued)),
        200,
    );
    assert_eq!(
        before,
        [
            fs::read(data.join("owner.bin")).unwrap(),
            fs::read(data.join("projects.bin")).unwrap(),
            fs::read(root.join("main/.git/index")).unwrap()
        ]
    );
}

#[test]
fn file_preview_reports_binary_limits_and_unsafe_replacements_without_contents() {
    let (_data, data) = configured();
    let (_root, root) = repositories::root_fixture();
    fs::write(root.join("main/binary"), [0, 255]).unwrap();
    fs::write(root.join("main/large"), vec![b'x'; 65537]).unwrap();
    fs::write(root.join("main/replaced"), "original").unwrap();
    stage(&root);
    fs::write(root.join("outside"), "OUTSIDE FIXTURE SECRET").unwrap();
    fs::remove_file(root.join("main/replaced")).unwrap();
    symlink("../outside", root.join("main/replaced")).unwrap();
    let (url, _) = seed(&data, &root);
    let (_server, address) = start(&data, &root);
    let issued = sign_in(address);
    for (path, message) in [
        ("binary", "Binary files"),
        ("large", "too large to display"),
        ("replaced", "missing, changed or unavailable"),
    ] {
        let response = read(address, "GET", &format!("{url}/{path}"), Some(&issued));
        let body = check(&response, 200);
        assert!(body.contains(message));
        assert!(!body.contains("OUTSIDE FIXTURE SECRET"));
    }
    fs::rename(root.join("main"), root.join("preserved")).unwrap();
    devoption_projects::worktree::ManagedRoot::open_absolute(&root).unwrap();
    let response = read(address, "GET", &url, Some(&issued));
    check(&response, 503);
}

#[test]
fn empty_indexes_and_missing_root_have_honest_authenticated_states() {
    let (_data, data) = configured();
    let (_root, root) = repositories::root_fixture();
    let (url, _) = seed(&data, &root);
    let (server, address) = start(&data, &root);
    let issued = sign_in(address);
    assert!(check(&read(address, "GET", &url, Some(&issued)), 200).contains("No tracked files"));
    let code = format!("{}/code", url.split("/repositories/").next().unwrap());
    assert!(check(&read(address, "GET", &code, Some(&issued)), 200).contains("No tracked files"));
    drop(server);
    let (_server, address) = start_with(&data);
    let issued = sign_in(address);
    assert!(
        check(&read(address, "GET", &url, Some(&issued)), 503)
            .contains("Repository inspection is not configured")
    );
    let response = read(address, "GET", &code, Some(&issued));
    let body = check(&response, 503);
    assert!(body.contains("Repository inspection is not configured"));
    assert!(!body.contains("No tracked files"));
}
