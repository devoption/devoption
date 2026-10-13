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
    assert!(response.contains("aria-label=\"Project sections\""));
    assert!(!response.contains("Build execution is not available yet."));
    assert!(!response.contains("<form class=\"project-form repository-form\""));
    assert!(!response.contains("<p class=\"repository-notice\">Repository attachment"));
    assert!(page(address, "GET", Some(&issued)).contains(&format!("href=\"{}\"", path(first))));
    let empty = page_at(address, "GET", &path(second), Some(&issued));
    assert!(empty.contains("No repositories yet."));
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
    assert!(!refreshed.contains("No repositories yet."));
    fs::write(data.join("projects.bin"), b"invalid private fixture").unwrap();
    let failed = page_at(address, "GET", &path(first), Some(&issued));
    assert!(failed.starts_with("HTTP/1.1 503"));
    assert!(failed.contains("cache-control: no-store"));
    assert!(!failed.contains("invalid private fixture"));
    assert!(!failed.contains("First fixture"));
}

#[test]
fn project_sections_are_deep_linkable_authenticated_and_read_only() {
    let (_temporary, data) = configured();
    let mut catalog = seed(&data);
    catalog.create_project("Unrelated project").unwrap();
    CatalogStore::open_absolute(&data)
        .unwrap()
        .save(&catalog)
        .unwrap();
    let id: String = catalog.projects()[0]
        .id()
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let base = format!("/projects/{id}");
    let sections = [
        ("overview", "Overview"),
        ("code", "Code"),
        ("builds", "Builds"),
        ("releases", "Releases"),
        ("packages", "Packages"),
        ("settings", "Settings"),
    ];
    let (_server, address) = start_with(&data);
    for (segment, _) in sections {
        for method in ["GET", "HEAD"] {
            let denied = page_at(address, method, &format!("{base}/{segment}"), None);
            assert!(denied.starts_with("HTTP/1.1 303"));
            assert!(denied.contains("cache-control: no-store"));
            assert!(denied.split_once("\r\n\r\n").unwrap().1.is_empty());
        }
    }
    let issued = sign_in(address);
    let original = fs::read(data.join("projects.bin")).unwrap();
    for (segment, label) in sections {
        let path = format!("{base}/{segment}");
        let html = page_at(address, "GET", &path, Some(&issued));
        assert!(html.starts_with("HTTP/1.1 200"));
        assert!(html.contains("cache-control: no-store"));
        assert!(html.contains(ESCAPED));
        assert!(!html.contains("Unrelated project"));
        let navigation = html
            .split_once("aria-label=\"Project sections\">")
            .unwrap()
            .1
            .split_once("</nav>")
            .unwrap()
            .0;
        assert_eq!(navigation.matches("aria-current=\"page\"").count(), 1);
        assert_eq!(navigation.matches("<a ").count(), 6);
        let active = if segment == "overview" {
            base.clone()
        } else {
            path.clone()
        };
        assert!(navigation.contains(&format!(
            "href=\"{active}\" aria-current=\"page\">{label}</a>"
        )));
        assert_eq!(
            html.contains("<form id=\"project-form\""),
            segment == "settings"
        );
        assert_eq!(
            html.contains("Builds are not available yet"),
            segment == "builds"
        );
        assert_eq!(
            html.contains("First fixture"),
            segment == "overview" || segment == "code"
        );
        let head = page_at(address, "HEAD", &path, Some(&issued));
        assert!(head.starts_with("HTTP/1.1 200"));
        assert!(head.split_once("\r\n\r\n").unwrap().1.is_empty());
    }
    for path in [
        format!("{base}/unknown"),
        format!("{base}/CODE"),
        format!("{base}/%FF"),
        "/projects/missing/code".into(),
        format!("/projects/{}/settings", "00".repeat(16)),
    ] {
        let missing = page_at(address, "GET", &path, Some(&issued));
        assert!(missing.starts_with("HTTP/1.1 404"), "{path}: {missing}");
        assert!(missing.contains("cache-control: no-store"));
        assert!(!missing.contains(ESCAPED));
    }
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), original);
    fs::write(data.join("projects.bin"), b"invalid fixture").unwrap();
    for (section, _) in sections {
        let response = page_at(address, "GET", &format!("{base}/{section}"), Some(&issued));
        assert!(response.starts_with("HTTP/1.1 503"));
        assert!(!response.contains(ESCAPED));
    }
}
