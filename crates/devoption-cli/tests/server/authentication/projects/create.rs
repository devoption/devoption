use super::*;

pub(super) fn create_request(address: SocketAddr, issued: &Reply, name: &str) -> String {
    format!(
        "POST /projects HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nCookie: {}\r\nX-CSRF-Token: {}\r\nContent-Type: text/plain;charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{name}",
        issued
            .header("set-cookie")
            .unwrap()
            .split(';')
            .next()
            .unwrap(),
        issued.header("x-csrf-token").unwrap(),
        name.len(),
    )
}

pub(super) fn status(address: SocketAddr, raw: &str) -> u16 {
    let response = request(address, raw);
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(body.is_empty());
    assert!(headers.contains("cache-control: no-store"));
    assert!(!headers.contains("set-cookie:"));
    assert!(!headers.contains("x-csrf-token:"));
    if !headers.starts_with("HTTP/1.1 204") {
        assert!(!headers.contains("location:"));
    }
    headers.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
fn creation_locations_open_the_new_saved_project_even_when_names_repeat() {
    let (_temporary, data) = configured();
    let (_server, address) = start_with(&data);
    let issued = sign_in(address);
    let mut destinations = Vec::new();
    for _ in 0..2 {
        let response = request(address, &create_request(address, &issued, NAME));
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("HTTP/1.1 204"));
        assert!(headers.contains("cache-control: no-store"));
        assert!(body.is_empty());
        let destination = headers
            .lines()
            .find_map(|line| line.strip_prefix("location: "))
            .unwrap();
        let catalog = devoption_projects::read_catalog(&data).unwrap().unwrap();
        let newest = catalog.projects().last().unwrap();
        let id: String = newest
            .id()
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(destination, format!("/projects/{id}"));
        let details = page_at(address, "GET", destination, Some(&issued));
        assert!(details.starts_with("HTTP/1.1 200"));
        assert!(details.contains(ESCAPED));
        assert!(details.contains("No repositories attached yet."));
        destinations.push(destination.to_owned());
    }
    assert_ne!(destinations[0], destinations[1]);
}

#[test]
fn authenticated_creation_is_immediately_visible_escaped_and_persists_after_restart() {
    let (_directory, data) = configured();
    let owner = fs::read(data.join("owner.bin")).unwrap();
    let (server, address) = start_with(&data);
    let issued = sign_in(address);
    assert_eq!(
        status(address, &create_request(address, &issued, NAME)),
        204
    );
    let response = page(address, "GET", Some(&issued));
    assert!(response.contains(ESCAPED));
    assert!(response.contains("<p>0 repositories</p>"));
    assert!(!response.contains(NAME));
    let saved = devoption_projects::read_catalog(&data).unwrap().unwrap();
    assert_eq!(saved.projects().len(), 1);
    assert!(saved.repositories().is_empty());
    drop(server);
    let (_server, address) = start_with(&data);
    assert!(page(address, "GET", Some(&sign_in(address))).contains(ESCAPED));
    assert_eq!(
        devoption_projects::read_catalog(&data).unwrap().unwrap(),
        saved
    );
    assert_eq!(fs::read(data.join("owner.bin")).unwrap(), owner);
}

#[test]
fn creation_requires_cookie_csrf_same_origin_and_no_authorization() {
    let (_directory, data) = configured();
    let (_server, address) = start_with(&data);
    let issued = sign_in(address);
    let raw = create_request(address, &issued, "Denied fixture");
    for header in ["Cookie:", "X-CSRF-Token:"] {
        let removed = raw
            .split("\r\n")
            .filter(|line| !line.starts_with(header))
            .collect::<Vec<_>>()
            .join("\r\n");
        assert_eq!(status(address, &removed), 401);
    }
    for value in ["", "Bearer fixture", "Basic fixture"] {
        let mixed = raw.replacen(
            "Connection:",
            &format!("Authorization: {value}\r\nConnection:"),
            1,
        );
        assert_eq!(status(address, &mixed), 401);
    }
    let wrong = raw.replace(issued.header("x-csrf-token").unwrap(), &"00".repeat(32));
    assert_eq!(status(address, &wrong), 401);
    // The outer Origin guard returns no body and does not touch storage.
    for origin in ["", "Origin: https://attacker.invalid\r\n"] {
        let denied = request(
            address,
            &raw.replace(&format!("Origin: http://{address}\r\n"), origin),
        );
        assert!(denied.starts_with("HTTP/1.1 403"));
        assert!(denied.split_once("\r\n\r\n").unwrap().1.is_empty());
    }
    assert!(!data.join("projects.bin").exists());
    assert!(!data.join(".projects.lock").exists());
}

#[test]
fn invalid_names_media_and_capacity_do_not_change_the_catalog() {
    let (_directory, data) = configured();
    let mut catalog = seed(&data);
    let saved = fs::read(data.join("projects.bin")).unwrap();
    let (_server, address) = start_with(&data);
    let issued = sign_in(address);
    for name in [
        "".into(),
        " padded ".into(),
        "line\nbreak".into(),
        "\u{0085}".into(),
        "é".repeat(65),
    ] {
        assert_eq!(
            status(address, &create_request(address, &issued, &name)),
            if name.len() > 128 { 413 } else { 400 }
        );
    }
    let raw = create_request(address, &issued, "Valid fixture");
    assert_eq!(
        status(
            address,
            &raw.replace("text/plain;charset=UTF-8", "application/json")
        ),
        415
    );
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), saved);
    for i in catalog.projects().len()..devoption_projects::MAX_PROJECTS {
        catalog
            .create_project(&format!("Capacity fixture {i}"))
            .unwrap();
    }
    CatalogStore::open_absolute(&data)
        .unwrap()
        .save(&catalog)
        .unwrap();
    assert_eq!(status(address, &raw), 409);
    assert_eq!(
        devoption_projects::read_catalog(&data).unwrap().unwrap(),
        catalog
    );
}

#[test]
fn concurrent_requests_preserve_every_successful_creation_and_repository_association() {
    let (_directory, data) = configured();
    let initial = seed(&data);
    let (_server, address) = start_with(&data);
    let issued = sign_in(address);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    std::thread::scope(|scope| {
        for i in 0..8 {
            let issued = &issued;
            let barrier = barrier.clone();
            scope.spawn(move || {
                let name = format!("Concurrent fixture {i}");
                barrier.wait();
                assert_eq!(
                    status(address, &create_request(address, issued, &name)),
                    204
                );
                assert!(page(address, "GET", Some(issued)).contains(&name));
            });
        }
    });
    let saved = devoption_projects::read_catalog(&data).unwrap().unwrap();
    assert_eq!(saved.projects().len(), 9);
    assert_eq!(saved.repositories(), initial.repositories());
    assert_eq!(saved.projects()[0], initial.projects()[0]);
    for i in 0..8 {
        assert_eq!(
            saved
                .projects()
                .iter()
                .filter(|project| project.name() == format!("Concurrent fixture {i}"))
                .count(),
            1
        );
    }
}

#[test]
fn busy_corrupt_or_unwritable_storage_never_reports_success_or_resets_records() {
    let (_directory, data) = configured();
    seed(&data);
    let (_server, address) = start_with(&data);
    let issued = sign_in(address);
    let raw = create_request(address, &issued, "Must not be saved");
    let saved = fs::read(data.join("projects.bin")).unwrap();
    let writer = CatalogStore::open_absolute(&data).unwrap();
    assert_eq!(status(address, &raw), 503);
    drop(writer);
    fs::set_permissions(&data, fs::Permissions::from_mode(0o500)).unwrap();
    assert_eq!(status(address, &raw), 503);
    fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), saved);
    let bad = b"private invalid catalog fixture";
    fs::write(data.join("projects.bin"), bad).unwrap();
    assert_eq!(status(address, &raw), 503);
    let response = page(address, "GET", Some(&issued));
    assert!(response.starts_with("HTTP/1.1 503"));
    assert!(!response.contains(ESCAPED));
    assert!(!response.contains("private invalid catalog"));
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), bad);
}
