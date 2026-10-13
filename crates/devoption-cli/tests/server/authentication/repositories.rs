use super::*;
use devoption_projects::{Catalog, CatalogStore, ProjectId, read_catalog};
use std::os::unix::fs::{PermissionsExt, symlink};

fn directory(path: &Path) {
    if path.is_dir() {
        return;
    }
    directory(path.parent().unwrap());
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

pub(super) fn root_fixture() -> (tempfile::TempDir, PathBuf) {
    let (temporary, root) = data_directory::fixture();
    for name in ["main", "other"] {
        directory(&root.join(name).join(".git/objects"));
        directory(&root.join(name).join(".git/refs"));
        fs::write(
            root.join(name).join(".git/config"),
            "[core]\nrepositoryformatversion = 0\nbare = false\n",
        )
        .unwrap();
        fs::write(root.join(name).join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    }
    directory(&root.join("linked"));
    let git = root.join("main/.git/worktrees/linked");
    directory(&git);
    fs::write(
        root.join("linked/.git"),
        "gitdir: ../main/.git/worktrees/linked\n",
    )
    .unwrap();
    fs::write(git.join("commondir"), "../..\n").unwrap();
    fs::write(git.join("gitdir"), "../../../../linked/.git\n").unwrap();
    fs::write(git.join("HEAD"), "ref: refs/heads/feature\n").unwrap();
    (temporary, root)
}

pub(super) fn seed(data: &Path) -> (ProjectId, ProjectId) {
    let mut catalog = Catalog::default();
    let first = catalog.create_project("First fixture").unwrap();
    let second = catalog.create_project("Second fixture").unwrap();
    CatalogStore::open_absolute(data)
        .unwrap()
        .save(&catalog)
        .unwrap();
    (first, second)
}

fn start_root(data: &Path, root: &Path) -> (Server, SocketAddr) {
    start_command(
        Command::new(env!("CARGO_BIN_EXE_devoption"))
            .args(["serve", "--port", "0", "--data-dir"])
            .arg(data)
            .arg("--repository-root")
            .arg(root),
    )
}

fn attachment(address: SocketAddr, issued: &Reply, project: ProjectId, path: &str) -> String {
    let project: String = project
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!(
        "POST /projects/{project}/repositories HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nCookie: {}\r\nX-CSRF-Token: {}\r\nContent-Type: text/plain;charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{path}",
        issued
            .header("set-cookie")
            .unwrap()
            .split(';')
            .next()
            .unwrap(),
        issued.header("x-csrf-token").unwrap(),
        path.len()
    )
}

fn status(address: SocketAddr, raw: &str) -> u16 {
    let response = request(address, raw);
    assert!(response.contains("cache-control: no-store"));
    assert!(!response.contains("set-cookie:"));
    assert!(!response.contains("x-csrf-token:"));
    response.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
fn attachments_share_identity_support_multiple_repositories_and_survive_process_restart() {
    let (_data, data) = configured();
    let (_root, root) = root_fixture();
    let (first, second) = seed(&data);
    let owner = fs::read(data.join("owner.bin")).unwrap();
    let (server, address) = start_root(&data, &root);
    let issued = sign_in(address);
    assert_eq!(
        status(address, &attachment(address, &issued, first, "main")),
        204
    );
    assert_eq!(
        status(address, &attachment(address, &issued, second, "linked")),
        204
    );
    assert_eq!(
        status(address, &attachment(address, &issued, first, "linked")),
        409
    );
    assert_eq!(
        status(address, &attachment(address, &issued, first, "other")),
        204
    );
    let saved = read_catalog(&data).unwrap().unwrap();
    assert_eq!(saved.repositories().len(), 2);
    assert_eq!(saved.projects()[0].repositories().len(), 2);
    assert_eq!(
        saved.projects()[1].repositories(),
        &saved.projects()[0].repositories()[..1]
    );
    assert_eq!(
        saved.default_checkouts()[1].relative_path(),
        Path::new("linked")
    );
    drop(server);
    let (server, address) = start_root(&data, &root);
    assert_eq!(
        status(
            address,
            &attachment(address, &sign_in(address), second, "main")
        ),
        409
    );
    assert_eq!(read_catalog(&data).unwrap().unwrap(), saved);
    drop(server);
    // Omission disables attachment without rewriting or discarding saved data.
    let (_server, address) = start_with(&data);
    assert_eq!(
        status(
            address,
            &attachment(address, &sign_in(address), first, "other")
        ),
        503
    );
    assert_eq!(read_catalog(&data).unwrap().unwrap(), saved);
    assert_eq!(fs::read(data.join("owner.bin")).unwrap(), owner);
}

#[test]
fn attachment_guards_reject_missing_cookie_csrf_origin_and_mixed_authorization_before_storage() {
    let (_data, data) = configured();
    let (_root, root) = root_fixture();
    let (first, _) = seed(&data);
    let before = fs::read(data.join("projects.bin")).unwrap();
    fs::remove_file(data.join(".projects.lock")).unwrap();
    let (_server, address) = start_root(&data, &root);
    let issued = sign_in(address);
    let raw = attachment(address, &issued, first, "main");
    for header in ["Cookie:", "X-CSRF-Token:"] {
        let removed = raw
            .split("\r\n")
            .filter(|line| !line.starts_with(header))
            .collect::<Vec<_>>()
            .join("\r\n");
        assert_eq!(status(address, &removed), 401);
    }
    for value in ["", "Bearer fixture", "Basic fixture"] {
        assert_eq!(
            status(
                address,
                &raw.replacen(
                    "Connection:",
                    &format!("Authorization: {value}\r\nConnection:"),
                    1
                )
            ),
            401
        );
    }
    assert_eq!(
        status(
            address,
            &raw.replace(issued.header("x-csrf-token").unwrap(), &"00".repeat(32))
        ),
        401
    );
    for origin in ["", "Origin: https://attacker.invalid\r\n"] {
        let response = request(
            address,
            &raw.replace(&format!("Origin: http://{address}\r\n"), origin),
        );
        assert!(response.starts_with("HTTP/1.1 403"));
        assert!(response.split_once("\r\n\r\n").unwrap().1.is_empty());
    }
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), before);
    assert!(!data.join(".projects.lock").exists());
}

#[test]
fn attachment_input_and_storage_errors_never_echo_paths_or_report_success() {
    let (_data, data) = configured();
    let (_root, root) = root_fixture();
    let (first, _) = seed(&data);
    let before = fs::read(data.join("projects.bin")).unwrap();
    let (_server, address) = start_root(&data, &root);
    let issued = sign_in(address);
    for candidate in [
        "",
        "../private-fixture",
        "/private-fixture",
        "main/.",
        "main//nested",
        "main\n",
        "missing",
    ] {
        let raw = attachment(address, &issued, first, candidate);
        let response = request(address, &raw);
        assert!(response.starts_with("HTTP/1.1 400"));
        assert!(!response.contains("private-fixture"));
        assert!(!response.contains(root.to_str().unwrap()));
    }
    assert_eq!(
        status(
            address,
            &attachment(address, &issued, first, &"x".repeat(4097))
        ),
        413
    );
    let raw = attachment(address, &issued, first, "main");
    assert_eq!(
        status(
            address,
            &raw.replace("text/plain;charset=UTF-8", "application/json")
        ),
        415
    );
    assert_eq!(
        status(
            address,
            &raw.replace(
                &first
                    .to_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
                "invalid"
            )
        ),
        400
    );
    assert_eq!(
        status(
            address,
            &attachment(address, &issued, ProjectId::from_bytes([0; 16]), "main")
        ),
        404
    );
    let writer = CatalogStore::open_absolute(&data).unwrap();
    let unavailable = request(address, &raw);
    assert!(unavailable.starts_with("HTTP/1.1 503"));
    assert!(unavailable.contains("x-devoption-error: unavailable\r\n"));
    drop(writer);
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), before);
    fs::write(
        data.join("projects.bin"),
        b"private fixture corrupt catalog",
    )
    .unwrap();
    let unavailable = request(address, &raw);
    assert!(unavailable.starts_with("HTTP/1.1 503"));
    assert!(unavailable.contains("x-devoption-error: unavailable\r\n"));
    assert_eq!(
        fs::read(data.join("projects.bin")).unwrap(),
        b"private fixture corrupt catalog"
    );
}

#[test]
fn concurrent_attachment_requests_save_each_association_once() {
    let (_data, data) = configured();
    let (_root, root) = root_fixture();
    let (first, second) = seed(&data);
    let (_server, address) = start_root(&data, &root);
    let issued = sign_in(address);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let statuses = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for index in 0..8 {
            let barrier = barrier.clone();
            let issued = &issued;
            handles.push(scope.spawn(move || {
                barrier.wait();
                status(
                    address,
                    &attachment(
                        address,
                        issued,
                        if index % 2 == 0 { first } else { second },
                        if index % 2 == 0 { "main" } else { "linked" },
                    ),
                )
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(statuses.iter().filter(|status| **status == 204).count(), 2);
    assert_eq!(statuses.iter().filter(|status| **status == 409).count(), 6);
    let saved = read_catalog(&data).unwrap().unwrap();
    assert_eq!(saved.repositories().len(), 1);
    assert_eq!(saved.default_checkouts().len(), 2);
}

fn startup_rejected(data: &Path, root: &Path, expected: &str) {
    let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_devoption"))
        .args([
            "serve",
            "--port",
            &occupied.local_addr().unwrap().port().to_string(),
            "--data-dir",
        ])
        .arg(data)
        .arg("--repository-root")
        .arg(root)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains(expected), "{error}");
    assert!(!error.contains(root.to_str().unwrap()));
}

#[test]
fn root_configuration_requires_existing_owner_safe_disjoint_root_and_saved_binding() {
    let (_data, data) = configured();
    let (_root, root) = root_fixture();
    let (_empty, empty) = data_directory::fixture();
    startup_rejected(&empty, &root, "requires an existing configured owner");
    assert!(fs::read_dir(&empty).unwrap().next().is_none());
    for bad in [
        &data,
        data.parent().unwrap(),
        &data.join("missing"),
        &root.join(".."),
        &root.join("missing"),
    ] {
        startup_rejected(&data, bad, "project catalog unavailable");
    }
    directory(&data.join("nested"));
    startup_rejected(&data, &data.join("nested"), "project catalog unavailable");
    symlink(&root, data.join("alias")).unwrap();
    startup_rejected(&data, &data.join("alias"), "project catalog unavailable");
    let (first, _) = seed(&data);
    let original = fs::read(data.join("projects.bin")).unwrap();
    let (server, _) = start_root(&data, &root);
    drop(server);
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), original);
    CatalogStore::open_absolute(&data)
        .unwrap()
        .attach_repository(
            first,
            &devoption_projects::worktree::PermittedRoot::open_absolute(&root).unwrap(),
            Path::new("main"),
        )
        .unwrap();
    let saved = fs::read(data.join("projects.bin")).unwrap();
    let (_different, different) = root_fixture();
    startup_rejected(&data, &different, "project catalog unavailable");
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), saved);
}

#[test]
fn replaced_live_repository_root_returns_conflict_without_rebinding_or_writing() {
    for registered in [false, true] {
        let (_data, data) = configured();
        let (_root, root) = root_fixture();
        let (_replacement, replacement) = root_fixture();
        let (first, second) = seed(&data);
        let (_server, address) = start_root(&data, &root);
        let issued = sign_in(address);
        if registered {
            assert_eq!(
                status(address, &attachment(address, &issued, first, "main")),
                204
            );
        }
        let saved = fs::read(data.join("projects.bin")).unwrap();
        let owner = fs::read(data.join("owner.bin")).unwrap();
        // Keep the original inodes alive inside the replacement fixture. The
        // configured path now contains another valid checkout tree.
        fs::rename(&root, replacement.join("original-root")).unwrap();
        fs::rename(&replacement, &root).unwrap();
        let response = request(address, &attachment(address, &issued, second, "linked"));
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("HTTP/1.1 409"), "{headers}");
        assert!(headers.contains("cache-control: no-store"));
        assert_eq!(
            body,
            "Repository association conflicts with saved state or catalog capacity."
        );
        assert_eq!(fs::read(data.join("projects.bin")).unwrap(), saved);
        assert_eq!(fs::read(data.join("owner.bin")).unwrap(), owner);
    }
}

#[test]
fn malformed_project_segments_use_fixed_noncacheable_errors_after_authentication() {
    let (_data, data) = configured();
    let (_root, root) = root_fixture();
    let (first, _) = seed(&data);
    let saved = fs::read(data.join("projects.bin")).unwrap();
    fs::remove_file(data.join(".projects.lock")).unwrap();
    let (_server, address) = start_root(&data, &root);
    let issued = sign_in(address);
    let raw = attachment(address, &issued, first, "main");
    let id: String = first
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    for segment in ["%FF", "%C3%28", "%", "%GG", "%2Fprivate-fixture", "invalid"] {
        let malformed = raw.replacen(
            &format!("/projects/{id}/"),
            &format!("/projects/{segment}/"),
            1,
        );
        let response = request(address, &malformed);
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("HTTP/1.1 400"), "{headers}");
        assert!(headers.contains("cache-control: no-store"), "{headers}");
        assert_eq!(body, "Repository checkout could not be validated.");
        // Route extraction must not supersede the existing cookie/CSRF guard.
        let anonymous = malformed
            .split("\r\n")
            .filter(|line| !line.starts_with("Cookie:"))
            .collect::<Vec<_>>()
            .join("\r\n");
        let denied = request(address, &anonymous);
        assert!(denied.starts_with("HTTP/1.1 401"));
        assert!(denied.split_once("\r\n\r\n").unwrap().1.is_empty());
    }
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), saved);
    assert!(!data.join(".projects.lock").exists());
}

#[test]
fn projects_render_contextual_forms_fresh_repository_lists_and_fixed_failure_codes() {
    let (_data, data) = configured();
    let (_root, root) = root_fixture();
    let (first, _) = seed(&data);
    let (server, address) = start_root(&data, &root);
    let issued = sign_in(address);
    let page = |address: SocketAddr, issued: &Reply| {
        request(
            address,
            &format!(
                "GET /projects HTTP/1.1\r\nHost: {address}\r\nCookie: {}\r\nConnection: close\r\n\r\n",
                issued
                    .header("set-cookie")
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap()
            ),
        )
    };
    let initial = page(address, &issued);
    assert_eq!(
        initial
            .matches("class=\"project-form repository-form\"")
            .count(),
        2
    );
    assert!(initial.contains("data-repository-path"));
    assert!(!initial.contains(root.to_str().unwrap()));
    let raw = attachment(address, &issued, first, "main");
    assert_eq!(status(address, &raw), 204);
    let saved = page(address, &issued);
    assert!(saved.contains("<p>1 repository</p>"));
    assert!(saved.contains("Default checkout: <code>main</code>"));
    for (raw, code) in [
        (raw.clone(), "already-associated"),
        (
            attachment(address, &issued, first, "../outside"),
            "invalid-checkout",
        ),
        (
            attachment(address, &issued, ProjectId::from_bytes([0; 16]), "main"),
            "project-missing",
        ),
    ] {
        assert!(request(address, &raw).contains(&format!("x-devoption-error: {code}\r\n")));
    }
    fs::rename(root.join("main"), root.join("old-main")).unwrap();
    fs::rename(root.join("other"), root.join("main")).unwrap();
    assert!(request(address, &raw).contains("x-devoption-error: checkout-changed\r\n"));
    drop(server);
    let (_server, address) = start_with(&data);
    let issued = sign_in(address);
    let disabled = page(address, &issued);
    assert!(disabled.contains("Repository attachment is not configured on this server"));
    assert!(!disabled.contains("class=\"project-form repository-form\""));
    assert!(disabled.contains("Default checkout: <code>main</code>"));
    assert!(
        request(address, &attachment(address, &issued, first, "main"))
            .contains("x-devoption-error: root-unconfigured\r\n")
    );
}
