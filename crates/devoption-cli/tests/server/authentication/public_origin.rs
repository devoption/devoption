use super::*;

const HOST: &str = "platform.example.test";
const ORIGIN: &str = "https://platform.example.test";

fn start_public(data: &Path) -> (Server, SocketAddr) {
    start_command(
        Command::new(env!("CARGO_BIN_EXE_devoption"))
            .args([
                "serve",
                "--port",
                "0",
                "--public-origin",
                ORIGIN,
                "--data-dir",
            ])
            .arg(data),
    )
}

fn credentials(reply: &Reply) -> String {
    format!(
        "Cookie: {}\r\nX-CSRF-Token: {}\r\n",
        reply
            .header("set-cookie")
            .unwrap()
            .split(';')
            .next()
            .unwrap(),
        reply.header("x-csrf-token").unwrap()
    )
}

fn send(address: SocketAddr, method: &str, path: &str, headers: &str, body: &str) -> String {
    let content_type = if path.starts_with("/auth/") {
        "application/x-www-form-urlencoded"
    } else {
        "text/plain"
    };
    request(
        address,
        &format!(
            "{method} {path} HTTP/1.1\r\nHost: {HOST}\r\n{headers}Content-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn reply(response: &str) -> Reply {
    let head = response.split_once("\r\n\r\n").unwrap().0;
    Reply {
        status: head
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap(),
        headers: head
            .lines()
            .skip(1)
            .map(|line| {
                let (k, v) = line.split_once(':').unwrap();
                (k.to_ascii_lowercase(), v.trim().to_owned())
            })
            .collect(),
    }
}

fn login(address: SocketAddr) -> Reply {
    let origin = format!("Origin: {ORIGIN}\r\n");
    let challenge = reply(&send(address, "POST", "/auth/challenge", &origin, ""));
    assert_eq!(challenge.status, 204);
    let logged_in = reply(&send(
        address,
        "POST",
        "/auth/sign-in",
        &format!("{origin}{}", credentials(&challenge)),
        &format!("password={PASSWORD}"),
    ));
    assert_eq!(logged_in.status, 204);
    let cookie = logged_in.header("set-cookie").unwrap();
    assert!(cookie.starts_with("__Host-devoption_session="));
    assert!(cookie.contains("Path=/; Secure; HttpOnly; SameSite=Strict"));
    assert!(!cookie.to_ascii_lowercase().contains("domain="));
    logged_in
}

#[test]
fn public_login_project_mutation_logout_and_restart_preserve_state() {
    let (_directory, data) = configured();
    let original_owner = fs::read(data.join("owner.bin")).unwrap();
    let (server, address) = start_public(&data);
    assert_eq!(
        reply(&send(address, "GET", "/projects", "", "")).status,
        303
    );
    assert_eq!(reply(&send(address, "GET", "/sign-in", "", "")).status, 200);
    assert_eq!(
        reply(&send(
            address,
            "POST",
            "/projects",
            &format!("Origin: {ORIGIN}\r\n"),
            "Public fixture"
        ))
        .status,
        401
    );
    let session = login(address);
    let auth = credentials(&session);
    for origin in [
        "",
        "Origin: null\r\n",
        "Origin: http://platform.example.test\r\n",
        "Origin: https://app.example.test\r\n",
    ] {
        assert_eq!(
            reply(&send(
                address,
                "POST",
                "/projects",
                &format!("{origin}{auth}"),
                "Rejected"
            ))
            .status,
            403
        );
    }
    assert_eq!(
        reply(&send(
            address,
            "POST",
            "/projects",
            &format!(
                "Origin: {ORIGIN}\r\nCookie: {}\r\n",
                session
                    .header("set-cookie")
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap()
            ),
            "Rejected"
        ))
        .status,
        401
    );
    let created = reply(&send(
        address,
        "POST",
        "/projects",
        &format!("Origin: {ORIGIN}\r\n{auth}"),
        "Public fixture",
    ));
    assert_eq!(created.status, 204);
    assert!(
        created
            .header("location")
            .unwrap()
            .starts_with("/projects/")
    );
    assert!(send(address, "GET", "/projects", &auth, "").contains("Public fixture"));
    let catalog = fs::read(data.join("projects.bin")).unwrap();
    assert_eq!(
        reply(&send(
            address,
            "POST",
            "/auth/session",
            &format!("Origin: {ORIGIN}\r\n{auth}"),
            ""
        ))
        .status,
        204
    );
    assert_eq!(
        reply(&send(
            address,
            "POST",
            "/auth/setup",
            &format!("Origin: {ORIGIN}\r\n{auth}"),
            ""
        ))
        .status,
        404
    );
    drop(server);
    let (_server, address) = start_public(&data);
    assert_eq!(
        reply(&send(address, "GET", "/projects", &auth, "")).status,
        303
    );
    let session = login(address);
    let auth = credentials(&session);
    assert!(send(address, "GET", "/projects", &auth, "").contains("Public fixture"));
    assert_eq!(fs::read(data.join("projects.bin")).unwrap(), catalog);
    assert_eq!(fs::read(data.join("owner.bin")).unwrap(), original_owner);
    assert_eq!(
        reply(&send(
            address,
            "POST",
            "/auth/sign-out",
            &format!("Origin: {ORIGIN}\r\n{auth}"),
            ""
        ))
        .status,
        204
    );
    assert_eq!(
        reply(&send(address, "GET", "/projects", &auth, "")).status,
        303
    );
}

#[test]
fn public_routes_reject_forwarded_and_conflicting_authorities() {
    let (_directory, data) = configured();
    let (_server, address) = start_public(&data);
    for path in ["/sign-in", "/projects", "/auth/challenge", "/absent"] {
        for field in [
            "Forwarded: proto=https;host=platform.example.test",
            "X-Forwarded-Host: platform.example.test",
            "X-Forwarded-Proto: https",
            "X-Forwarded-For: 127.0.0.1",
            "X-Real-IP: 127.0.0.1",
        ] {
            let response = reply(&send(
                address,
                "POST",
                path,
                &format!("Origin: {ORIGIN}\r\n{field}\r\n"),
                "",
            ));
            assert_eq!(response.status, 400);
            assert_eq!(response.header("cache-control"), Some("no-store"));
            assert!(response.header("set-cookie").is_none());
        }
    }
    for host in [
        address.to_string(),
        "app.example.test".into(),
        format!("{HOST}.evil.test"),
        format!("{HOST}:80"),
    ] {
        let response = request(
            address,
            &format!("GET /sign-in HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"),
        );
        assert_eq!(reply(&response).status, 400);
    }
}

#[test]
fn public_mode_never_serves_without_existing_owner() {
    let (_directory, data) = data_directory::fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_devoption"))
        .args(["serve", "--public-origin", ORIGIN, "--data-dir"])
        .arg(&data)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("public origin requires an existing configured owner")
    );
    assert_eq!(fs::read_dir(data).unwrap().count(), 0);
}

#[test]
#[ignore = "requires an explicitly supplied Caddy binary; local disposable TLS only"]
fn public_https_proxy() {
    let caddy = std::env::var_os("DEVOPTION_TEST_CADDY")
        .expect("set DEVOPTION_TEST_CADDY to a verified Caddy binary");
    let (_directory, data) = configured();
    let (_server, address) = start_public(&data);
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/check_public_proxy.py");
    let result = Command::new("python3")
        .arg(script)
        .arg(caddy)
        .arg(address.to_string())
        .status()
        .unwrap();
    assert!(result.success());
}
