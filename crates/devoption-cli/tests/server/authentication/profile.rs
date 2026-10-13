use super::*;
use std::os::unix::fs::PermissionsExt;

pub(super) fn read(
    address: SocketAddr,
    method: &str,
    path: &str,
    issued: Option<&Reply>,
) -> String {
    let credentials = issued
        .map(|reply| {
            format!(
                "Cookie: {}\r\n",
                reply
                    .header("set-cookie")
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap()
            )
        })
        .unwrap_or_default();
    request(
        address,
        &format!(
            "{method} {path} HTTP/1.1\r\nHost: {address}\r\n{credentials}Connection: close\r\n\r\n"
        ),
    )
}
pub(super) fn save_request(address: SocketAddr, issued: &Reply, name: &str) -> String {
    format!(
        "POST /account/profile HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nCookie: {}\r\nX-CSRF-Token: {}\r\nContent-Type: text/plain;charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{name}",
        issued
            .header("set-cookie")
            .unwrap()
            .split(';')
            .next()
            .unwrap(),
        issued.header("x-csrf-token").unwrap(),
        name.len()
    )
}
pub(super) fn check(response: &str, status: u16) -> &str {
    assert!(
        response.starts_with(&format!("HTTP/1.1 {status}")),
        "{response}"
    );
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(headers.contains("cache-control: no-store"));
    assert!(!headers.contains("set-cookie:"));
    assert!(!headers.contains("x-csrf-token:"));
    body
}
#[test]
fn profile_save_clear_and_restart_preserve_credentials_and_identity() {
    let (_directory, data) = configured();
    let owner = fs::read(data.join("owner.bin")).unwrap();
    let (server, address) = start_with(&data);
    for path in ["/settings/profile", "/account/profile"] {
        for method in ["GET", "HEAD"] {
            assert_eq!(check(&read(address, method, path, None), 303), "");
        }
    }
    let issued = sign_in(address);
    assert_eq!(
        check(
            &read(address, "GET", "/account/profile", Some(&issued)),
            200
        ),
        ""
    );
    assert!(!data.join("profile.bin").exists());
    let page = read(address, "GET", "/settings/profile", Some(&issued));
    assert!(check(&page, 200).contains("id=\"profile-form\""));
    assert!(page.contains("<label for=\"display-name\">Display name</label>"));
    let name = "Élodie 林 <script>&";
    assert_eq!(
        check(
            &request(address, &save_request(address, &issued, name)),
            204
        ),
        ""
    );
    assert_eq!(
        check(
            &read(address, "GET", "/account/profile", Some(&issued)),
            200
        ),
        name
    );
    assert_eq!(
        check(
            &read(address, "HEAD", "/account/profile", Some(&issued)),
            200
        ),
        ""
    );
    drop(server);
    let (_server, address) = start_with(&data);
    assert_eq!(
        check(
            &read(address, "GET", "/account/profile", Some(&issued)),
            303
        ),
        ""
    );
    let issued = sign_in(address);
    assert_eq!(
        check(
            &read(address, "GET", "/account/profile", Some(&issued)),
            200
        ),
        name
    );
    assert_eq!(
        check(&request(address, &save_request(address, &issued, "")), 204),
        ""
    );
    assert_eq!(
        check(
            &read(address, "GET", "/account/profile", Some(&issued)),
            200
        ),
        ""
    );
    assert_eq!(fs::read(data.join("owner.bin")).unwrap(), owner);
    assert_eq!(
        fs::metadata(data.join("profile.bin"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(!data.join("projects.bin").exists());
}
#[test]
fn profile_mutation_requires_current_cookie_csrf_and_origin_without_identity_overrides() {
    let (_directory, data) = configured();
    let (_server, address) = start_with(&data);
    let issued = sign_in(address);
    let raw = save_request(address, &issued, "Untrusted fixture");
    for header in ["Cookie:", "X-CSRF-Token:"] {
        let removed = raw
            .split("\r\n")
            .filter(|line| !line.starts_with(header))
            .collect::<Vec<_>>()
            .join("\r\n");
        assert_eq!(check(&request(address, &removed), 401), "");
        let line = raw
            .split("\r\n")
            .find(|line| line.starts_with(header))
            .unwrap();
        assert_eq!(
            check(
                &request(
                    address,
                    &raw.replacen(line, &format!("{line}\r\n{line}"), 1)
                ),
                401
            ),
            ""
        );
    }
    for origin in ["", "Origin: https://attacker.invalid\r\n"] {
        assert!(
            request(
                address,
                &raw.replace(&format!("Origin: http://{address}\r\n"), origin)
            )
            .starts_with("HTTP/1.1 403")
        );
    }
    let mixed = raw.replace(
        "Connection:",
        "Authorization: Bearer fixture\r\nConnection:",
    );
    assert_eq!(check(&request(address, &mixed), 401), "");
    let wrong = raw.replace(issued.header("x-csrf-token").unwrap(), &"00".repeat(32));
    assert_eq!(check(&request(address, &wrong), 401), "");
    let current = sign_in(address);
    assert_eq!(check(&request(address, &raw), 401), "");
    assert!(!data.join("profile.bin").exists());
    let ignored = save_request(address, &current, "Fixture").replace(
        "Connection:",
        "X-User-Id: fake\r\nX-Role: admin\r\nConnection:",
    );
    check(&request(address, &ignored), 204);
    assert_eq!(
        &fs::read(data.join("profile.bin")).unwrap()[9..25],
        &[7; 16]
    );
    assert_eq!(
        post(address, "/auth/sign-out", Some(&current), "").status,
        204
    );
    check(
        &request(address, &save_request(address, &current, "After logout")),
        401,
    );
}
#[test]
fn invalid_input_and_unavailable_storage_never_reset_profile_or_report_success() {
    let (_directory, data) = configured();
    let (server, address) = start_with(&data);
    let issued = sign_in(address);
    check(
        &request(address, &save_request(address, &issued, "Saved fixture")),
        204,
    );
    let saved = fs::read(data.join("profile.bin")).unwrap();
    for name in [
        " padded ",
        "Ada\u{feff}",
        "a\nb",
        "a\u{202e}",
        &"é".repeat(65),
    ] {
        check(
            &request(address, &save_request(address, &issued, name)),
            if name.len() > 128 { 413 } else { 400 },
        );
    }
    let raw = save_request(address, &issued, "New fixture");
    for replacement in [
        "application/json",
        "text/plain\r\nContent-Encoding: gzip",
        "text/plain\r\nContent-Type: text/plain",
    ] {
        check(
            &request(
                address,
                &raw.replace("text/plain;charset=UTF-8", replacement),
            ),
            415,
        );
    }
    assert_eq!(fs::read(data.join("profile.bin")).unwrap(), saved);
    fs::set_permissions(&data, fs::Permissions::from_mode(0o500)).unwrap();
    check(&request(address, &raw), 503);
    fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(fs::read(data.join("profile.bin")).unwrap(), saved);
    fs::write(data.join("profile.bin"), b"damaged private fixture").unwrap();
    let unavailable = read(address, "GET", "/account/profile", Some(&issued));
    assert!(!check(&unavailable, 503).contains("damaged private"));
    check(&request(address, &raw), 503);
    drop(server);
    let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_devoption"))
        .args([
            "serve",
            "--port",
            &occupied.local_addr().unwrap().port().to_string(),
            "--data-dir",
        ])
        .arg(&data)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("profile unavailable:")
    );
    assert_eq!(
        fs::read(data.join("profile.bin")).unwrap(),
        b"damaged private fixture"
    );
}
