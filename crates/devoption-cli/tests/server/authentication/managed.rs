use super::profile::{check, read};
use super::*;
use devoption_projects::{ProjectId, read_catalog};
use std::os::unix::fs::PermissionsExt;

// Model a device-number change in a disposable version-2 fixture only. Keep all
// paths, inodes, IDs and bytes outside these identity fields unchanged.
fn retired_device_record(bytes: &mut [u8]) {
    fn count(bytes: &[u8], offset: &mut usize) -> usize {
        let result = u16::from_le_bytes(bytes[*offset..*offset + 2].try_into().unwrap()) as usize;
        *offset += 2;
        result
    }
    fn string(bytes: &[u8], offset: &mut usize) {
        let len = count(bytes, offset);
        *offset += len;
    }
    fn identity(bytes: &mut [u8], offset: &mut usize) {
        bytes[*offset + 4] ^= 1;
        *offset += 16;
    }
    assert_eq!(&bytes[..9], b"DVPROJS\0\x02");
    let mut offset = 9;
    for _ in 0..count(bytes, &mut offset) {
        offset += 16;
        string(bytes, &mut offset);
    }
    for _ in 0..count(bytes, &mut offset) {
        offset += 16;
        string(bytes, &mut offset);
        offset += 16 * count(bytes, &mut offset);
    }
    string(bytes, &mut offset);
    identity(bytes, &mut offset);
    for _ in 0..count(bytes, &mut offset) {
        offset += 16;
        identity(bytes, &mut offset);
    }
    for _ in 0..count(bytes, &mut offset) {
        offset += 32;
        string(bytes, &mut offset);
        identity(bytes, &mut offset);
        identity(bytes, &mut offset);
    }
    assert_eq!(offset, bytes.len());
}

#[test]
fn reviewed_device_recovery_restores_managed_service_without_changing_owner_or_source() {
    let (_data, data) = configured();
    let (_root, root) = repositories::root_fixture();
    let (project, _) = repositories::seed(&data);
    let permitted = devoption_projects::worktree::PermittedRoot::open_absolute(&root).unwrap();
    devoption_projects::CatalogStore::open_absolute(&data)
        .unwrap()
        .attach_repository(project, &permitted, Path::new("main"))
        .unwrap();
    let owner = fs::read(data.join("owner.bin")).unwrap();
    let source = fs::read(root.join("main/.git/HEAD")).unwrap();
    let mut retired = fs::read(data.join("projects.bin")).unwrap();
    retired_device_record(&mut retired);
    fs::write(data.join("projects.bin"), &retired).unwrap();
    let blocked = Command::new(env!("CARGO_BIN_EXE_devoption"))
        .args(["serve", "--port", "0", "--data-dir"])
        .arg(&data)
        .arg("--managed-repository-root")
        .arg(&root)
        .output()
        .unwrap();
    assert_eq!(blocked.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("repository root does not match"));
    let recover = |approval: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_devoption"));
        command
            .args(["recover-repositories", "--data-dir"])
            .arg(&data)
            .arg("--repository-root")
            .arg(&root);
        if let Some(approval) = approval {
            command.args(["--approve-device-rebind", approval]);
        }
        command.output().unwrap()
    };
    let reviewed = recover(None);
    assert!(reviewed.status.success(), "{:?}", reviewed);
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), retired);
    let text = String::from_utf8(reviewed.stdout).unwrap();
    let fingerprint = text
        .lines()
        .find_map(|s| s.strip_prefix("Review fingerprint: "))
        .unwrap();
    assert!(text.contains("do NOT prove the same filesystem"));
    assert_eq!(recover(Some(&"0".repeat(64))).status.code(), Some(1));
    assert!(recover(Some(fingerprint)).status.success());
    let backup = data.join(format!("projects.before-rebind-{fingerprint}.bin"));
    assert_eq!(fs::read(backup).unwrap(), retired);
    assert_eq!(fs::read(data.join("owner.bin")).unwrap(), owner);
    assert_eq!(fs::read(root.join("main/.git/HEAD")).unwrap(), source);
    let (server, address) = start_managed(&data, &root);
    assert!(check(&read(address, "GET", "/sign-in", None), 200).contains("password"));
    let session = sign_in(address);
    assert!(
        check(
            &read(
                address,
                "GET",
                &format!("/projects/{}/code", hex(project)),
                Some(&session)
            ),
            200
        )
        .contains("main")
    );
    assert_eq!(
        check(
            &request(
                address,
                &creation(address, &session, project, "after-recovery")
            ),
            204
        ),
        ""
    );
    let saved = read_catalog(&data).unwrap().unwrap();
    drop(server);
    let (_server, address) = start_managed(&data, &root);
    let session = sign_in(address);
    assert!(
        check(
            &read(
                address,
                "GET",
                &format!("/projects/{}/code", hex(project)),
                Some(&session)
            ),
            200
        )
        .contains("after-recovery")
    );
    assert_eq!(read_catalog(&data).unwrap().unwrap(), saved);
}
fn hex(id: ProjectId) -> String {
    id.to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn start_managed(data: &Path, root: &Path) -> (Server, SocketAddr) {
    start_command(
        Command::new(env!("CARGO_BIN_EXE_devoption"))
            .args(["serve", "--port", "0", "--data-dir"])
            .arg(data)
            .arg("--managed-repository-root")
            .arg(root),
    )
}
fn creation(address: SocketAddr, issued: &Reply, project: ProjectId, name: &str) -> String {
    format!(
        "POST /projects/{}/repositories/create HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nCookie: {}\r\nX-CSRF-Token: {}\r\nContent-Type: text/plain;charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{name}",
        hex(project),
        issued
            .header("set-cookie")
            .unwrap()
            .split(';')
            .next()
            .unwrap(),
        issued.header("x-csrf-token").unwrap(),
        name.len()
    )
}
#[test]
fn explicit_managed_cli_mode_creates_and_registers_a_repository_that_survives_restart() {
    let (_data, data) = configured();
    let (_root, root) = repositories::root_fixture();
    let (project, _) = repositories::seed(&data);
    let owner = fs::read(data.join("owner.bin")).unwrap();
    let existing = fs::read(root.join("main/.git/config")).unwrap();
    let (server, address) = start_managed(&data, &root);
    let issued = sign_in(address);
    let page = read(
        address,
        "GET",
        &format!("/projects/{}/code", hex(project)),
        Some(&issued),
    );
    assert!(check(&page, 200).contains("id=\"repository-create-form\""));
    let response = request(address, &creation(address, &issued, project, "new-service"));
    assert_eq!(check(&response, 204), "");
    let location = response
        .split("\r\n")
        .find_map(|line| line.strip_prefix("location: "))
        .unwrap();
    assert!(location.starts_with(&format!("/projects/{}/repositories/", hex(project))));
    assert!(location.ends_with("/files"));
    assert!(
        check(&read(address, "GET", location, Some(&issued)), 200).contains("No tracked files")
    );
    let saved = read_catalog(&data).unwrap().unwrap();
    let repository = saved.projects()[0].repositories()[0];
    assert_eq!(saved.repositories()[0].name(), "new-service");
    let status_path = format!(
        "/projects/{}/repositories/{}/status",
        hex(project),
        repository
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    assert_eq!(
        check(&read(address, "GET", &status_path, Some(&issued)), 200),
        "main\n\n0\n0\n0\n0"
    );
    drop(server);
    let (_server, address) = start_managed(&data, &root);
    let session = sign_in(address);
    assert!(
        check(
            &read(
                address,
                "GET",
                &format!("/projects/{}/code", hex(project)),
                Some(&session)
            ),
            200
        )
        .contains("new-service")
    );
    assert_eq!(read_catalog(&data).unwrap().unwrap(), saved);
    assert_eq!(fs::read(data.join("owner.bin")).unwrap(), owner);
    assert_eq!(fs::read(root.join("main/.git/config")).unwrap(), existing);
}
#[test]
fn inspection_configuration_does_not_implicitly_grant_creation() {
    let (_data, data) = configured();
    let (_root, root) = repositories::root_fixture();
    let (project, _) = repositories::seed(&data);
    let (_server, address) = start_command(
        Command::new(env!("CARGO_BIN_EXE_devoption"))
            .args(["serve", "--port", "0", "--data-dir"])
            .arg(&data)
            .arg("--repository-root")
            .arg(&root),
    );
    let issued = sign_in(address);
    assert!(
        !check(
            &read(
                address,
                "GET",
                &format!("/projects/{}/code", hex(project)),
                Some(&issued)
            ),
            200
        )
        .contains("id=\"repository-create-form\"")
    );
    check(
        &request(
            address,
            &creation(address, &issued, project, "must-not-exist"),
        ),
        503,
    );
    check(
        &request(
            address,
            &creation(address, &issued, project, "must-not-exist")
                .replace("/repositories/create", "/repositories/starter"),
        ),
        503,
    );
    assert!(
        !root
            .join(format!("{}--must-not-exist", hex(project)))
            .exists()
    );
    assert!(
        read_catalog(&data)
            .unwrap()
            .unwrap()
            .repositories()
            .is_empty()
    );
}
#[test]
fn managed_mutations_reject_untrusted_transports_input_and_existing_paths() {
    let (_data, data) = configured();
    let (_root, root) = repositories::root_fixture();
    let (project, _) = repositories::seed(&data);
    let (_server, address) = start_managed(&data, &root);
    let issued = sign_in(address);
    let before = fs::read(data.join("projects.bin")).unwrap();
    let raw = creation(address, &issued, project, "service");
    for header in ["Cookie:", "X-CSRF-Token:"] {
        let denied = raw
            .split("\r\n")
            .filter(|line| !line.starts_with(header))
            .collect::<Vec<_>>()
            .join("\r\n");
        check(&request(address, &denied), 401);
    }
    check(
        &request(
            address,
            &raw.replace(
                "Connection:",
                "Authorization: Bearer fixture\r\nConnection:",
            ),
        ),
        401,
    );
    assert!(
        request(
            address,
            &raw.replace(
                &format!("Origin: http://{address}"),
                "Origin: https://attacker.invalid"
            )
        )
        .starts_with("HTTP/1.1 403")
    );
    for name in [
        "../escape",
        "/absolute",
        "two/parts",
        ".git",
        "with space",
        "é",
        "",
    ] {
        check(
            &request(address, &creation(address, &issued, project, name)),
            400,
        );
    }
    check(
        &request(
            address,
            &creation(address, &issued, project, &"x".repeat(65)),
        ),
        413,
    );
    check(
        &request(
            address,
            &raw.replace("text/plain;charset=UTF-8", "application/json"),
        ),
        415,
    );
    // A pre-existing scoped path is preserved, even without a saved association.
    let occupied = root.join(format!("{}--occupied", hex(project)));
    fs::write(&occupied, "preserved").unwrap();
    check(
        &request(address, &creation(address, &issued, project, "occupied")),
        409,
    );
    assert_eq!(fs::read(&occupied).unwrap(), b"preserved");
    check(
        &request(
            address,
            &creation(address, &issued, ProjectId::from_bytes([0; 16]), "missing"),
        ),
        404,
    );
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), before);
    assert!(!root.join(format!("{}--service", hex(project))).exists());
    assert!(!root.join(format!("{}--missing", "00".repeat(16))).exists());
    check(&request(address, &raw), 204);
    let saved = fs::read(data.join("projects.bin")).unwrap();
    check(&request(address, &raw), 409);
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), saved);
}
#[test]
fn concurrent_duplicate_requests_create_one_identity_and_failed_writes_remain_inspectable() {
    let (_data, data) = configured();
    let (_root, root) = repositories::root_fixture();
    let (project, _) = repositories::seed(&data);
    let (_server, address) = start_managed(&data, &root);
    let issued = sign_in(address);
    let responses = thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| request(address, &creation(address, &issued, project, "one"))))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        responses
            .iter()
            .filter(|r| r.starts_with("HTTP/1.1 204"))
            .count(),
        1
    );
    assert_eq!(
        responses
            .iter()
            .filter(|r| r.starts_with("HTTP/1.1 409"))
            .count(),
        3
    );
    assert_eq!(
        read_catalog(&data).unwrap().unwrap().repositories().len(),
        1
    );
    let saved = fs::read(data.join("projects.bin")).unwrap();
    fs::set_permissions(&data, fs::Permissions::from_mode(0o500)).unwrap();
    check(
        &request(address, &creation(address, &issued, project, "unconfirmed")),
        503,
    );
    fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), saved);
    assert!(
        root.join(format!("{}--unconfirmed/.git/HEAD", hex(project)))
            .is_file()
    );
    check(
        &request(address, &creation(address, &issued, project, "unconfirmed")),
        409,
    );
}

#[test]
fn cli_managed_projects_can_each_create_ui_and_browse_independent_files_after_restart() {
    let (_data, data) = configured();
    let (_root, root) = repositories::root_fixture();
    let (first, second) = repositories::seed(&data);
    let existing = fs::read(root.join("main/.git/config")).unwrap();
    let (server, address) = start_managed(&data, &root);
    let issued = sign_in(address);
    let mut locations = Vec::new();
    for project in [first, second] {
        let raw = creation(address, &issued, project, "ui");
        let response = request(address, &raw);
        check(&response, 204);
        locations.push(
            response
                .split("\r\n")
                .find_map(|line| line.strip_prefix("location: "))
                .unwrap()
                .to_owned(),
        );
        check(&request(address, &raw), 409);
    }
    let saved = read_catalog(&data).unwrap().unwrap();
    assert_eq!(saved.repositories().len(), 2);
    assert!(saved.repositories().iter().all(|r| r.name() == "ui"));
    assert_ne!(saved.repositories()[0].id(), saved.repositories()[1].id());
    let contents = ["First project only", "Second project only"];
    for (checkout, text) in saved.default_checkouts().iter().zip(contents) {
        let path = root.join(checkout.relative_path());
        fs::write(path.join("README.md"), text).unwrap();
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(path)
                .args(["add", "README.md"])
                .status()
                .unwrap()
                .success()
        );
    }
    fs::write(
        root.join(saved.default_checkouts()[0].relative_path())
            .join("note.txt"),
        "first only",
    )
    .unwrap();
    drop(server);
    let (_server, address) = start_managed(&data, &root);
    let issued = sign_in(address);
    for (index, (location, content)) in locations.iter().zip(contents).enumerate() {
        let page = read(
            address,
            "GET",
            &format!("{location}/README.md"),
            Some(&issued),
        );
        let page = check(&page, 200);
        assert!(page.contains(content));
        assert!(!page.contains(contents[1 - index]));
        let status = format!("{}/status", location.strip_suffix("/files").unwrap());
        assert_eq!(
            check(&read(address, "GET", &status, Some(&issued)), 200),
            format!("main\n\n1\n0\n{}\n0", usize::from(index == 0))
        );
    }
    let foreign = locations[1].replace(&hex(second), &hex(first));
    check(&read(address, "GET", &foreign, Some(&issued)), 404);
    assert_eq!(read_catalog(&data).unwrap().unwrap(), saved);
    assert_eq!(fs::read(root.join("main/.git/config")).unwrap(), existing);
}

#[test]
fn starter_requires_managed_owner_action_and_preserves_committed_source_across_restart() {
    let (_data, data) = configured();
    let (_root, root) = repositories::root_fixture();
    let (project, _) = repositories::seed(&data);
    let owner = fs::read(data.join("owner.bin")).unwrap();
    let existing = fs::read(root.join("main/.git/config")).unwrap();
    let hooks = root.join("ambient-hooks");
    fs::create_dir(&hooks).unwrap();
    let hook = hooks.join("post-commit");
    fs::write(&hook, "#!/bin/sh\ntouch ../ambient-hook-ran\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
    let (server, address) = start_command(
        Command::new(env!("CARGO_BIN_EXE_devoption"))
            .args(["serve", "--port", "0", "--data-dir"])
            .arg(&data)
            .arg("--managed-repository-root")
            .arg(&root)
            .env("GIT_CONFIG_COUNT", "2")
            .env("GIT_CONFIG_KEY_0", "core.hooksPath")
            .env("GIT_CONFIG_VALUE_0", &hooks)
            .env("GIT_CONFIG_KEY_1", "commit.gpgSign")
            .env("GIT_CONFIG_VALUE_1", "true")
            .env("GIT_AUTHOR_NAME", "Untrusted ambient identity"),
    );
    let issued = sign_in(address);
    let raw = creation(address, &issued, project, "web")
        .replace("/repositories/create", "/repositories/starter");
    let before = fs::read(data.join("projects.bin")).unwrap();
    for header in ["Cookie:", "X-CSRF-Token:"] {
        let denied = raw
            .split("\r\n")
            .filter(|line| !line.starts_with(header))
            .collect::<Vec<_>>()
            .join("\r\n");
        check(&request(address, &denied), 401);
    }
    check(
        &request(
            address,
            &raw.replace(
                &format!("Origin: http://{address}"),
                "Origin: https://attacker.invalid",
            ),
        ),
        403,
    );
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), before);
    let result = request(address, &raw);
    check(&result, 204);
    let location = result
        .split("\r\n")
        .find_map(|line| line.strip_prefix("location: "))
        .unwrap()
        .to_owned();
    assert!(location.starts_with(&format!("/projects/{}/builds/", hex(project))));
    let saved = read_catalog(&data).unwrap().unwrap();
    let checkout = root.join(saved.default_checkouts()[0].relative_path());
    let source = fs::read(checkout.join("src/lib.rs")).unwrap();
    assert!(!checkout.join("target").exists());
    assert!(!root.join("ambient-hook-ran").exists());
    check(&request(address, &raw), 409);
    let other = creation(address, &issued, project, "another")
        .replace("/repositories/create", "/repositories/starter");
    check(&request(address, &other), 409);
    assert!(!root.join(format!("{}--another", hex(project))).exists());
    drop(server);
    let (_server, address) = start_managed(&data, &root);
    let issued = sign_in(address);
    let page = read(address, "GET", &location, Some(&issued));
    let html = check(&page, 200);
    assert!(html.contains("Cargo.toml"));
    assert!(html.contains("Source checks passed"));
    assert!(html.contains("src/lib.rs"));
    assert_eq!(read_catalog(&data).unwrap().unwrap(), saved);
    assert_eq!(fs::read(checkout.join("src/lib.rs")).unwrap(), source);
    assert_eq!(fs::read(data.join("owner.bin")).unwrap(), owner);
    assert_eq!(fs::read(root.join("main/.git/config")).unwrap(), existing);
}
