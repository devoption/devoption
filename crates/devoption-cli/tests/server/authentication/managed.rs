use super::profile::{check, read};
use super::*;
use devoption_projects::{ProjectId, read_catalog};
use std::os::unix::fs::PermissionsExt;
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
    assert!(location.starts_with(&format!("/projects/{}/code#repository-", hex(project))));
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
    assert!(!root.join("must-not-exist").exists());
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
    check(
        &request(address, &creation(address, &issued, project, "main")),
        409,
    );
    check(
        &request(
            address,
            &creation(address, &issued, ProjectId::from_bytes([0; 16]), "missing"),
        ),
        404,
    );
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), before);
    assert!(!root.join("service").exists());
    assert!(!root.join("missing").exists());
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
    assert!(root.join("unconfirmed/.git/HEAD").is_file());
    check(
        &request(address, &creation(address, &issued, project, "unconfirmed")),
        409,
    );
}
