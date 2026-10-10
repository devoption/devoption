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
fn serves_get_and_head_on_loopback() {
    let (_server, address) = start();
    let get = request(
        address,
        "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    let (headers, body) = get.split_once("\r\n\r\n").unwrap();
    assert!(headers.starts_with("HTTP/1.1 200 OK"));
    assert!(headers.contains("content-type: text/plain; charset=utf-8"));
    assert_eq!(body, "DevOption local server\n");

    let head = request(
        address,
        "HEAD / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    assert!(head.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(head.split_once("\r\n\r\n").unwrap().1, "");
}

#[test]
fn errors_do_not_stop_the_server() {
    let (_server, address) = start();
    let missing = request(
        address,
        "GET /missing HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    assert!(missing.starts_with("HTTP/1.1 404 Not Found"));
    let post = request(
        address,
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(post.starts_with("HTTP/1.1 405 Method Not Allowed"));
    assert!(post.contains("allow: GET,HEAD"));
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
