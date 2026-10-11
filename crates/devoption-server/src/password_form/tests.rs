use super::*;
use axum::{
    Router,
    body::{Body, Bytes},
    routing::any,
};
use http_body::Frame;
use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll},
};
use tower::ServiceExt;

fn request(body: Body) -> Request {
    Request::builder()
        .method(Method::POST)
        .uri("/form")
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body)
        .unwrap()
}

fn response(app: &Router, request: Request) -> (StatusCode, HeaderMap, Bytes) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let response = app.clone().oneshot(request).await.unwrap();
            let (parts, body) = response.into_parts();
            (
                parts.status,
                parts.headers,
                to_bytes(body, 1024).await.unwrap(),
            )
        })
}

fn fixture() -> (Router, Arc<Mutex<Vec<String>>>) {
    let accepted = Arc::new(Mutex::new(Vec::new()));
    let handler_accepted = Arc::clone(&accepted);
    // `any` also verifies that accidental registration on a safe method fails closed.
    let app = Router::new().route(
        "/form",
        any(move |form: PasswordForm| async move {
            handler_accepted.lock().unwrap().push(form.into_password());
            StatusCode::NO_CONTENT
        }),
    );
    (app, accepted)
}

fn reject(app: &Router, request: Request, expected: StatusCode) {
    let (status, headers, body) = response(app, request);
    assert_eq!(status, expected);
    assert_eq!(headers[CACHE_CONTROL], "no-store");
    assert!(body.is_empty());
    assert!(!headers.contains_key("set-cookie"));
    if status == StatusCode::METHOD_NOT_ALLOWED {
        assert_eq!(headers[ALLOW], "POST");
    }
}

fn encoded(password: &str) -> String {
    format!(
        "password={}",
        password
            .as_bytes()
            .iter()
            .map(|byte| format!("%{byte:02X}"))
            .collect::<String>()
    )
}

struct Frames {
    frames: VecDeque<Result<Bytes, io::Error>>,
    pending: bool,
    polls: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
}

impl http_body::Body for Frames {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        if let Some(frame) = self.frames.pop_front() {
            Poll::Ready(Some(frame.map(Frame::data)))
        } else if self.pending {
            Poll::Pending
        } else {
            Poll::Ready(None)
        }
    }
}

impl Drop for Frames {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

fn stream(
    frames: Vec<Result<Bytes, io::Error>>,
    pending: bool,
) -> (Body, Arc<AtomicUsize>, Arc<AtomicBool>) {
    let polls = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicBool::new(false));
    let body = Body::new(Frames {
        frames: frames.into(),
        pending,
        polls: Arc::clone(&polls),
        dropped: Arc::clone(&dropped),
    });
    (body, polls, dropped)
}

#[test]
fn url_encoding_preserves_password_bytes_and_supported_utf8_media_types() {
    let (app, accepted) = fixture();
    for (input, expected, media) in [
        (
            "password=fixture",
            "fixture",
            "application/x-www-form-urlencoded",
        ),
        (
            "password=++caf%C3%A9+%2b%26%3D%00%0A",
            "  café +&=\0\n",
            "application/x-www-form-urlencoded; charset=UTF-8",
        ),
        (
            "%70assword=cafe%CC%81",
            "cafe\u{301}",
            "Application/X-WWW-Form-Urlencoded; Charset=\"utf-8\"",
        ),
        ("password=a=b", "a=b", "application/x-www-form-urlencoded"),
    ] {
        let mut req = request(Body::from(input));
        req.headers_mut()
            .insert(CONTENT_TYPE, media.parse().unwrap());
        req.headers_mut().insert(CONTENT_LENGTH, input.len().into());
        assert_eq!(response(&app, req).0, StatusCode::NO_CONTENT);
        assert_eq!(accepted.lock().unwrap().last().unwrap(), expected);
    }
}

#[test]
fn decoded_password_limit_is_in_bytes_without_truncation() {
    let (app, accepted) = fixture();
    let maximum = "🔑".repeat(password::MAX_PASSWORD_BYTES / 4);
    assert_eq!(
        response(&app, request(Body::from(encoded(&maximum)))).0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(accepted.lock().unwrap()[0], maximum);
    reject(
        &app,
        request(Body::from(encoded(&format!("{maximum}x")))),
        StatusCode::BAD_REQUEST,
    );
    assert_eq!(accepted.lock().unwrap().len(), 1);
}

#[test]
fn malformed_unknown_and_duplicate_fields_never_reach_the_handler() {
    let (app, accepted) = fixture();
    for body in [
        "",
        "password",
        "password=",
        "other=fixture",
        "Password=fixture",
        "password=one&password=two",
        "password=one&%70assword=two",
        "password=one&role=owner",
        "&password=fixture",
        "password=fixture&",
        "password=%",
        "password=%0",
        "password=%GG",
        "password=%FF",
        "password=%C0%AF",
        "password=%ED%A0%80",
        "pass%GGword=fixture",
    ] {
        reject(&app, request(Body::from(body)), StatusCode::BAD_REQUEST);
    }
    reject(
        &app,
        request(Body::from(b"password=\xff".to_vec())),
        StatusCode::BAD_REQUEST,
    );
    assert!(accepted.lock().unwrap().is_empty());
}

#[test]
fn unsafe_media_and_ambiguous_headers_are_rejected_before_body_reads() {
    let (app, accepted) = fixture();
    for (name, values, expected) in [
        (CONTENT_TYPE, vec![], StatusCode::UNSUPPORTED_MEDIA_TYPE),
        (
            CONTENT_TYPE,
            vec!["application/json"],
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            CONTENT_TYPE,
            vec!["application/x-www-form-urlencoded; charset=iso-8859-1"],
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            CONTENT_TYPE,
            vec!["application/x-www-form-urlencoded; charset=utf-8; charset=utf-8"],
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            CONTENT_TYPE,
            vec![
                "application/x-www-form-urlencoded",
                "application/x-www-form-urlencoded",
            ],
            StatusCode::BAD_REQUEST,
        ),
        (
            CONTENT_ENCODING,
            vec!["gzip"],
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            CONTENT_ENCODING,
            vec!["identity", "identity"],
            StatusCode::BAD_REQUEST,
        ),
        (CONTENT_LENGTH, vec!["4097"], StatusCode::PAYLOAD_TOO_LARGE),
        (CONTENT_LENGTH, vec!["+10"], StatusCode::BAD_REQUEST),
        (CONTENT_LENGTH, vec!["10", "10"], StatusCode::BAD_REQUEST),
    ] {
        let (body, polls, dropped) = stream(vec![], true);
        let mut req = request(body);
        req.headers_mut().remove(&name);
        for value in values {
            req.headers_mut().append(&name, value.parse().unwrap());
        }
        reject(&app, req, expected);
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        assert!(dropped.load(Ordering::SeqCst));
    }
    assert!(accepted.lock().unwrap().is_empty());
}

#[test]
fn body_limit_counts_actual_frames_despite_a_false_length_header() {
    let (app, accepted) = fixture();
    let (body, polls, dropped) = stream(
        vec![
            Ok(Bytes::from(vec![b'x'; 3000])),
            Ok(Bytes::from(vec![b'x'; 1097])),
            Ok(Bytes::from_static(b"must not be read")),
        ],
        false,
    );
    let mut req = request(body);
    req.headers_mut()
        .insert(CONTENT_LENGTH, HeaderValue::from_static("1"));
    reject(&app, req, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(polls.load(Ordering::SeqCst), 2);
    assert!(dropped.load(Ordering::SeqCst));
    let exact = format!("password={}", "x".repeat(MAX_FORM_BYTES - 9));
    // At the encoded limit, the separate decoded-password bound rejects the form.
    reject(
        &app,
        request(Body::from(exact.clone())),
        StatusCode::BAD_REQUEST,
    );
    reject(
        &app,
        request(Body::from(format!("{exact}x"))),
        StatusCode::PAYLOAD_TOO_LARGE,
    );
    assert!(accepted.lock().unwrap().is_empty());
}

#[test]
fn successful_multiframe_reads_and_truncated_or_failed_reads_are_distinct() {
    let (app, accepted) = fixture();
    let (body, _, _) = stream(
        vec![
            Ok(Bytes::from_static(b"pass")),
            Ok(Bytes::from_static(b"word=fixture")),
        ],
        false,
    );
    assert_eq!(response(&app, request(body)).0, StatusCode::NO_CONTENT);
    for length in ["1", "100"] {
        let mut req = request(Body::from("password=fixture"));
        req.headers_mut()
            .insert(CONTENT_LENGTH, length.parse().unwrap());
        reject(&app, req, StatusCode::BAD_REQUEST);
    }
    let (body, _, dropped) = stream(
        vec![
            Ok(Bytes::from_static(b"password=fixture")),
            Err(io::Error::other("private fixture error detail")),
        ],
        false,
    );
    reject(&app, request(body), StatusCode::BAD_REQUEST);
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(*accepted.lock().unwrap(), ["fixture"]);
}

#[test]
fn incomplete_body_hits_the_total_deadline_and_is_dropped() {
    let (app, accepted) = fixture();
    let (body, polls, dropped) = stream(vec![Ok(Bytes::from_static(b"password=partial"))], true);
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let start = tokio::time::Instant::now();
            let response = app.oneshot(request(body)).await.unwrap();
            assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
            assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
            assert!(
                to_bytes(response.into_body(), 1024)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(start.elapsed(), READ_TIMEOUT);
        });
    assert!(polls.load(Ordering::SeqCst) >= 2);
    assert!(dropped.load(Ordering::SeqCst));
    assert!(accepted.lock().unwrap().is_empty());
}

#[test]
fn only_post_can_extract_a_password() {
    let (app, accepted) = fixture();
    for method in [
        Method::GET,
        Method::HEAD,
        Method::OPTIONS,
        Method::PUT,
        Method::DELETE,
    ] {
        let mut req = request(Body::from("password=fixture"));
        *req.method_mut() = method;
        reject(&app, req, StatusCode::METHOD_NOT_ALLOWED);
    }
    assert!(accepted.lock().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn fixture_form_connects_to_bounded_sign_in_without_changing_the_password() {
    use crate::{owner_auth::service::OwnerAuthService, owner_store::OwnerStore};
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let auth = OwnerAuthService::new(OwnerStore::open(directory.path()).unwrap());
    let password = "  café + fixture 🔑\0\n";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(auth.setup(password.into())).unwrap();
    let app = Router::new().route(
        "/form",
        any(move |form: PasswordForm| {
            let auth = auth.clone();
            async move {
                match auth.sign_in(form.into_password()).await.unwrap() {
                    Some(_) => StatusCode::NO_CONTENT,
                    None => StatusCode::UNAUTHORIZED,
                }
            }
        }),
    );
    assert_eq!(
        response(&app, request(Body::from(encoded(password)))).0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        response(&app, request(Body::from(encoded(password.trim())))).0,
        StatusCode::UNAUTHORIZED
    );
}
