use axum::{
    body::to_bytes,
    extract::Request,
    http::{StatusCode, header::CONTENT_TYPE},
};
use std::{error::Error as _, time::Duration};

pub(super) async fn text_body(request: Request, limit: usize) -> Result<String, StatusCode> {
    let mut media = request.headers().get_all(CONTENT_TYPE).iter();
    let accepted = media
        .next()
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("text/plain")
                || value.eq_ignore_ascii_case("text/plain;charset=UTF-8")
        });
    if !accepted || media.next().is_some() || request.headers().contains_key("content-encoding") {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let bytes = tokio::time::timeout(Duration::from_secs(5), to_bytes(request.into_body(), limit))
        .await
        .map_err(|_| StatusCode::REQUEST_TIMEOUT)?
        .map_err(|error| {
            if error
                .source()
                .is_some_and(|source| source.is::<http_body_util::LengthLimitError>())
            {
                StatusCode::PAYLOAD_TOO_LARGE
            } else {
                StatusCode::BAD_REQUEST
            }
        })?;
    String::from_utf8(bytes.to_vec()).map_err(|_| StatusCode::BAD_REQUEST)
}
