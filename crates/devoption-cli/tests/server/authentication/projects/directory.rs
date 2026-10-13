use super::*;
use create::{create_request, status};

fn private_directory(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn storage_stays_selected(replace_ancestor: bool) {
    let (_temporary, base) = configured();
    let parent = base.join("parent");
    let data = parent.join("data");
    private_directory(&parent);
    private_directory(&data);
    fs::rename(base.join("owner.bin"), data.join("owner.bin")).unwrap();
    let catalog = seed(&data);
    let project = catalog.projects()[0].id();
    let (_repositories, root) = repositories::root_fixture();
    let (_server, address) = start_command(
        Command::new(env!("CARGO_BIN_EXE_devoption"))
            .args(["serve", "--port", "0", "--data-dir"])
            .arg(&data)
            .arg("--repository-root")
            .arg(&root),
    );
    let issued = sign_in(address);

    let retained = base.join("retained");
    let original = if replace_ancestor {
        fs::rename(&parent, &retained).unwrap();
        retained.join("data")
    } else {
        fs::rename(&data, &retained).unwrap();
        retained
    };
    private_directory(&parent);
    private_directory(&data);
    let mut replacement = Catalog::default();
    replacement.create_project("Replacement fixture").unwrap();
    CatalogStore::open_absolute(&data)
        .unwrap()
        .save(&replacement)
        .unwrap();
    let replacement_bytes = fs::read(data.join("projects.bin")).unwrap();

    // Exercise both authenticated operations before checking their results.
    let response = page(address, "GET", Some(&issued));
    let created = status(address, &create_request(address, &issued, "HTTP fixture"));
    let original_snapshot = devoption_projects::read_catalog(&original)
        .unwrap()
        .unwrap();
    assert_eq!(created, 204);
    assert!(response.starts_with("HTTP/1.1 200"));
    let selected_read = response.contains(ESCAPED);
    let selected_write = original_snapshot
        .projects()
        .iter()
        .any(|p| p.name() == "HTTP fixture");
    let replacement_unchanged = fs::read(data.join("projects.bin")).unwrap() == replacement_bytes;
    assert!(
        selected_read && selected_write && replacement_unchanged,
        "selected read: {selected_read}; selected write: {selected_write}; replacement unchanged: {replacement_unchanged}"
    );
    assert!(!response.contains("Replacement fixture"));

    // Authentication stays on the same original directory, too. No owner was
    // created in the replacement. A fresh sign-in invalidates the earlier session.
    let issued = sign_in(address);
    assert!(page(address, "HEAD", Some(&issued)).starts_with("HTTP/1.1 200"));
    let project: String = project
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let attachment = create_request(address, &issued, "main").replacen(
        "POST /projects HTTP",
        &format!("POST /projects/{project}/repositories HTTP"),
        1,
    );
    assert_eq!(status(address, &attachment), 204);
    assert_eq!(
        devoption_projects::read_catalog(&original)
            .unwrap()
            .unwrap()
            .repository_bindings()
            .len(),
        1
    );
    let mut writer = CatalogStore::open_absolute(&original).unwrap();
    writer.create_project("External fixture").unwrap();
    let blocked = create_request(address, &issued, "After lock fixture");
    assert_eq!(status(address, &blocked), 503);
    assert!(page(address, "GET", Some(&issued)).contains("External fixture"));
    drop(writer);
    assert_eq!(status(address, &blocked), 204);
    assert!(page(address, "GET", Some(&issued)).contains("After lock fixture"));

    // The retained directory is revalidated, not blindly trusted forever.
    fs::set_permissions(&original, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(page(address, "GET", Some(&issued)).starts_with("HTTP/1.1 503"));
    assert_eq!(status(address, &blocked), 503);
    fs::set_permissions(&original, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        fs::read(data.join("projects.bin")).unwrap(),
        replacement_bytes
    );
}

#[test]
fn selected_storage_survives_directory_replacement() {
    storage_stays_selected(false);
}

#[test]
fn selected_storage_survives_ancestor_replacement() {
    storage_stays_selected(true);
}
