//! Unprivileged source-install fixtures; no real home directory or system service.
use super::*;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

#[test]
fn copied_binary_restores_a_private_backup_without_restoring_sessions() {
    let (_directory, data) = configured();
    let original = fs::read(data.join("owner.bin")).unwrap();
    let (installation, root) = data_directory::fixture();
    let binary = root.join("devoption");
    fs::copy(env!("CARGO_BIN_EXE_devoption"), &binary).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    let start_installed = |path: &Path| {
        start_command(
            Command::new(&binary)
                .current_dir(&root)
                .args(["serve", "--port", "0", "--data-dir"])
                .arg(path),
        )
    };
    let (server, address) = start_installed(&data);
    let session = sign_in(address);
    drop(server);

    // Copy only after stopping; keep the source and backup unchanged throughout.
    let backup = root.join("backup");
    let restored = root.join("restored");
    for path in [&backup, &restored] {
        fs::DirBuilder::new().mode(0o700).create(path).unwrap();
    }
    fs::copy(data.join("owner.bin"), backup.join("owner.bin")).unwrap();
    fs::copy(backup.join("owner.bin"), restored.join("owner.bin")).unwrap();
    let (server, address) = start_installed(&restored);
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
    let output = Command::new(&binary)
        .args(["serve", "--port", "0", "--data-dir"])
        .arg(&restored)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("data directory unavailable:")
    );
    assert_eq!(
        fs::read(restored.join("owner.bin")).unwrap(),
        b"damaged fixture backup"
    );
    assert_eq!(fs::read(backup.join("owner.bin")).unwrap(), original);
    drop(installation);
}
