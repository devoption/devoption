use super::*;
use devoption_cli::{
    identity::IdentityId,
    owner_store::OwnerStore,
    password::{SALT_BYTES, hash_password},
};
use std::{
    ffi::OsString,
    fs::{self, DirBuilder, OpenOptions},
    os::unix::{
        ffi::OsStringExt,
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
    },
    path::{Path, PathBuf},
};

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let temporary = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    // macOS's temporary root can contain an OS-provided symlink. The CLI requires
    // the actual path; resolution here prepares only this test's fixture input.
    let root = temporary.path().canonicalize().unwrap();
    (temporary, root)
}

fn private_file(path: &Path, bytes: &[u8]) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

fn rejected_before_listening(path: &Path) {
    // An occupied port proves storage validation happens before attempting bind:
    // otherwise the CLI would report the address conflict instead of storage.
    let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_devoption"))
        .args([
            "serve",
            "--port",
            &occupied.local_addr().unwrap().port().to_string(),
            "--data-dir",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("data directory unavailable:"), "{error}");
    assert!(!error.contains(&path.to_string_lossy().to_string()));
    assert!(!error.contains("private fixture record"));
}

#[test]
fn configured_preview_accepts_both_option_orders_without_mutating_owner_data() {
    let (_temporary, root) = fixture();
    // Linux supports arbitrary filename bytes; macOS rejects invalid UTF-8 names.
    #[cfg(target_os = "linux")]
    let data = root.join(OsString::from_vec(b"private-fixture-\xff".to_vec()));
    #[cfg(not(target_os = "linux"))]
    let data = root.join("private-fixture");
    DirBuilder::new().mode(0o700).create(&data).unwrap();
    let store = OwnerStore::open_absolute(&data).unwrap();
    for existing_owner in [false, true] {
        if existing_owner {
            let hash = hash_password("password fixture only", &[0x5a; SALT_BYTES]).unwrap();
            store
                .create(IdentityId::from_bytes([0x12; 16]), &hash)
                .unwrap();
        }
        let before = fs::read(data.join("owner.bin")).ok();
        let mut command = Command::new(env!("CARGO_BIN_EXE_devoption"));
        command.arg("serve");
        if existing_owner {
            command.arg("--data-dir").arg(&data).args(["--port", "0"]);
        } else {
            command.args(["--port", "0", "--data-dir"]).arg(&data);
        }
        let (server, address) = start_command(&mut command);
        assert!(
            request(
                address,
                &format!("GET / HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
            )
            .starts_with("HTTP/1.1 200 OK")
        );
        assert!(request(address, &format!(
            "POST /setup HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )).starts_with("HTTP/1.1 401 Unauthorized"));
        drop(server);
        assert_eq!(fs::read(data.join("owner.bin")).ok(), before);
        assert_eq!(
            fs::read_dir(&data).unwrap().count(),
            usize::from(existing_owner)
        );
        assert_eq!(fs::metadata(&data).unwrap().mode() & 0o777, 0o700);
    }
}

#[test]
fn non_utf8_missing_path_fails_as_storage_input_without_panicking() {
    let (_temporary, root) = fixture();
    let missing = root.join(OsString::from_vec(b"missing-\xff".to_vec()));
    rejected_before_listening(&missing);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
}

#[test]
fn missing_insecure_symlink_and_parent_paths_fail_without_repairs_or_creation() {
    let (_temporary, root) = fixture();
    let insecure = root.join("insecure");
    fs::create_dir(&insecure).unwrap();
    fs::set_permissions(&insecure, fs::Permissions::from_mode(0o755)).unwrap();
    let data = root.join("data");
    DirBuilder::new().mode(0o700).create(&data).unwrap();
    symlink(&data, root.join("alias")).unwrap();
    symlink(&root, root.join("ancestor")).unwrap();
    private_file(&root.join("regular-file"), b"fixture");
    for path in [
        root.join("missing"),
        insecure.clone(),
        root.join("alias/./"),
        root.join("ancestor/data"),
        data.join(".."),
        root.join("regular-file"),
    ] {
        rejected_before_listening(&path);
    }
    assert!(!root.join("missing").exists());
    assert_eq!(fs::metadata(&insecure).unwrap().mode() & 0o777, 0o755);
    assert_eq!(fs::read_dir(&data).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 5);
}

#[test]
fn invalid_owner_records_fail_closed_without_resetting_or_exposing_data() {
    let (_temporary, root) = fixture();
    let record = root.join("owner.bin");
    private_file(&record, b"private fixture record");
    let original = fs::read(&record).unwrap();
    rejected_before_listening(&root);
    assert_eq!(fs::read(&record).unwrap(), original);

    fs::remove_file(&record).unwrap();
    let store = OwnerStore::open_absolute(&root).unwrap();
    let hash = hash_password("password fixture only", &[0x5a; SALT_BYTES]).unwrap();
    store
        .create(IdentityId::from_bytes([0x12; 16]), &hash)
        .unwrap();
    let valid = fs::read(&record).unwrap();
    let mut unsupported = valid.clone();
    unsupported[8] = 99;
    fs::write(&record, &unsupported).unwrap();
    rejected_before_listening(&root);
    assert_eq!(fs::read(&record).unwrap(), unsupported);

    fs::write(&record, &valid).unwrap();
    fs::set_permissions(&record, fs::Permissions::from_mode(0o644)).unwrap();
    rejected_before_listening(&root);
    assert_eq!(fs::metadata(&record).unwrap().mode() & 0o777, 0o644);
    assert_eq!(fs::read(&record).unwrap(), valid);

    fs::rename(&record, root.join("saved-owner")).unwrap();
    symlink(root.join("saved-owner"), &record).unwrap();
    rejected_before_listening(&root);
    assert!(fs::symlink_metadata(&record).unwrap().is_symlink());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 2);

    fs::remove_file(&record).unwrap();
    fs::create_dir(&record).unwrap();
    rejected_before_listening(&root);
    assert!(record.is_dir());
}
