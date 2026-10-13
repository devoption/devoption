use super::*;
use devoption_projects::{CatalogStore, read_catalog, worktree::PermittedRoot};
use std::os::unix::fs::symlink;

#[test]
fn offline_catalog_backup_preserves_shared_identities_and_default_checkouts() {
    let (_owner, data) = configured();
    let (_repositories, repository_root) = repositories::root_fixture();
    let (first, second) = repositories::seed(&data);
    let permitted = PermittedRoot::open_absolute(&repository_root).unwrap();
    {
        let mut store = CatalogStore::open_absolute(&data).unwrap();
        let shared = store
            .attach_repository(first, &permitted, Path::new("main"))
            .unwrap();
        assert_eq!(
            store
                .attach_repository(second, &permitted, Path::new("linked"))
                .unwrap(),
            shared
        );
        store
            .attach_repository(first, &permitted, Path::new("other"))
            .unwrap();
    }
    let expected = read_catalog(&data).unwrap().unwrap();
    assert_eq!(expected.repositories().len(), 2);
    assert_eq!(expected.projects()[0].repositories().len(), 2);
    assert_eq!(expected.default_checkouts().len(), 3);
    let owner_bytes = fs::read(data.join("owner.bin")).unwrap();
    let catalog_bytes = fs::read(data.join("projects.bin")).unwrap();
    assert_eq!(catalog_bytes[8], 2);
    fs::write(
        data.join(".projects-interrupted.tmp"),
        b"unpublished fixture",
    )
    .unwrap();
    let (_installation, root) = data_directory::fixture();
    let binary = installed_binary(&root);
    let (server, address) = start_installed(&binary, &data, Some(&repository_root));
    let old_session = sign_in(address);
    drop(server);

    let backup = root.join("backup");
    let restored = root.join("restored");
    for path in [&backup, &restored] {
        fs::DirBuilder::new().mode(0o700).create(path).unwrap();
    }
    assert!(guide_copy("data-backup", &data, &backup).status.success());
    assert_eq!(
        fs::read(backup.join("catalog-state")).unwrap(),
        b"present\n"
    );
    assert!(
        guide_copy("data-restore", &backup, &restored)
            .status
            .success()
    );
    assert_eq!(fs::read_dir(&backup).unwrap().count(), 3);
    assert_eq!(fs::read_dir(&restored).unwrap().count(), 2);
    let backed_up_binary = backup.join("devoption");
    fs::copy(&binary, &backed_up_binary).unwrap();
    for selected_root in [None, Some(repository_root.as_path())] {
        let (server, address) = start_installed(&backed_up_binary, &restored, selected_root);
        assert_eq!(
            post(address, "/auth/session", Some(&old_session), "").status,
            401
        );
        let session = sign_in(address);
        assert_eq!(
            post(address, "/auth/sign-out", Some(&old_session), "").status,
            401
        );
        assert_eq!(
            post(address, "/auth/session", Some(&session), "").status,
            204
        );
        let page = request(
            address,
            &format!(
                "GET /projects HTTP/1.1\r\nHost: {address}\r\nCookie: {}\r\nConnection: close\r\n\r\n",
                session
                    .header("set-cookie")
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap()
            ),
        );
        assert!(page.starts_with("HTTP/1.1 200"));
        let mut details = String::new();
        for project in expected.projects() {
            let id = project
                .id()
                .to_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert!(page.contains(&format!("href=\"/projects/{id}\"")));
            details.push_str(&request(address, &format!("GET /projects/{id}/code HTTP/1.1\r\nHost: {address}\r\nCookie: {}\r\nConnection: close\r\n\r\n", session.header("set-cookie").unwrap().split(';').next().unwrap())));
        }
        for path in ["main", "linked", "other"] {
            assert!(details.contains(&format!("Default checkout: <code>{path}</code>")));
        }
        assert_eq!(read_catalog(&restored).unwrap().unwrap(), expected);
        drop(server);
    }
    for path in [&data, &backup, &restored] {
        assert_eq!(fs::read(path.join("owner.bin")).unwrap(), owner_bytes);
        assert_eq!(fs::read(path.join("projects.bin")).unwrap(), catalog_bytes);
        assert_eq!(fs::metadata(path).unwrap().mode() & 0o777, 0o700);
        for record in ["owner.bin", "projects.bin"] {
            assert_eq!(
                fs::metadata(path.join(record)).unwrap().mode() & 0o777,
                0o600
            );
        }
    }
    assert!(data.join(".projects.lock").exists());
    assert_eq!(
        fs::read(data.join(".projects-interrupted.tmp")).unwrap(),
        b"unpublished fixture"
    );
    let (_changed, changed_root) = repositories::root_fixture();
    startup_rejected(
        &backed_up_binary,
        &restored,
        Some(&changed_root),
        "project catalog unavailable:",
    );
    assert_eq!(
        fs::read(restored.join("projects.bin")).unwrap(),
        catalog_bytes
    );
    fs::write(restored.join("projects.bin"), b"damaged catalog fixture").unwrap();
    startup_rejected(
        &backed_up_binary,
        &restored,
        None,
        "project catalog unavailable:",
    );
    assert_eq!(
        fs::read(restored.join("projects.bin")).unwrap(),
        b"damaged catalog fixture"
    );
    assert_eq!(
        fs::read(backup.join("projects.bin")).unwrap(),
        catalog_bytes
    );
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), catalog_bytes);
}

#[test]
fn documented_restore_rejects_incomplete_or_contradictory_catalog_snapshots() {
    let (_owner, data) = configured();
    for failure in [
        "missing-marker",
        "invalid-marker",
        "missing-catalog",
        "unexpected-catalog",
        "symlink-catalog",
    ] {
        let (_backup, backup) = data_directory::fixture();
        let (_restore, restored) = data_directory::fixture();
        assert!(guide_copy("data-backup", &data, &backup).status.success());
        match failure {
            "missing-marker" => fs::remove_file(backup.join("catalog-state")).unwrap(),
            "invalid-marker" => fs::write(backup.join("catalog-state"), b"unknown\n").unwrap(),
            "missing-catalog" => fs::write(backup.join("catalog-state"), b"present\n").unwrap(),
            "unexpected-catalog" => {
                fs::write(backup.join("projects.bin"), b"unexpected fixture").unwrap()
            }
            _ => {
                fs::write(backup.join("catalog-state"), b"present\n").unwrap();
                symlink(backup.join("owner.bin"), backup.join("projects.bin")).unwrap();
            }
        }
        assert!(
            !guide_copy("data-restore", &backup, &restored)
                .status
                .success(),
            "{failure}"
        );
        assert_eq!(fs::read_dir(&restored).unwrap().count(), 0, "{failure}");
    }
}
