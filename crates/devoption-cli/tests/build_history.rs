#![cfg(unix)]
use devoption_server::{owner_auth, owner_store::OwnerStore};
use std::{fs, os::unix::fs::DirBuilderExt, process::Command};

#[test]
fn explicit_history_startup_recovers_before_listen_and_retains_writer() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let data = root.join("owner");
    fs::DirBuilder::new().mode(0o700).create(&data).unwrap();
    let owner = OwnerStore::open_absolute(&data).unwrap();
    owner_auth::setup(&owner, "public disposable startup fixture phrase").unwrap();
    drop(owner);
    let mut catalog = devoption_projects::Catalog::default();
    let project = catalog.create_project("Build fixture").unwrap();
    let repository = catalog.create_repository("Source fixture").unwrap();
    let unrelated = catalog.create_repository("Unassociated fixture").unwrap();
    catalog.associate(project, repository).unwrap();
    let mut store = devoption_projects::CatalogStore::open_absolute(&data).unwrap();
    store.save(&catalog).unwrap();
    drop(store);
    let hex = |bytes: [u8; 16]| {
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let output = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/build_history.py"
        ))
        .arg(env!("CARGO_BIN_EXE_devoption"))
        .arg(root)
        .arg(hex(project.to_bytes()))
        .arg(hex(repository.to_bytes()))
        .arg(hex(unrelated.to_bytes()))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn history_option_requires_absolute_unique_path_and_data_directory() {
    for args in [
        vec!["--build-history-dir"],
        vec!["--build-history-dir", "/missing"],
        vec!["--data-dir", "/missing", "--build-history-dir", "relative"],
        vec![
            "--data-dir",
            "/missing",
            "--build-history-dir",
            "/one",
            "--build-history-dir",
            "/two",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_devoption"))
            .arg("serve")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}
