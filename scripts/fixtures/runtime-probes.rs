//! Disposable runtime policy probes; never used as the example application.
use std::io::Write;
use wasip2::http::types::{Fields, IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam};
struct App;
wasip2::http::proxy::export!(App);
impl wasip2::exports::http::incoming_handler::Guest for App {
    fn handle(request: IncomingRequest, output: ResponseOutparam) {
        let path = request.path_with_query().unwrap_or_default();
        let content = match path.as_str() {
            "/loop" => loop { std::hint::black_box(1); },
            "/sleep" => { std::thread::sleep(std::time::Duration::from_secs(10)); "late".into() },
            "/memory" => { let mut bytes=vec![1u8;100*1024*1024]; std::hint::black_box(&mut bytes); "unexpected memory grant".into() },
            "/overflow" => "x".repeat(1024*1024+1),
            "/stdout" => { print!("{}", "x".repeat(128*1024)); "unexpected output grant".into() },
            "/capabilities" => {
                assert!(std::env::vars_os().next().is_none());
                assert!(std::env::args_os().next().is_none());
                assert!(std::fs::read("/etc/passwd").is_err());
                assert!(std::fs::write("/tmp/probe",b"denied").is_err());
                assert!(std::net::TcpStream::connect("127.0.0.1:18081").is_err());
                assert!(std::net::UdpSocket::bind("127.0.0.1:0").is_err());
                use std::net::ToSocketAddrs;
                assert!("example.com:80".to_socket_addrs().is_err());
                "files environment sockets DNS denied".into()
            },
            "/outbound" => {
                use wasip2::http::types::{OutgoingRequest, Scheme, ErrorCode};
                for authority in ["127.0.0.1:18081", "10.0.0.1", "169.254.169.254", "example.com"] {
                    let request=OutgoingRequest::new(Fields::new());
                    request.set_scheme(Some(&Scheme::Http)).unwrap();
                    request.set_authority(Some(authority)).unwrap();
                    request.set_path_with_query(Some("/")).unwrap();
                    let response=wasip2::http::outgoing_handler::handle(request,None).unwrap();
                    response.subscribe().block();
                    assert!(matches!(response.get(),Some(Ok(Err(ErrorCode::HttpRequestDenied)))));
                }
                "outbound HTTP denied".into()
            },
            "/headers" => {
                let headers=request.headers();
                assert!(headers.get("authorization").is_empty());
                assert!(headers.get("cookie").is_empty());
                assert!(headers.get("x-devoption-probe").is_empty());
                "control headers removed".into()
            },
            "/healthz" => "ok probes".into(),
            _ => "probe".into(),
        };
        let response=OutgoingResponse::new(Fields::new());
        let body=response.body().unwrap();
        ResponseOutparam::set(output,Ok(response));
        { let mut stream=body.write().unwrap(); stream.write_all(content.as_bytes()).unwrap(); stream.flush().unwrap(); }
        OutgoingBody::finish(body,None).unwrap();
    }
}
