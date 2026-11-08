//! Disposable transport fixture, not an account/session implementation.
//! Fixed public test identities are deliberately unsuitable for production.
use std::io::Write;
use wasip2::http::types::{Fields, IncomingRequest, Method, OutgoingBody, OutgoingResponse, ResponseOutparam};
struct App;
wasip2::http::proxy::export!(App);
impl wasip2::exports::http::incoming_handler::Guest for App {
    fn handle(request: IncomingRequest, output: ResponseOutparam) {
        let headers = request.headers();
        assert!(headers.get("authorization").is_empty());
        assert!(headers.get("proxy-authorization").is_empty());
        assert!(headers.get("x-csrf-token").is_empty());
        assert!(headers.get("x-devoption-probe").is_empty());
        for value in headers.get("cookie") {
            assert!(value.starts_with(b"__Host-devoption-app-session="));
            assert!(!value.contains(&b';'));
        }
        let host = request.authority().unwrap();
        let audience = if host.starts_with("one.") { "one" } else { "two" };
        let session = format!("__Host-devoption-app-session=fixture_{audience}");
        let signed_in = headers.get("cookie") == vec![session.as_bytes().to_vec()];
        let origin = format!("https://{host}");
        let same_origin = headers.get("origin") == vec![origin.as_bytes().to_vec()];
        let csrf = headers.get("x-app-csrf-token") == vec![b"fixture-csrf".to_vec()];
        let mut status = 200;
        let mut cookie = None;
        let path = request.path_with_query().unwrap_or_default();
        let body = match (request.method(), path.as_str()) {
            (Method::Get, "/healthz") => "ok auth".to_owned(),
            (Method::Post, "/account/login") if same_origin && csrf => {
                cookie = Some(format!("{session}; Secure; HttpOnly; Path=/; SameSite=Strict"));
                "signed in".into()
            }
            (Method::Post, "/account/logout") if signed_in && same_origin && csrf => {
                cookie = Some("__Host-devoption-app-session=; Secure; HttpOnly; Path=/; SameSite=Strict; Max-Age=0".into());
                "signed out".into()
            }
            (Method::Post, "/account/change") if signed_in && same_origin && csrf => "changed".into(),
            (Method::Get, "/account/session") if signed_in => format!("member {audience}"),
            (Method::Get, "/unsafe-cookie") => {
                cookie = Some("__Host-devoption=guest_forgery; Secure; HttpOnly; Path=/; SameSite=Strict".into());
                "refused control cookie".into()
            }
            (Method::Get, "/domain-cookie") => {
                cookie = Some(format!("{session}; Secure; HttpOnly; Path=/; SameSite=Strict; Domain=example.test"));
                "refused domain cookie".into()
            }
            _ => { status = 401; "unauthorized".into() }
        };
        let fields = Fields::new();
        if let Some(cookie) = cookie { fields.append("set-cookie", cookie.as_bytes()).unwrap(); }
        let response = OutgoingResponse::new(fields);
        response.set_status_code(status).unwrap();
        let outgoing = response.body().unwrap();
        ResponseOutparam::set(output, Ok(response));
        { let mut stream = outgoing.write().unwrap(); stream.write_all(body.as_bytes()).unwrap(); stream.flush().unwrap(); }
        OutgoingBody::finish(outgoing, None).unwrap();
    }
}
