use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(5);

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start() -> (Server, SocketAddr) {
    start_command(Command::new(env!("CARGO_BIN_EXE_devoption")).args(["serve", "--port", "0"]))
}

fn start_command(command: &mut Command) -> (Server, SocketAddr) {
    let mut server = Server(command.stdout(Stdio::piped()).spawn().unwrap());
    let stdout = server.0.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).unwrap();
        let _ = sender.send(line);
    });
    let line = receiver
        .recv_timeout(TIMEOUT)
        .expect("server startup timed out");
    let address: SocketAddr = line
        .strip_prefix("Listening on http://")
        .expect("server must announce its bound address")
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(address.ip(), Ipv4Addr::LOCALHOST);
    assert_ne!(address.port(), 0);
    (server, address)
}

#[cfg(unix)]
#[path = "server/authentication.rs"]
mod authentication;
#[cfg(unix)]
#[path = "server/data_directory.rs"]
mod data_directory;

fn send_request(address: SocketAddr, request: &str) -> TcpStream {
    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT).unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(TIMEOUT)).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    stream
}

fn read_response(reader: &mut impl Read) -> io::Result<String> {
    let mut response = String::new();
    reader.read_to_string(&mut response)?;
    Ok(response)
}

fn request(address: SocketAddr, request: &str) -> String {
    read_response(&mut send_request(address, request)).unwrap()
}

fn assert_parser_rejection(address: SocketAddr, message: &str) {
    let mut stream = send_request(address, message);
    let mut response = String::new();
    if let Err(error) = stream.read_to_string(&mut response) {
        // Only malformed requests rejected by the HTTP parser may reset TCP.
        assert_eq!(error.kind(), io::ErrorKind::ConnectionReset);
    }
    let (headers, body) = response.split_once("\r\n\r\n").expect("complete headers");
    assert!(matches!(
        headers.lines().next(),
        Some("HTTP/1.0 400 Bad Request" | "HTTP/1.1 400 Bad Request")
    ));
    let fields: Vec<_> = headers
        .lines()
        .skip(1)
        .map(|line| line.split_once(':').expect("valid response header"))
        .collect();
    let lengths: Vec<_> = fields
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.trim())
        .collect();
    assert_eq!(lengths, ["0"], "complete empty response framing");
    assert!(
        !fields
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"))
    );
    assert_eq!(body, "");
}

#[test]
fn normal_response_reads_fail_on_reset_after_partial_success() {
    struct Reset;
    impl Read for Reset {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::ConnectionReset.into())
        }
    }
    // Deterministically model the partial responses that could pass status/title
    // assertions, without platform-specific TCP reset setup or extra dependencies.
    for partial in [
        "HTTP/1.1 200 OK\r\n",
        "HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n<title>Home · DevOption</title>",
    ] {
        let mut reader = partial.as_bytes().chain(Reset);
        assert_eq!(
            read_response(&mut reader).unwrap_err().kind(),
            io::ErrorKind::ConnectionReset
        );
    }
}

#[test]
fn pages_share_layout_and_have_working_navigation() {
    let (_server, address) = start();
    for (path, title, message) in [
        ("/", "Home", "No home content yet."),
        (
            "/projects",
            "Projects",
            "Project management is not available yet.",
        ),
        ("/about", "About", "No about content yet."),
        (
            "/settings/appearance",
            "Appearance",
            "Choose how DevOption looks in this browser.",
        ),
    ] {
        let get = request(
            address,
            &format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"),
        );
        let (headers, body) = get.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("HTTP/1.1 200 OK"));
        assert!(headers.contains("content-type: text/html; charset=utf-8"));
        assert!(body.starts_with("<!doctype html>"));
        assert!(body.contains(&format!("<title>{title} · DevOption</title>")));
        assert!(body.contains(&format!(
            "<h1 id=\"page-title\" tabindex=\"-1\">{title}</h1>"
        )));
        assert!(body.contains(message));
        assert!(body.contains("<a class=\"brand\" href=\"/\" aria-label=\"DevOption home\"><svg "));
        let navigation = body
            .split_once("<nav aria-label=\"Main navigation\">")
            .unwrap()
            .1
            .split_once("</nav>")
            .unwrap()
            .0;
        for (href, label) in [
            ("/", "Home"),
            ("/projects", "Projects"),
            ("/about", "About"),
        ] {
            let current = if href == path {
                " aria-current=\"page\""
            } else {
                ""
            };
            let link = navigation
                .split_once(&format!("<a href=\"{href}\"{current}>"))
                .unwrap()
                .1
                .split_once("</a>")
                .unwrap()
                .0;
            assert!(link.contains(&format!("<span>{label}</span>")));
        }
        assert_eq!(body.matches("aria-current=\"page\"").count(), 1);
        assert!(body.contains("href=\"#main-content\""));
        assert!(body.contains("<main id=\"main-content\" tabindex=\"-1\">"));
        assert!(body.contains("<header class=\"content-header\">"));
        assert!(body.contains("In development</p>"));
        assert!(body.contains("popovertarget=\"profile-popup\" aria-label=\"Profile menu\""));
        let (_, popup) = body
            .split_once("<section id=\"profile-popup\" popover aria-labelledby=\"profile-title\">")
            .unwrap();
        let (popup, _) = popup.split_once("</section>").unwrap();
        assert!(popup.contains("id=\"profile-title\""));
        assert!(popup.contains("Sign in to your local account."));
        assert!(popup.contains("href=\"/sign-in\">Sign in</a>"));
        let appearance_page = path == "/settings/appearance";
        let current = if appearance_page {
            " aria-current=\"page\""
        } else {
            ""
        };
        assert!(popup.contains(&format!(
            "href=\"/settings/appearance\"{current}>Appearance</a>"
        )));
        assert!(!popup.contains("<select"));
        assert!(!popup.contains("<input"));
        assert!(!popup.contains("theme-preview"));
        assert!(body.find("</script>").unwrap() < body.find("</head>").unwrap());
        let main = body.split_once("<main id=\"main-content\"").unwrap().1;
        assert_eq!(main.contains("class=\"theme-preview\""), appearance_page);
        assert_eq!(body.contains("<select"), appearance_page);
        assert_eq!(body.contains("type=\"color\""), appearance_page);
        if appearance_page {
            assert!(main.contains("aria-label=\"Appearance preferences\""));
            assert!(main.contains("id=\"appearance-theme-note\" hidden"));
            assert!(main.contains("id=\"appearance-status\" role=\"status\""));
            assert!(main.contains("<label for=\"appearance-primary\">Primary color</label>"));
            assert!(main.contains("id=\"appearance-primary\" type=\"color\""));
            assert!(main.contains("id=\"appearance-reset\" type=\"button\""));
            for setting in ["theme", "mode", "accent"] {
                assert!(main.contains(&format!("<label for=\"appearance-{setting}\">")));
                assert!(main.contains(&format!(
                    "id=\"appearance-{setting}\" aria-describedby=\"appearance-status\" disabled"
                )));
            }
            for (value, label) in [
                ("nord", "Nord"),
                ("dracula", "Dracula"),
                ("tokyo-night", "Tokyo Night"),
                ("dark", "Dark"),
                ("light", "Light"),
                ("system", "System"),
                ("purple", "Purple"),
                ("blue", "Blue"),
                ("green", "Green"),
                ("custom", "Custom"),
            ] {
                assert!(main.contains(&format!("<option value=\"{value}\">{label}</option>")));
            }
        }

        let head = request(
            address,
            &format!("HEAD {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"),
        );
        assert!(head.starts_with("HTTP/1.1 200 OK"));
        assert_eq!(head.split_once("\r\n\r\n").unwrap().1, "");
    }
}

#[test]
fn errors_do_not_stop_the_server() {
    let (_server, address) = start();
    let missing = request(
        address,
        &format!("GET /missing HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"),
    );
    assert!(missing.starts_with("HTTP/1.1 404 Not Found"));
    for path in ["/", "/projects", "/about", "/settings/appearance"] {
        let post = request(
            address,
            &format!(
                "POST {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ),
        );
        assert!(post.starts_with("HTTP/1.1 401 Unauthorized"));
        assert!(!post.contains("www-authenticate:"));
    }
    assert_parser_rejection(address, "GET / INVALID\r\n\r\n");
    let healthy = request(
        address,
        &format!("GET / HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"),
    );
    assert!(healthy.starts_with("HTTP/1.1 200 OK"));
}

#[test]
fn local_authorities_work_in_http_10_and_11() {
    let (_server, address) = start();
    let port = address.port();
    for version in ["HTTP/1.0", "HTTP/1.1"] {
        for host in [
            format!("127.0.0.1:{port}"),
            format!("localhost:{port}"),
            format!("LoCaLhOsT:{port}"),
            format!("localhost:0{port}"),
            format!("\tlocalhost:{port}\t"),
        ] {
            let response = request(
                address,
                &format!(
                    "GET /?next=http://elsewhere.invalid/ {version}\r\nhOsT: {host}\r\n\
                     Forwarded: host=elsewhere.invalid\r\nX-Forwarded-Host: elsewhere.invalid\r\n\
                     Connection: close\r\n\r\n"
                ),
            );
            assert!(response.starts_with(&format!("{version} 200 OK")), "{host}");
            assert!(response.contains("<title>Home · DevOption</title>"));
        }
    }
}

fn assert_bad_request(address: SocketAddr, message: &str) {
    let response = request(address, message);
    assert_eq!(
        response.split_whitespace().nth(1),
        Some("400"),
        "{message:?}"
    );
    assert_eq!(
        response.split_once("\r\n\r\n").unwrap().1,
        "",
        "{message:?}"
    );
}

#[test]
fn missing_duplicate_and_deceptive_hosts_are_rejected() {
    let (_server, address) = start();
    let port = address.port();
    let mut headers = vec![
        String::new(),
        "Host:\r\n".into(),
        "Host: \t\r\n".into(),
        format!("Host: {address}\r\nHost: {address}\r\n"),
        format!("Host: {address}\r\nhOsT: attacker.invalid:{port}\r\n"),
        format!("Host: attacker.invalid:{port}\r\nHost: {address}\r\n"),
        format!("Forwarded: host={address}\r\nX-Forwarded-Host: {address}\r\n"),
        format!(
            "Host: attacker.invalid:{port}\r\nForwarded: host={address}\r\nX-Forwarded-Host: {address}\r\n"
        ),
    ];
    for host in [
        "localhost".into(),
        "127.0.0.1".into(),
        "localhost:0".into(),
        "localhost:80".into(),
        "localhost:65536".into(),
        "localhost:".into(),
        format!("localhost:+{port}"),
        format!("localhost:-{port}"),
        format!("localhost:{port}x"),
        format!("localhost: {port}"),
        format!("localhost:{port}:80"),
        format!("localhost:{port}, {address}"),
        format!("localhost.attacker.invalid:{port}"),
        format!("localhost.:{port}"),
        format!("127.0.0.1.attacker.invalid:{port}"),
        format!("127.1:{port}"),
        format!("127.0.0.2:{port}"),
        format!("2130706433:{port}"),
        format!("[::1]:{port}"),
        format!("[::ffff:127.0.0.1]:{port}"),
        format!("%6cocalhost:{port}"),
        format!("user@localhost:{port}"),
        format!("localhost:{port}@attacker.invalid"),
        format!("http://localhost:{port}"),
        format!("localhost:{port}/"),
        format!("localhost:{port}?x"),
        format!("localhost:{port}#x"),
        format!("local\thost:{port}"),
        format!("lócálhost:{port}"),
    ] {
        headers.push(format!("Host: {host}\r\n"));
    }
    // The application's explicit-Host policy also applies to HTTP/1.0.
    for version in ["HTTP/1.0", "HTTP/1.1"] {
        assert_parser_rejection(
            address,
            &format!("GET / {version}\r\nHost : {address}\r\nConnection: close\r\n\r\n"),
        );
        for fields in &headers {
            assert_bad_request(
                address,
                &format!("GET / {version}\r\n{fields}Connection: close\r\n\r\n"),
            );
        }
    }
    // The guard must cover route misses and unsupported methods too.
    for (method, path) in [("GET", "/missing"), ("POST", "/projects"), ("HEAD", "/")] {
        assert_bad_request(
            address,
            &format!(
                "{method} {path} HTTP/1.1\r\nHost: attacker.invalid:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ),
        );
    }
}

#[test]
fn absolute_targets_must_match_the_allowed_host_and_port() {
    let (_server, address) = start();
    let port = address.port();
    for version in ["HTTP/1.0", "HTTP/1.1"] {
        for (authority, host) in [
            (address.to_string(), address.to_string()),
            (format!("LOCALHOST:{port}"), format!("localhost:{port}")),
            (format!("localhost:0{port}"), format!("localhost:{port}")),
        ] {
            for (method, path, status) in [
                ("GET", "", "200"),
                ("GET", "/projects?tab=all", "200"),
                ("HEAD", "/about", "200"),
                ("GET", "/missing", "404"),
                ("POST", "/", "401"),
            ] {
                let response = request(
                    address,
                    &format!(
                        "{method} http://{authority}{path} {version}\r\nHost: {host}\r\nOrigin: http://{host}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    ),
                );
                assert_eq!(response.split_whitespace().nth(1), Some(status));
                if method == "HEAD" {
                    assert_eq!(response.split_once("\r\n\r\n").unwrap().1, "");
                }
            }
        }
    }
    for target in [
        format!("http://attacker.invalid:{port}/"),
        format!("http://localhost.attacker.invalid:{port}/"),
        format!("http://localhost:{port}/"), // Different allowed alias, conflicting Host.
        "http://127.0.0.1/".into(),
        "http://127.0.0.1:0/".into(),
        "http://127.0.0.1:/".into(),
        format!("http://user@{address}/"),
        format!("http://{address}@attacker.invalid/"),
        format!("https://{address}/"),
        format!("ftp://{address}/"),
        format!("//{address}/"),
        address.to_string(),
        "*".into(),
    ] {
        assert_bad_request(
            address,
            &format!("GET {target} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"),
        );
    }
    // A safe target cannot repair an absent, foreign, or duplicated Host.
    for host in [
        String::new(),
        "Host: attacker.invalid\r\n".into(),
        format!("Host: {address}\r\nHost: {address}\r\n"),
    ] {
        assert_bad_request(
            address,
            &format!("GET http://{address}/ HTTP/1.1\r\n{host}Connection: close\r\n\r\n"),
        );
    }
    assert_bad_request(
        address,
        &format!("CONNECT {address} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"),
    );
}

#[test]
fn authority_is_checked_for_each_request_on_a_connection() {
    let (_server, address) = start();
    let response = request(
        address,
        &format!(
            "GET / HTTP/1.1\r\nHost: {address}\r\n\r\n\
         GET / HTTP/1.1\r\nHost: attacker.invalid\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(response.matches("HTTP/1.1 400 Bad Request").count(), 1);
    assert_eq!(response.matches("<!doctype html>").count(), 1);
}

#[test]
fn unsafe_and_extension_methods_require_origin_before_routing() {
    let (_server, address) = start();
    let port = address.port();
    for version in ["HTTP/1.0", "HTTP/1.1"] {
        for host in [address.to_string(), format!("LoCaLhOsT:0{port}")] {
            for method in ["POST", "PUT", "PATCH", "DELETE", "CUSTOM", "QUERY", "post"] {
                for (origin, status) in [
                    (String::new(), "403"),
                    (format!("Origin: HTTP://{host}\r\n"), "401"),
                ] {
                    let response = request(
                        address,
                        &format!(
                            "{method} / {version}\r\nHost: {host}\r\n{origin}Content-Length: 0\r\nConnection: close\r\n\r\n"
                        ),
                    );
                    assert_eq!(
                        response.split_whitespace().nth(1),
                        Some(status),
                        "{method} {host}"
                    );
                    assert_eq!(response.split_once("\r\n\r\n").unwrap().1, "");
                }
            }
        }
    }
    for (origin, status) in [
        ("", "403"),
        (&format!("Origin: http://{address}\r\n"), "401"),
    ] {
        let response = request(
            address,
            &format!(
                "POST /missing HTTP/1.1\r\nHost: {address}\r\n{origin}Content-Length: 0\r\nConnection: close\r\n\r\n"
            ),
        );
        assert_eq!(response.split_whitespace().nth(1), Some(status));
    }
}

#[test]
fn malformed_and_cross_origins_cannot_submit_simple_forms() {
    let (_server, address) = start();
    let port = address.port();
    let mut fields = vec![
        String::new(),
        format!("Origin: http://{address}\r\nOrigin: http://{address}\r\n"),
        format!("Origin: http://{address}\r\noRiGiN: http://attacker.invalid\r\n"),
        format!("Origin: http://attacker.invalid\r\nOrigin: http://{address}\r\n"),
        format!(
            "Referer: http://{address}/\r\nSec-Fetch-Site: same-origin\r\nForwarded: host={address}\r\nX-Forwarded-Host: {address}\r\nX-Forwarded-Proto: http\r\n"
        ),
    ];
    for origin in [
        "".into(),
        "null".into(),
        "NULL".into(),
        format!("http://localhost:{port}"), // A different origin from the IP Host.
        "http://127.0.0.1".into(),
        "http://127.0.0.1:0".into(),
        "http://127.0.0.1:65536".into(),
        format!("http://127.0.0.1:+{port}"),
        format!("https://{address}"),
        format!("ftp://{address}"),
        format!("//{address}"),
        format!("http://{address}/"),
        format!("http://{address}?x"),
        format!("http://{address}#x"),
        format!("http://user@{address}"),
        format!("http://{address}@attacker.invalid"),
        format!("http://127.0.0.1.attacker.invalid:{port}"),
        format!("http://127.0.0.1.:{port}"),
        format!("http://%31%32%37.0.0.1:{port}"),
        format!("http://[::1]:{port}"),
        format!("http://127.1:{port}"),
        format!("http://lócálhost:{port}"),
        format!("http://{address}, http://{address}"),
        format!("http://{address} http://{address}"),
        format!("http://{address}\\attacker.invalid"),
    ] {
        fields.push(format!("Origin: {origin}\r\n"));
    }
    for version in ["HTTP/1.0", "HTTP/1.1"] {
        for content_type in [
            "application/x-www-form-urlencoded",
            "multipart/form-data; boundary=x",
            "text/plain",
        ] {
            for origin in &fields {
                let response = request(
                    address,
                    &format!(
                        "POST /projects {version}\r\nHost: {address}\r\n{origin}Content-Type: {content_type}\r\nContent-Length: 3\r\nConnection: close\r\n\r\nx=1"
                    ),
                );
                assert_eq!(
                    response.split_whitespace().nth(1),
                    Some("403"),
                    "{origin:?}"
                );
                assert_eq!(response.split_once("\r\n\r\n").unwrap().1, "");
                assert!(
                    !response
                        .to_ascii_lowercase()
                        .contains("access-control-allow-origin")
                );
            }
            let response = request(
                address,
                &format!(
                    "POST /projects {version}\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: {content_type}\r\nContent-Length: 3\r\nConnection: close\r\n\r\nx=1"
                ),
            );
            assert_eq!(response.split_whitespace().nth(1), Some("401"));
        }
    }
    let response = request(
        address,
        &format!(
            "POST / HTTP/1.1\r\nHost: localhost:{port}\r\nOrigin: http://{address}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ),
    );
    assert_eq!(response.split_whitespace().nth(1), Some("403"));
}

#[test]
fn unprovisioned_server_denies_credentials_without_echoing_them() {
    let (_server, address) = start();
    let fixture = "ab".repeat(32);
    for version in ["HTTP/1.0", "HTTP/1.1"] {
        for path in ["/", "/projects", "/missing"] {
            for authorization in [
                String::new(),
                format!("Authorization: Bearer {fixture}\r\n"),
                format!("Authorization: Bearer {fixture}\r\naUtHoRiZaTiOn: Bearer {fixture}\r\n"),
                format!("Authorization: Bearer {fixture}, Bearer {fixture}\r\n"),
            ] {
                let response = request(
                    address,
                    &format!(
                        "POST {path} {version}\r\nHost: {address}\r\nOrigin: http://{address}\r\n{authorization}Content-Length: 0\r\nConnection: close\r\n\r\n"
                    ),
                );
                let (headers, body) = response.split_once("\r\n\r\n").unwrap();
                assert!(headers.starts_with(&format!("{version} 401 Unauthorized")));
                assert!(!headers.contains("www-authenticate:"));
                assert!(headers.contains("cache-control: no-store\r\n"));
                assert!(!headers.contains("set-cookie:"));
                assert!(!headers.contains("access-control-allow-origin:"));
                assert!(!response.contains(&fixture));
                assert_eq!(body, "");
            }
        }
    }
}

#[test]
fn safe_methods_keep_existing_navigation_and_do_not_enable_cors() {
    let (_server, address) = start();
    for (method, status) in [
        ("GET", "200"),
        ("HEAD", "200"),
        ("OPTIONS", "405"),
        ("TRACE", "405"),
    ] {
        for origin in [
            "",
            "Origin: null\r\n",
            "Origin: http://attacker.invalid\r\n",
        ] {
            let response = request(
                address,
                &format!(
                    "{method} / HTTP/1.1\r\nHost: {address}\r\n{origin}Connection: close\r\n\r\n"
                ),
            );
            assert_eq!(response.split_whitespace().nth(1), Some(status));
            assert!(
                !response
                    .to_ascii_lowercase()
                    .contains("access-control-allow-origin")
            );
        }
    }
}

#[test]
fn origin_checks_cover_absolute_targets_and_each_request() {
    let (_server, address) = start();
    for (target, host, origin, status) in [
        (
            format!("http://{address}/"),
            address.to_string(),
            format!("http://{address}"),
            "401",
        ),
        (
            format!("http://{address}/"),
            address.to_string(),
            "http://attacker.invalid".into(),
            "403",
        ),
        (
            "http://attacker.invalid/".into(),
            address.to_string(),
            format!("http://{address}"),
            "400",
        ),
        (
            "/".into(),
            "attacker.invalid".into(),
            format!("http://{address}"),
            "400",
        ),
    ] {
        let response = request(
            address,
            &format!(
                "POST {target} HTTP/1.1\r\nHost: {host}\r\nOrigin: {origin}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ),
        );
        assert_eq!(response.split_whitespace().nth(1), Some(status));
    }
    let response = request(
        address,
        &format!(
            "POST / HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Length: 0\r\n\r\nPOST / HTTP/1.1\r\nHost: {address}\r\nOrigin: null\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(response.starts_with("HTTP/1.1 401 Unauthorized"));
    assert_eq!(response.matches("HTTP/1.1 403 Forbidden").count(), 1);
}

#[test]
fn occupied_port_is_a_clear_startup_failure() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_devoption"))
        .args(["serve", "--port", &port])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains(&format!("could not serve on 127.0.0.1:{port}")));
    assert!(!error.contains("panicked"));
}
