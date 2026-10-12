#![cfg(unix)]

use devoption_server::{owner_auth, owner_store::OwnerStore};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink},
    process::Command,
};

#[test]
fn setup_rejects_redirected_input_and_unsafe_storage_without_mutation() {
    let temporary = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temporary.path()).unwrap();
    let data = root.join("data");
    fs::DirBuilder::new().mode(0o700).create(&data).unwrap();
    let insecure = root.join("insecure");
    fs::create_dir(&insecure).unwrap();
    fs::set_permissions(&insecure, fs::Permissions::from_mode(0o755)).unwrap();
    symlink(&data, root.join("alias")).unwrap();
    for path in [
        &data,
        &insecure,
        &root.join("missing"),
        &root.join("alias"),
        &data.join(".."),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_devoption"))
            .args(["setup", "--data-dir"])
            .arg(path)
            .env("DEVOPTION_PASSWORD", "ignored public fixture password")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(!error.contains("ignored public fixture"));
        assert!(!error.contains(&path.to_string_lossy().to_string()));
    }
    assert_eq!(fs::read_dir(&data).unwrap().count(), 0);
    assert_eq!(fs::metadata(insecure).unwrap().mode() & 0o777, 0o755);
    assert!(!root.join("missing").exists());
}

#[test]
fn terminal_setup_is_hidden_bounded_cancellable_and_creates_a_verifiable_owner() {
    let temporary = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temporary.path()).unwrap();
    let output = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/setup_terminal.py"
        ))
        .arg(env!("CARGO_BIN_EXE_devoption"))
        .arg(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let store = OwnerStore::open_absolute(&root.join("success")).unwrap();
    assert!(
        owner_auth::authenticate(&store, "  café fixture phrase 🔐  ")
            .unwrap()
            .is_some()
    );
    assert!(
        owner_auth::authenticate(&store, "café fixture phrase 🔐")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fs::metadata(root.join("success/owner.bin")).unwrap().mode() & 0o777,
        0o600
    );
}
