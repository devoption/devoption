use std::{
    io::{BufRead, BufReader, Read, Write},
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
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_devoption"))
            .args(["serve", "--port", "0"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
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

fn request(address: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT).unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(TIMEOUT)).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
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
            &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
        );
        let (headers, body) = get.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("HTTP/1.1 200 OK"));
        assert!(headers.contains("content-type: text/html; charset=utf-8"));
        assert!(body.starts_with("<!doctype html>"));
        assert!(body.contains(&format!("<title>{title} · DevOption</title>")));
        assert!(body.contains(&format!("<h1>{title}</h1>")));
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
        assert!(popup.contains("Account features are not available yet."));
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
            &format!("HEAD {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
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
        "GET /missing HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    assert!(missing.starts_with("HTTP/1.1 404 Not Found"));
    for path in ["/", "/projects", "/about", "/settings/appearance"] {
        let post = request(
            address,
            &format!(
                "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ),
        );
        assert!(post.starts_with("HTTP/1.1 405 Method Not Allowed"));
        assert!(post.contains("allow: GET,HEAD"));
    }
    let malformed = request(address, "GET / INVALID\r\n\r\n");
    assert!(malformed.starts_with("HTTP/1.1 400 Bad Request"));
    let healthy = request(
        address,
        "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    assert!(healthy.starts_with("HTTP/1.1 200 OK"));
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
