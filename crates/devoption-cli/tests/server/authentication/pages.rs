use super::*;

const PATHS: [&str; 3] = ["/", "/projects", "/settings/appearance"];

fn read(address: SocketAddr, method: &str, path: &str, credentials: &str) -> String {
    request(
        address,
        &format!(
            "{method} {path} HTTP/1.1\r\nHost: {address}\r\n{credentials}Connection: close\r\n\r\n"
        ),
    )
}

fn cookie(issued: &Reply) -> String {
    format!(
        "Cookie: {}\r\n",
        issued
            .header("set-cookie")
            .unwrap()
            .split(';')
            .next()
            .unwrap()
    )
}

fn assert_pages(address: SocketAddr, credentials: &str, allowed: bool) {
    for method in ["GET", "HEAD"] {
        for path in PATHS {
            let response = read(address, method, path, credentials);
            let (headers, body) = response.split_once("\r\n\r\n").unwrap();
            assert!(headers.starts_with(if allowed {
                "HTTP/1.1 200"
            } else {
                "HTTP/1.1 303"
            }));
            assert!(headers.contains("cache-control: no-store\r\n"));
            assert!(!headers.contains("set-cookie:"));
            assert!(!headers.contains("x-csrf-token:"));
            if allowed {
                assert!(!headers.contains("location:"));
                if method == "GET" {
                    assert!(body.contains("<main id=\"main-content\""));
                } else {
                    assert!(body.is_empty());
                }
            } else {
                assert!(headers.contains("location: /sign-in\r\n"));
                assert!(body.is_empty());
            }
        }
    }
}

#[test]
fn configured_page_access_tracks_real_sign_in_logout_and_restart() {
    let (_directory, data) = configured();
    let original = fs::read(data.join("owner.bin")).unwrap();
    let (server, address) = start_with(&data);
    assert_pages(address, "", false);
    for method in ["GET", "HEAD"] {
        let response = read(address, method, "/sign-in", "");
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(!response.contains("\r\nlocation:"));
    }
    let first = sign_in(address);
    assert_pages(address, &cookie(&first), true); // Normal navigation has no CSRF header.
    let query = read(
        address,
        "GET",
        "/projects?preview=true&next=https://attacker.invalid/",
        "",
    );
    assert!(query.starts_with("HTTP/1.1 303"));
    assert!(!query.contains("attacker.invalid"));
    for credentials in [
        format!("{}{}", cookie(&first), cookie(&first)),
        format!(
            "{}Authorization: Bearer {}\r\n",
            cookie(&first),
            "ab".repeat(32)
        ),
    ] {
        assert_pages(address, &credentials, false);
    }
    assert_eq!(
        post(address, "/auth/sign-out", Some(&first), "").status,
        204
    );
    assert_pages(address, &cookie(&first), false);
    let second = sign_in(address);
    assert_pages(address, &cookie(&second), true);
    drop(server);
    let (_server, address) = start_with(&data);
    assert_pages(address, &cookie(&second), false);
    assert_eq!(fs::read(data.join("owner.bin")).unwrap(), original);

    // A configured process must not fall back to the public preview if storage
    // later loses its owner. Changing request data cannot select public mode either.
    fs::remove_file(data.join("owner.bin")).unwrap();
    assert_pages(address, "", false);
    assert!(
        read(address, "GET", "/sign-in?configured=false", "")
            .contains("data-owner-configured=\"true\"")
    );
}

#[test]
fn absent_configuration_or_owner_keeps_only_the_intended_public_preview() {
    let (_directory, empty) = data_directory::fixture();
    for with_directory in [false, true] {
        let (_server, address) = if with_directory {
            start_with(&empty)
        } else {
            start()
        };
        assert_pages(address, "", true);
        let signin = read(address, "GET", "/sign-in", "");
        assert!(signin.contains("Owner setup is required"));
        assert!(signin.contains("data-owner-configured=\"false\""));
        assert_eq!(post(address, "/auth/session", None, "").status, 401);
        assert_eq!(fs::read_dir(&empty).unwrap().count(), 0);
    }
}

#[test]
fn malformed_or_unknown_project_repository_ids_preserve_http_access_boundaries() {
    let (_directory, data) = configured();
    let (_root, root) = data_directory::fixture();
    let (_server, address) = start_command(
        Command::new(env!("CARGO_BIN_EXE_devoption"))
            .args(["serve", "--port", "0", "--data-dir"])
            .arg(&data)
            .arg("--repository-root")
            .arg(&root),
    );
    let issued = sign_in(address);
    for id in [
        "bad".to_owned(),
        "+0".repeat(16),
        "0".repeat(33),
        "%C3%A9".repeat(16),
        "00".repeat(16),
        "aF".repeat(16),
    ] {
        let valid = "00".repeat(16);
        for path in [
            format!("/projects/{id}/code"),
            format!("/projects/{id}/repositories/{valid}/status"),
            format!("/projects/{valid}/repositories/{id}/status"),
            format!("/projects/{valid}/repositories/{id}/files"),
            format!("/projects/{valid}/builds/{id}"),
            format!("/projects/{valid}/builds/{id}/run"),
        ] {
            let anonymous = read(address, "GET", &path, "");
            assert!(anonymous.starts_with("HTTP/1.1 303"), "{path}: {anonymous}");
            let authenticated = read(address, "GET", &path, &cookie(&issued));
            assert!(
                authenticated.starts_with("HTTP/1.1 404"),
                "{path}: {authenticated}"
            );
        }
    }
}
