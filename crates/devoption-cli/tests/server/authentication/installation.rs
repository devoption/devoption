//! Unprivileged source-install fixtures; no real home directory or system service.
use super::*;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

#[path = "installation/catalog.rs"]
mod catalog;

// Linux can temporarily reject a copied executable while a concurrent child
// retains an inherited writable descriptor before exec. Retry only this kernel
// error, only in copied-binary fixtures, at most 50 times (one second of waits).
// Permanent failures and every other error still fail the test.
fn copied_binary_operation<T>(mut operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut retries = 0;
    loop {
        match operation() {
            Err(error) if error.kind() == io::ErrorKind::ExecutableFileBusy && retries < 50 => {
                retries += 1;
                thread::sleep(Duration::from_millis(20));
            }
            result => return result,
        }
    }
}

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
    command.stdout(Stdio::piped());
    started_child(copied_binary_operation(|| command.spawn()).unwrap())
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
    let output = copied_binary_operation(|| command.output()).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr).unwrap().contains(expected));
}

#[test]
fn copied_binary_retries_are_bounded_and_preserve_other_errors() {
    let mut attempts = 0;
    let error = copied_binary_operation::<()>(|| {
        attempts += 1;
        Err(io::ErrorKind::ExecutableFileBusy.into())
    })
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::ExecutableFileBusy);
    assert_eq!(attempts, 51);
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::NotFound] {
        let mut attempts = 0;
        let error = copied_binary_operation::<()>(|| {
            attempts += 1;
            Err(kind.into())
        })
        .unwrap_err();
        assert_eq!(error.kind(), kind);
        assert_eq!(attempts, 1);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn copied_binary_launch_recovers_after_writable_descriptor_closes() {
    let (_temporary, root) = data_directory::fixture();
    let binary = installed_binary(&root);
    let mut writer = Some(fs::OpenOptions::new().write(true).open(&binary).unwrap());
    let mut command = Command::new(&binary);
    command.arg("--help");
    let mut attempts = 0;
    let output = copied_binary_operation(|| {
        attempts += 1;
        let result = command.output();
        if attempts == 1 {
            assert!(
                matches!(&result, Err(error) if error.kind() == io::ErrorKind::ExecutableFileBusy)
            );
            drop(writer.take());
        }
        result
    })
    .unwrap();
    assert!(attempts >= 2);
    assert!(output.status.success());
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
