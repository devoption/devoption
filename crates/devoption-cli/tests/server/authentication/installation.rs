//! Unprivileged source-install fixtures; no real home directory or system service.
use super::*;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

#[path = "installation/catalog.rs"]
mod catalog;

fn installed_binary(root: &Path) -> PathBuf {
    let binary = root.join("devoption");
    fs::copy(env!("CARGO_BIN_EXE_devoption"), &binary).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    binary
}

fn start_installed(
    binary: &Path,
    data: &Path,
    repository_root: Option<&Path>,
) -> (Server, SocketAddr) {
    let mut command = Command::new(binary);
    command
        .current_dir(binary.parent().unwrap())
        .args(["serve", "--port", "0", "--data-dir"])
        .arg(data);
    if let Some(root) = repository_root {
        command.arg("--repository-root").arg(root);
    }
    start_command(&mut command)
}

fn guide_copy(operation: &str, source: &Path, destination: &Path) -> std::process::Output {
    let guide = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/private-linux-install.md"
    ));
    let script = guide
        .split_once(&format!("# BEGIN {operation}\n"))
        .unwrap()
        .1
        .split_once(&format!("# END {operation}"))
        .unwrap()
        .0;
    Command::new("sh")
        .args(["-eu", "-c", &format!("umask 077\n{script}")])
        .env("devoption_data", source)
        .env(
            "backup",
            if operation == "data-backup" {
                destination
            } else {
                source
            },
        )
        .env("restored", destination)
        .output()
        .unwrap()
}

fn startup_rejected(binary: &Path, data: &Path, repository_root: Option<&Path>, expected: &str) {
    // An occupied port keeps a validation regression from starting a server
    // that leaves output() waiting forever; require the intended storage error.
    let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let mut command = Command::new(binary);
    command
        .args([
            "serve",
            "--port",
            &occupied.local_addr().unwrap().port().to_string(),
            "--data-dir",
        ])
        .arg(data);
    if let Some(root) = repository_root {
        command.arg("--repository-root").arg(root);
    }
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr).unwrap().contains(expected));
}

#[test]
fn copied_binary_restores_a_private_backup_without_restoring_sessions() {
    let (_directory, data) = configured();
    let original = fs::read(data.join("owner.bin")).unwrap();
    let (installation, root) = data_directory::fixture();
    let binary = installed_binary(&root);
    let (server, address) = start_installed(&binary, &data, None);
    let session = sign_in(address);
    drop(server);

    // Copy only after stopping; keep the source and backup unchanged throughout.
    let backup = root.join("backup");
    let restored = root.join("restored");
    for path in [&backup, &restored] {
        fs::DirBuilder::new().mode(0o700).create(path).unwrap();
    }
    assert!(guide_copy("data-backup", &data, &backup).status.success());
    assert_eq!(fs::read(backup.join("catalog-state")).unwrap(), b"absent\n");
    assert!(
        guide_copy("data-restore", &backup, &restored)
            .status
            .success()
    );
    assert!(!restored.join("projects.bin").exists());
    let backed_up_binary = backup.join("devoption");
    fs::copy(&binary, &backed_up_binary).unwrap();
    let (server, address) = start_installed(&backed_up_binary, &restored, None);
    assert_eq!(
        post(address, "/auth/session", Some(&session), "").status,
        401
    );
    let restored_session = sign_in(address);
    assert_eq!(
        post(address, "/auth/session", Some(&restored_session), "").status,
        204
    );
    drop(server);
    for path in [&data, &backup, &restored] {
        assert_eq!(fs::read(path.join("owner.bin")).unwrap(), original);
        assert_eq!(fs::metadata(path).unwrap().mode() & 0o777, 0o700);
        assert_eq!(
            fs::metadata(path.join("owner.bin")).unwrap().mode() & 0o777,
            0o600
        );
    }

    // A damaged restore must fail before listening, without repairing the record.
    fs::write(restored.join("owner.bin"), b"damaged fixture backup").unwrap();
    startup_rejected(
        &backed_up_binary,
        &restored,
        None,
        "data directory unavailable:",
    );
    assert_eq!(
        fs::read(restored.join("owner.bin")).unwrap(),
        b"damaged fixture backup"
    );
    assert_eq!(fs::read(backup.join("owner.bin")).unwrap(), original);
    drop(installation);
}
