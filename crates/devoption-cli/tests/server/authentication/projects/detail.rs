use super::*;

#[test]
fn details_select_one_project_require_auth_and_read_current_catalog() {
    let (_temporary, data) = configured();
    let mut catalog = seed(&data);
    let first = catalog.projects()[0].id();
    let second = catalog.create_project("Other project only").unwrap();
    CatalogStore::open_absolute(&data)
        .unwrap()
        .save(&catalog)
        .unwrap();
    let path = |id: devoption_projects::ProjectId| {
        format!(
            "/projects/{}",
            id.to_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
    };
    let (_server, address) = start_with(&data);
    for method in ["GET", "HEAD"] {
        for target in [path(first), "/projects/missing".into()] {
            let denied = page_at(address, method, &target, None);
            assert!(denied.starts_with("HTTP/1.1 303"));
            assert!(denied.contains("location: /sign-in"));
            assert!(!denied.contains(ESCAPED));
        }
    }
    let issued = sign_in(address);
    let original = fs::read(data.join("projects.bin")).unwrap();
    let response = page_at(address, "GET", &path(first), Some(&issued));
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("cache-control: no-store"));
    assert!(response.contains(&format!("<title>{ESCAPED} · DevOption</title>")));
    assert!(response.contains("First fixture"));
    assert!(response.contains("Second fixture"));
    assert!(!response.contains("Other project only"));
    assert!(!response.contains(NAME));
    assert!(response.contains("<a href=\"/projects\">Projects</a>"));
    assert!(response.contains("Build execution is not available yet."));
    assert!(!response.contains("<form class=\"project-form repository-form\""));
    assert!(response.contains("Repository attachment is not configured"));
    assert!(page(address, "GET", Some(&issued)).contains(&format!("href=\"{}\"", path(first))));
    let empty = page_at(address, "GET", &path(second), Some(&issued));
    assert!(empty.contains("No repositories attached yet."));
    assert!(!empty.contains("First fixture"));
    for target in [
        path(first),
        "/projects/missing".into(),
        format!("/projects/{}", "00".repeat(16)),
    ] {
        let head = page_at(address, "HEAD", &target, Some(&issued));
        assert!(head.contains("cache-control: no-store"));
        assert!(head.split_once("\r\n\r\n").unwrap().1.is_empty());
        if target != path(first) {
            let missing = page_at(address, "GET", &target, Some(&issued));
            assert!(missing.starts_with("HTTP/1.1 404"));
            assert!(!missing.contains(ESCAPED));
        }
    }
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), original);
    let repository = catalog.create_repository("Fresh association").unwrap();
    catalog.associate(second, repository).unwrap();
    CatalogStore::open_absolute(&data)
        .unwrap()
        .save(&catalog)
        .unwrap();
    let refreshed = page_at(address, "GET", &path(second), Some(&issued));
    assert!(refreshed.contains("Fresh association"));
    assert!(!refreshed.contains("No repositories attached yet."));
    fs::write(data.join("projects.bin"), b"invalid private fixture").unwrap();
    let failed = page_at(address, "GET", &path(first), Some(&issued));
    assert!(failed.starts_with("HTTP/1.1 503"));
    assert!(failed.contains("cache-control: no-store"));
    assert!(!failed.contains("invalid private fixture"));
    assert!(!failed.contains("First fixture"));
}
