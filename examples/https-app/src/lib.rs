//! Generic HTTPS capability fixture. All credentials remain in the host.
use std::io::Write;
use wasip2::http::types::{Fields, IncomingRequest, OutgoingBody, OutgoingRequest, OutgoingResponse, ResponseOutparam, Scheme, Method};
struct App;
wasip2::http::proxy::export!(App);
impl wasip2::exports::http::incoming_handler::Guest for App {
    fn handle(request: IncomingRequest, output: ResponseOutparam) {
        let path = request.path_with_query().unwrap_or_default();
        let content = if path == "/healthz" {"ok https".to_owned()}
        else if path == "/isolation" {
            use std::net::ToSocketAddrs;
            assert!(std::env::vars_os().next().is_none());
            assert!(std::env::args_os().next().is_none());
            for path in ["/run/credentials/devoption-app.service/https.json","/etc/passwd","/component.wasm"] { assert!(std::fs::read(path).is_err()); }
            assert!(std::net::TcpStream::connect("127.0.0.1:443").is_err());
            assert!("api.example.test:443".to_socket_addrs().is_err());
            "guest isolated".to_owned()
        } else {
            let authority = if path == "/wrong-host" {"other.example.test"} else {"api.example.test"};
            let endpoint = if path == "/wrong-path" {"/other"} else {"/events"};
            let once = || {
                let headers = Fields::new();
                headers.set("content-type", &[b"application/json".to_vec()]).unwrap();
                if path != "/no-retry-key" {headers.set("idempotency-key", &[b"fixture-operation-001".to_vec()]).unwrap();}
                let outgoing = OutgoingRequest::new(headers);
                outgoing.set_method(&Method::Post).unwrap();
                outgoing.set_scheme(Some(&Scheme::Https)).unwrap();
                outgoing.set_authority(Some(authority)).unwrap();
                outgoing.set_path_with_query(Some(endpoint)).unwrap();
                let body = outgoing.body().unwrap();
                let response = wasip2::http::outgoing_handler::handle(outgoing, None).unwrap();
                {let mut stream = body.write().unwrap();let payload = if path == "/bad-json" {"invalid".to_owned()} else if path == "/oversize" {format!("{{\"event\":\"{}\"}}","x".repeat(33000))} else {format!("{{\"event\":\"{}\"}}",path)};stream.write_all(payload.as_bytes()).unwrap();stream.flush().unwrap();}
                OutgoingBody::finish(body,None).unwrap();
                response.subscribe().block();
                match response.get() {
                    Some(Ok(Ok(response))) => {
                        let status=response.status();
                        let body=response.consume().unwrap();
                        let stream=body.stream().unwrap();
                        let mut bytes=Vec::new();
                        while let Ok(chunk)=stream.blocking_read(4096) {if chunk.is_empty(){continue;}bytes.extend_from_slice(&chunk);if bytes.len()>65536{break;}}
                        format!("{} {}",status,String::from_utf8(bytes).unwrap())
                    },
                    Some(Ok(Err(wasip2::http::types::ErrorCode::HttpRequestDenied))) => "denied".to_owned(),
                    _ => "unavailable".to_owned(),
                }
            };
            let first=once();
            if path == "/twice" {format!("{first};{}",once())} else {first}
        };
        let response=OutgoingResponse::new(Fields::new());
        let body=response.body().unwrap();ResponseOutparam::set(output,Ok(response));
        {let mut stream=body.write().unwrap();stream.write_all(content.as_bytes()).unwrap();stream.flush().unwrap();}
        OutgoingBody::finish(body,None).unwrap();
    }
}
