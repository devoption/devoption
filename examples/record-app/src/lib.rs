//! Public generic persistence fixture. Not a form service or admin API.
use std::io::Write;
use wasip2::http::types::{Fields, IncomingRequest, Method, OutgoingBody, OutgoingResponse, ResponseOutparam};

#[cfg(all(feature = "record-reads",feature = "delivery-jobs"))]
compile_error!("select separate fixture profiles");
#[cfg(all(not(feature = "record-reads"),not(feature = "delivery-jobs")))]
wit_bindgen::generate!({ path: "../../crates/devoption-runtime/wit", world: "record-imports" });
#[cfg(all(feature = "record-reads",not(feature = "delivery-jobs")))]
wit_bindgen::generate!({ path: "../../crates/devoption-runtime/wit", world: "record-access-imports" });
#[cfg(feature = "delivery-jobs")]
wit_bindgen::generate!({ path: "../../crates/devoption-runtime/wit", world: "delivery-imports" });
struct App;
wasip2::http::proxy::export!(App);

impl wasip2::exports::http::incoming_handler::Guest for App {
    fn handle(request: IncomingRequest, output: ResponseOutparam) {
        use devoption::app::records::{append, WriteError, WriteOutcome};
        let path = request.path_with_query().unwrap_or_default();
        #[cfg(feature = "record-reads")]
        let mut record_payload: Option<String> = None;
        #[cfg(not(feature = "record-reads"))]
        let record_payload: Option<String> = None;
        let (status, content) = if matches!(request.method(), Method::Get) && path == "/denials" {
            use std::net::ToSocketAddrs;
            use wasip2::http::types::{OutgoingRequest, Scheme, ErrorCode};
            assert!(std::env::vars_os().next().is_none());
            assert!(std::env::args_os().next().is_none());
            assert!(std::fs::read("/etc/passwd").is_err());
            assert!(std::fs::write("/tmp/record-probe", b"denied").is_err());
            assert!(std::net::TcpStream::connect("127.0.0.1:5432").is_err());
            assert!(std::net::UdpSocket::bind("127.0.0.1:0").is_err());
            assert!("example.com:80".to_socket_addrs().is_err());
            for authority in ["127.0.0.1:5432", "10.0.0.1", "169.254.169.254", "example.com"] {
                let outgoing = OutgoingRequest::new(Fields::new());
                outgoing.set_scheme(Some(&Scheme::Http)).unwrap();
                outgoing.set_authority(Some(authority)).unwrap();
                outgoing.set_path_with_query(Some("/")).unwrap();
                let response = wasip2::http::outgoing_handler::handle(outgoing, None).unwrap();
                response.subscribe().block();
                assert!(matches!(response.get(), Some(Ok(Err(ErrorCode::HttpRequestDenied)))));
            }
            (200, "files environment sockets DNS outgoing HTTP denied")
        } else if matches!(request.method(), Method::Get) && path == "/healthz" {
            (200, "ok records")
        } else if matches!(request.method(), Method::Post) && (path.starts_with("/enqueue/") || path.starts_with("/later/")) {
            #[cfg(feature = "delivery-jobs")]
            {
                use devoption::app::delivery_jobs::{enqueue,EnqueueOutcome,EnqueueError};
                let body=request.consume().unwrap();let stream=body.stream().unwrap();let mut bytes=Vec::new();
                while let Ok(chunk)=stream.blocking_read(4096) {bytes.extend_from_slice(&chunk);if bytes.len()>24576 {break;}}
                let (id,delay)=if path.starts_with("/later/"){(&path[7..],60)}else{(&path[9..],0)};
                match std::str::from_utf8(&bytes).ok().map(|p|enqueue(id,p,delay)) {
                    Some(Ok(EnqueueOutcome::Accepted))=>(201,"accepted"),
                    Some(Ok(EnqueueOutcome::Replayed))=>(200,"replayed"),
                    Some(Err(EnqueueError::Conflict))=>(409,"conflict"),
                    Some(Err(EnqueueError::Unavailable))=>(503,"unavailable"),
                    Some(Err(EnqueueError::Denied))=>(403,"denied"),
                    _=>(422,"invalid"),
                }
            }
            #[cfg(not(feature = "delivery-jobs"))]
            {(404,"not found")}
        } else if matches!(request.method(), Method::Post) && path.starts_with("/append/") {
            let body = request.consume().unwrap();
            let stream = body.stream().unwrap();
            let mut bytes = Vec::new();
            while let Ok(chunk) = stream.blocking_read(4096) {
                bytes.extend_from_slice(&chunk);
                if bytes.len() > 24 * 1024 { break; }
            }
            match std::str::from_utf8(&bytes).ok().map(|s| append(&path[8..], s)) {
                Some(Ok(WriteOutcome::Created)) => (201, "created"),
                Some(Ok(WriteOutcome::Replayed)) => (200, "replayed"),
                Some(Err(WriteError::Conflict)) => (409, "conflict"),
                Some(Err(WriteError::Unavailable)) => (503, "unavailable"),
                Some(Err(WriteError::Denied)) => (503, "denied"),
                _ => (422, "invalid"),
            }
        } else if matches!(request.method(), Method::Get) && path.starts_with("/record/") {
            #[cfg(feature = "record-reads")]
            {
                use devoption::app::record_lookup::{get, ReadError};
                match get(&path[8..]) {
                    Ok(Some(payload)) => { record_payload = Some(payload); (200, "record") },
                    Ok(None) => (404, "not found"),
                    Err(ReadError::Invalid) => (422, "invalid"),
                    Err(ReadError::Denied) => (403, "denied"),
                    Err(ReadError::Unavailable) => (503, "unavailable"),
                }
            }
            #[cfg(not(feature = "record-reads"))]
            { (404, "not found") }
        } else { (404, "not found") };
        let headers = Fields::new();
        headers.set("content-type", &[if record_payload.is_some() { b"application/json".to_vec() } else { b"text/plain".to_vec() }]).unwrap();
        headers.set("cache-control", &[b"no-store".to_vec()]).unwrap();
        let response = OutgoingResponse::new(headers);
        response.set_status_code(status).unwrap();
        let body = response.body().unwrap();
        ResponseOutparam::set(output, Ok(response));
        { let mut stream = body.write().unwrap(); stream.write_all(record_payload.as_deref().unwrap_or(content).as_bytes()).unwrap(); stream.flush().unwrap(); }
        OutgoingBody::finish(body, None).unwrap();
    }
}
