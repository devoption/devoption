use std::io::Write;
use wasip2::http::types::{Fields, IncomingRequest, Method, OutgoingBody, OutgoingResponse, ResponseOutparam};

struct App;
wasip2::http::proxy::export!(App);

impl wasip2::exports::http::incoming_handler::Guest for App {
    fn handle(request: IncomingRequest, output: ResponseOutparam) {
        let (status, content_type, content) = if !matches!(request.method(), Method::Get) {
            (405, "text/plain", "Method not allowed")
        } else {
            match request.path_with_query().as_deref().unwrap_or("/").split('?').next().unwrap_or("/") {
                "/" => (200, "text/html; charset=utf-8", "<!doctype html><html><head><title>DevOption app</title></head><body><h1>DevOption app revision A</h1></body></html>"),
                "/healthz" => (200, "text/plain", "ok A"),
                _ => (404, "text/plain", "Not found"),
            }
        };
        let headers = Fields::new();
        headers.set("content-type", &[content_type.as_bytes().to_vec()]).unwrap();
        let response = OutgoingResponse::new(headers);
        response.set_status_code(status).unwrap();
        let body = response.body().unwrap();
        ResponseOutparam::set(output, Ok(response));
        {
            let mut stream = body.write().unwrap();
            stream.write_all(content.as_bytes()).unwrap();
            stream.flush().unwrap();
        }
        OutgoingBody::finish(body, None).unwrap();
    }
}
