use super::*;

fn rename_request(address: SocketAddr, issued: &Reply, id: &str, name: &str) -> String {
    create::create_request(address, issued, name).replacen(
        "POST /projects ",
        &format!("POST /projects/{id}/name "),
        1,
    )
}

fn id(catalog: &Catalog) -> String {
    catalog.projects()[0]
        .id()
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn rename_preserves_identity_associations_fresh_records_and_restart_state() {
    let (_directory, data) = configured();
    let mut before = seed(&data);
    let project_id = id(&before);
    let owner = fs::read(data.join("owner.bin")).unwrap();
    let (server, address) = start_with(&data);
    let issued = sign_in(address);
    before.create_project("Added after server startup").unwrap();
    CatalogStore::open_absolute(&data)
        .unwrap()
        .save(&before)
        .unwrap();
    let renamed = "Renamed <project> & \"工具\"";
    assert_eq!(
        create::status(
            address,
            &rename_request(address, &issued, &project_id, renamed)
        ),
        204
    );
    let saved = devoption_projects::read_catalog(&data).unwrap().unwrap();
    assert_eq!(saved.projects()[0].id(), before.projects()[0].id());
    assert_eq!(
        saved.projects()[0].repositories(),
        before.projects()[0].repositories()
    );
    assert_eq!(saved.projects()[0].name(), renamed);
    assert_eq!(saved.repositories(), before.repositories());
    assert_eq!(&saved.projects()[1..], &before.projects()[1..]);
    let path = format!("/projects/{project_id}");
    for path in ["/projects", path.as_str()] {
        let html = page_at(address, "GET", path, Some(&issued));
        assert!(html.contains("Renamed &lt;project&gt; &amp; &quot;工具&quot;"));
        assert!(!html.contains(renamed));
    }
    drop(server);
    let (_server, address) = start_with(&data);
    assert!(
        page_at(address, "GET", &path, Some(&sign_in(address))).contains("Renamed &lt;project&gt;")
    );
    assert_eq!(
        devoption_projects::read_catalog(&data).unwrap().unwrap(),
        saved
    );
    assert_eq!(fs::read(data.join("owner.bin")).unwrap(), owner);
}

#[test]
fn rename_denies_invalid_authority_input_missing_projects_and_storage_without_writes() {
    let (_directory, data) = configured();
    let catalog = seed(&data);
    let project_id = id(&catalog);
    let saved = fs::read(data.join("projects.bin")).unwrap();
    let (_server, address) = start_with(&data);
    let issued = sign_in(address);
    let raw = rename_request(address, &issued, &project_id, "Retained fixture");
    for header in ["Cookie:", "X-CSRF-Token:"] {
        let denied = raw
            .split("\r\n")
            .filter(|line| !line.starts_with(header))
            .collect::<Vec<_>>()
            .join("\r\n");
        assert_eq!(create::status(address, &denied), 401);
    }
    assert_eq!(
        create::status(
            address,
            &raw.replace(issued.header("x-csrf-token").unwrap(), &"00".repeat(32))
        ),
        401
    );
    for authorization in ["", "Bearer fixture", "Basic fixture"] {
        let denied = raw.replace(
            "Connection:",
            &format!("Authorization: {authorization}\r\nConnection:"),
        );
        assert_eq!(create::status(address, &denied), 401);
    }
    for origin in ["", "Origin: https://attacker.invalid\r\n"] {
        let denied = request(
            address,
            &raw.replace(&format!("Origin: http://{address}\r\n"), origin),
        );
        assert!(denied.starts_with("HTTP/1.1 403"));
    }
    assert!(!data.join(".projects.lock").exists());
    for name in [
        "".into(),
        " padded ".into(),
        "line\nbreak".into(),
        "\u{0085}".into(),
        "é".repeat(65),
    ] {
        assert_eq!(
            create::status(
                address,
                &rename_request(address, &issued, &project_id, &name)
            ),
            if name.len() > 128 { 413 } else { 400 }
        );
    }
    assert_eq!(
        create::status(
            address,
            &raw.replace("text/plain;charset=UTF-8", "application/json")
        ),
        415
    );
    for missing in ["not-an-id", &"00".repeat(16)] {
        assert_eq!(
            create::status(
                address,
                &rename_request(address, &issued, missing, "Missing fixture")
            ),
            404
        );
    }
    let lock = CatalogStore::open_absolute(&data).unwrap();
    assert_eq!(create::status(address, &raw), 503);
    drop(lock);
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), saved);
    fs::write(data.join("projects.bin"), b"invalid fixture catalog").unwrap();
    assert_eq!(create::status(address, &raw), 503);
    assert_eq!(
        fs::read(data.join("projects.bin")).unwrap(),
        b"invalid fixture catalog"
    );
}
