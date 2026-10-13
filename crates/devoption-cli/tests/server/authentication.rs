use super::*;
use devoption_server::{
    identity::IdentityId,
    owner_store::OwnerStore,
    password::{SALT_BYTES, hash_password},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

const PASSWORD: &str = "existing owner fixture only";

#[path = "authentication/installation.rs"]
mod installation;
#[path = "authentication/pages.rs"]
mod pages;
#[path = "authentication/projects.rs"]
mod projects;
#[path = "authentication/repositories.rs"]
mod repositories;

fn configured() -> (tempfile::TempDir, PathBuf) {
    let (directory, path) = data_directory::fixture();
    let store = OwnerStore::open_absolute(&path).unwrap();
    let hash = hash_password(PASSWORD, &[0xa5; SALT_BYTES]).unwrap();
    store
        .create(IdentityId::from_bytes([7; 16]), &hash)
        .unwrap();
    (directory, path)
}

fn start_with(path: &Path) -> (Server, SocketAddr) {
    start_command(
        Command::new(env!("CARGO_BIN_EXE_devoption"))
            .args(["serve", "--port", "0", "--data-dir"])
            .arg(path),
    )
}

#[derive(Clone)]
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

fn post(address: SocketAddr, path: &str, issued: Option<&Reply>, body: &str) -> Reply {
    let credentials = issued
        .map(|reply| {
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
        })
        .unwrap_or_default();
    let response = request(
        address,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\n{credentials}Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    );
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(body.is_empty());
    let reply = Reply {
        status: headers
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap(),
        headers: headers
            .lines()
            .skip(1)
            .map(|line| {
                let (key, value) = line.split_once(':').unwrap();
                (key.to_ascii_lowercase(), value.trim().to_owned())
            })
            .collect(),
    };
    if matches!(path, "/auth/challenge" | "/auth/sign-in" | "/auth/sign-out") {
        assert_eq!(reply.header("cache-control"), Some("no-store"));
    }
    if reply.status != 204 {
        assert!(reply.header("set-cookie").is_none());
        assert!(reply.header("x-csrf-token").is_none());
    }
    reply
}

fn challenge(address: SocketAddr) -> Reply {
    let response = post(address, "/auth/challenge", None, "");
    assert_eq!(response.status, 204);
    response
}

fn sign_in(address: SocketAddr) -> Reply {
    let preauth = challenge(address);
    let response = post(
        address,
        "/auth/sign-in",
        Some(&preauth),
        &format!("password={PASSWORD}"),
    );
    assert_eq!(response.status, 204);
    assert_ne!(
        response.header("x-csrf-token"),
        preauth.header("x-csrf-token")
    );
    assert_eq!(
        post(
            address,
            "/auth/sign-in",
            Some(&preauth),
            &format!("password={PASSWORD}")
        )
        .status,
        403
    );
    response
}

#[test]
fn no_configuration_or_owner_never_opens_sign_in_or_first_visitor_enrollment() {
    let (_directory, empty) = data_directory::fixture();
    for configured in [false, true] {
        let (_server, address) = if configured {
            start_with(&empty)
        } else {
            start()
        };
        for path in [
            "/auth/session",
            "/auth/challenge",
            "/auth/sign-in",
            "/auth/sign-out",
            "/auth/setup",
        ] {
            assert_eq!(
                post(address, path, None, &format!("password={PASSWORD}")).status,
                401
            );
        }
        assert_eq!(fs::read_dir(&empty).unwrap().count(), 0);
    }
}

#[test]
fn recovery_requires_same_origin_cookie_and_tracks_rotation_logout_and_restart() {
    let (_directory, data) = configured();
    let (server, address) = start_with(&data);
    let first = sign_in(address);
    let cookie = first
        .header("set-cookie")
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let recover = |method: &str, host: &str, origin: &str, credentials: &str| {
        request(
            address,
            &format!(
                "{method} /auth/session HTTP/1.1\r\nHost: {host}\r\n{origin}{credentials}Content-Length: 0\r\nConnection: close\r\n\r\n"
            ),
        )
    };
    let host = address.to_string();
    let origin = format!("Origin: http://{address}\r\n");
    let valid = format!("Cookie: {cookie}\r\n");
    for _ in 0..2 {
        let response = recover("POST", &host, &origin, &valid);
        assert!(response.starts_with("HTTP/1.1 204"));
        assert!(response.contains(&format!(
            "x-csrf-token: {}",
            first.header("x-csrf-token").unwrap()
        )));
        assert!(response.contains("cache-control: no-store"));
        assert!(!response.contains("set-cookie:"));
        assert!(!response.contains("access-control-allow-origin:"));
        assert!(response.ends_with("\r\n\r\n"));
    }
    for (method, host, origin, credentials, status) in [
        ("POST", &host[..], "", valid.clone(), 403),
        ("POST", &host, "Origin: null\r\n", valid.clone(), 403),
        (
            "POST",
            &host,
            "Origin: https://attacker.invalid\r\n",
            valid.clone(),
            403,
        ),
        ("POST", "attacker.invalid", &origin, valid.clone(), 400),
        ("GET", &host, &origin, valid.clone(), 405),
        ("POST", &host, &origin, String::new(), 401),
        ("POST", &host, &origin, format!("{valid}{valid}"), 401),
        (
            "POST",
            &host,
            &origin,
            format!("{valid}Authorization: Bearer {}\r\n", "ab".repeat(32)),
            401,
        ),
    ] {
        let response = recover(method, host, origin, &credentials);
        assert!(response.starts_with(&format!("HTTP/1.1 {status}")));
        assert!(!response.contains("x-csrf-token:"));
        assert!(!response.contains("set-cookie:"));
    }
    let second = sign_in(address);
    assert!(recover("POST", &host, &origin, &valid).starts_with("HTTP/1.1 401"));
    assert_eq!(
        post(address, "/auth/sign-out", Some(&first), "").status,
        401
    );
    assert_eq!(
        post(address, "/auth/session", Some(&second), "").status,
        204
    );
    assert_eq!(
        post(address, "/auth/sign-out", Some(&second), "").status,
        204
    );
    assert_eq!(
        post(address, "/auth/session", Some(&second), "").status,
        401
    );
    let third = sign_in(address);
    drop(server);
    let (_server, address) = start_with(&data);
    assert_eq!(post(address, "/auth/session", Some(&third), "").status, 401);
}

#[test]
fn sign_in_page_distinguishes_cli_setup_and_keeps_inputs_disabled_without_script() {
    let (_directory, data) = configured();
    let (_server, address) = start_with(&data);
    let page = request(
        address,
        &format!("GET /sign-in HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"),
    );
    assert!(page.starts_with("HTTP/1.1 200"));
    assert!(page.contains("cache-control: no-store"));
    assert!(page.contains("data-owner-configured=\"true\""));
    assert!(page.contains("<label for=\"auth-password\">Password</label>"));
    assert!(page.contains("autocomplete=\"current-password\""));
    assert!(page.contains("aria-describedby=\"auth-status\" disabled"));
    let body = page.split_once("</head>").unwrap().1;
    assert!(body.contains("<body class=\"auth-page\""));
    assert!(body.contains("role=\"img\" aria-label=\"DevOption\""));
    assert!(body.contains("aria-labelledby=\"page-title\""));
    assert!(body.contains("action=\"/auth/sign-in\" method=\"post\""));
    assert_eq!(body.matches("<form ").count(), 1);
    assert!(!body.contains("<nav"));
    assert!(!body.contains("profile-popup"));
    assert!(!body.contains("<header class=\"content-header\""));
    let (_server, address) = start();
    let page = request(
        address,
        &format!("GET /sign-in HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"),
    );
    assert!(page.contains("Owner setup is required"));
    assert!(!page.contains("<form id=\"signin-form\""));
    assert!(page.contains("data-owner-configured=\"false\""));
    let body = page.split_once("</head>").unwrap().1;
    assert!(body.contains("Set up your workspace"));
    assert!(body.contains("devoption setup --help"));
    assert!(!body.contains("<form "));
    assert!(!body.contains("<input "));
    assert!(!body.contains("<nav"));
}

#[test]
fn real_sign_in_rotation_and_logout_share_the_production_session_guard() {
    let (_directory, data) = configured();
    let original = fs::read(data.join("owner.bin")).unwrap();
    let (_server, address) = start_with(&data);
    let initial = challenge(address);
    assert_eq!(
        post(
            address,
            "/auth/sign-in",
            Some(&initial),
            "password=wrong fixture"
        )
        .status,
        401
    );
    assert_eq!(
        post(
            address,
            "/auth/sign-in",
            Some(&initial),
            &format!("password={PASSWORD}")
        )
        .status,
        403
    );
    let first = sign_in(address);
    assert!(
        first
            .header("set-cookie")
            .unwrap()
            .ends_with("Path=/; Secure; HttpOnly; SameSite=Strict")
    );
    assert_eq!(post(address, "/projects", Some(&first), "").status, 415);
    assert_eq!(post(address, "/auth/sign-out", None, "").status, 401);
    let mut wrong = first.clone();
    wrong
        .headers
        .iter_mut()
        .find(|(key, _)| key == "x-csrf-token")
        .unwrap()
        .1 = "00".repeat(32);
    assert_eq!(
        post(address, "/auth/sign-out", Some(&wrong), "").status,
        401
    );
    let second = sign_in(address);
    assert_ne!(first.header("set-cookie"), second.header("set-cookie"));
    assert_eq!(post(address, "/projects", Some(&first), "").status, 401);
    assert_eq!(
        post(address, "/auth/sign-out", Some(&first), "").status,
        401
    );
    assert_eq!(post(address, "/projects", Some(&second), "").status, 415);
    // Enrollment remains absent even after a valid sign-in.
    assert_eq!(post(address, "/auth/setup", Some(&second), "").status, 404);
    let (_other_server, other_address) = start_with(&data);
    assert_eq!(
        post(other_address, "/auth/sign-out", Some(&second), "").status,
        401
    );
    let logout = post(address, "/auth/sign-out", Some(&second), "");
    assert_eq!(logout.status, 204);
    assert_eq!(
        logout.header("set-cookie"),
        Some("__Host-devoption_session=; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=0")
    );
    assert_eq!(post(address, "/projects", Some(&second), "").status, 401);
    assert_eq!(
        post(address, "/auth/sign-out", Some(&second), "").status,
        401
    );
    assert_eq!(fs::read(data.join("owner.bin")).unwrap(), original);
    assert_eq!(fs::read_dir(&data).unwrap().count(), 1);
}

#[test]
fn issued_browser_tokens_cannot_authorize_bearer_or_mixed_http_requests() {
    let (_directory, data) = configured();
    let (_server, address) = start_with(&data);
    let issued = sign_in(address);
    let cookie = issued
        .header("set-cookie")
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let token = cookie.split_once('=').unwrap().1;
    let csrf = issued.header("x-csrf-token").unwrap();
    let pair = format!("Cookie: {cookie}\r\nX-CSRF-Token: {csrf}\r\n");
    let bearer = format!("Authorization: Bearer {token}\r\n");
    for path in ["/projects", "/missing", "/auth/sign-out", "/auth/session"] {
        for credentials in [
            bearer.clone(),
            format!("{pair}{bearer}"),
            format!("{bearer}{bearer}"),
            format!("{pair}Authorization:\r\n"),
            format!("{pair}Cookie: {cookie}\r\n"),
            format!("Cookie: {cookie}; {cookie}\r\nX-CSRF-Token: {csrf}\r\n"),
        ] {
            let response = request(
                address,
                &format!(
                    "POST {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\n{credentials}Content-Length: 0\r\nConnection: close\r\n\r\n"
                ),
            );
            assert!(response.starts_with("HTTP/1.1 401"), "{path}");
            assert!(response.contains("cache-control: no-store"));
            assert!(response.ends_with("\r\n\r\n"));
            for forbidden in [
                "set-cookie:",
                "x-csrf-token:",
                "www-authenticate:",
                token,
                csrf,
            ] {
                assert!(!response.contains(forbidden));
            }
        }
    }
    // Rejected requests cannot clear or replace the valid browser session.
    assert_eq!(post(address, "/projects", Some(&issued), "").status, 415);
    assert_eq!(
        post(address, "/auth/session", Some(&issued), "").status,
        204
    );
    assert_eq!(
        post(address, "/auth/sign-out", Some(&issued), "").status,
        204
    );
    assert_eq!(post(address, "/projects", Some(&issued), "").status, 401);
}

#[test]
fn repeated_password_attempts_share_the_http_admission_limit() {
    let (_directory, data) = configured();
    let (_server, address) = start_with(&data);
    for _ in 0..5 {
        let preauth = challenge(address);
        assert_eq!(
            post(
                address,
                "/auth/sign-in",
                Some(&preauth),
                "password=wrong fixture"
            )
            .status,
            401
        );
    }
    let preauth = challenge(address);
    let limited = post(
        address,
        "/auth/sign-in",
        Some(&preauth),
        &format!("password={PASSWORD}"),
    );
    assert_eq!(limited.status, 429);
    assert!(
        (1..=60).contains(
            &limited
                .header("retry-after")
                .unwrap()
                .parse::<u64>()
                .unwrap()
        )
    );
    assert_eq!(
        post(
            address,
            "/auth/sign-in",
            Some(&preauth),
            &format!("password={PASSWORD}")
        )
        .status,
        403
    );
}

#[test]
fn owner_store_failure_or_disappearance_after_startup_never_creates_an_account() {
    let (_directory, data) = configured();
    let (_server, address) = start_with(&data);
    let record = data.join("owner.bin");
    fs::write(&record, b"invalid private fixture record").unwrap();
    let preauth = challenge(address);
    assert_eq!(
        post(
            address,
            "/auth/sign-in",
            Some(&preauth),
            &format!("password={PASSWORD}")
        )
        .status,
        503
    );
    assert_eq!(
        fs::read(&record).unwrap(),
        b"invalid private fixture record"
    );
    fs::remove_file(&record).unwrap();
    let preauth = challenge(address);
    assert_eq!(
        post(
            address,
            "/auth/sign-in",
            Some(&preauth),
            &format!("password={PASSWORD}")
        )
        .status,
        401
    );
    assert_eq!(fs::read_dir(&data).unwrap().count(), 0);
}
