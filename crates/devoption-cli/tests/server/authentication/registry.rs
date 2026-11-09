use super::*;
use std::os::unix::fs::PermissionsExt;
fn root(directory: &Path) -> PathBuf {
    let path = directory.join("registry");
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    for child in ["candidates", "policy"] {
        fs::create_dir(path.join(child)).unwrap();
        fs::set_permissions(path.join(child), fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(
        path.join("config.toml"),
        "schema=1\nregistry='team'\nindex='sparse+https://packages.example.test/index/'\n",
    )
    .unwrap();
    fs::set_permissions(path.join("config.toml"), fs::Permissions::from_mode(0o600)).unwrap();
    path.canonicalize().unwrap()
}
#[test]
fn package_registry_cli_opt_in_uses_existing_owner_auth_and_preserves_default_disabled_state() {
    let (_temporary, data) = configured();
    let registry_parent = tempfile::tempdir().unwrap();
    let registry = root(registry_parent.path());
    let owner = fs::read(data.join("owner.bin")).unwrap();
    let (server, address) = start_with(&data);
    let issued = sign_in(address);
    let url = format!(
        "/projects/{}/registry/missing/0.1.0/status",
        "01".repeat(16)
    );
    profile::check(&profile::read(address, "GET", &url, Some(&issued)), 503);
    drop(server);
    assert_eq!(
        fs::read_dir(registry.join("candidates")).unwrap().count(),
        0
    );
    assert_eq!(fs::read_dir(registry.join("policy")).unwrap().count(), 0);
    let (_server, address) = start_command(
        Command::new(env!("CARGO_BIN_EXE_devoption"))
            .args(["serve", "--port", "0", "--data-dir"])
            .arg(&data)
            .arg("--package-registry-dir")
            .arg(&registry),
    );
    profile::check(&profile::read(address, "GET", &url, None), 303);
    let issued = sign_in(address);
    profile::check(&profile::read(address, "GET", &url, Some(&issued)), 404);
    let request = profile::save_request(address, &issued, "").replace(
        "POST /account/profile ",
        "POST /package-registry/recovery/prepare ",
    );
    profile::check(&super::super::request(address, &request), 409);
    assert_eq!(fs::read(data.join("owner.bin")).unwrap(), owner);
}
#[test]
fn unsafe_package_registry_configuration_is_refused_before_listening() {
    let (_temporary, data) = configured();
    let registry_parent = tempfile::tempdir().unwrap();
    let registry = root(registry_parent.path());
    fs::set_permissions(
        registry.join("config.toml"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_devoption"))
        .args(["serve", "--port", "0", "--data-dir"])
        .arg(&data)
        .arg("--package-registry-dir")
        .arg(&registry)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("package registry configuration or storage unavailable")
    );
    assert_eq!(
        fs::read_dir(registry.join("candidates")).unwrap().count(),
        0
    );
}
