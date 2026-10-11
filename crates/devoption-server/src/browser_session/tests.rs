use super::*;
use crate::{owner_auth::service::OwnerAuthService, owner_store::OwnerStore};
use std::{fs, os::unix::fs::PermissionsExt};

fn headers(cookie: &str, csrf: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(COOKIE, cookie.parse().unwrap());
    headers.insert(CSRF_HEADER, csrf.parse().unwrap());
    headers
}

#[test]
fn cookie_parser_accepts_only_one_unambiguous_session_and_explicit_csrf_header() {
    let token = "ab".repeat(TOKEN_BYTES);
    let csrf = "cd".repeat(CSRF_TOKEN_BYTES);
    let cookie = format!("{COOKIE_NAME}={token}");
    for value in [
        &cookie,
        &format!("other=ok; {cookie}; empty=; quoted=\"ok\""),
        &format!("{COOKIE_NAME}={}", token.to_uppercase()),
    ] {
        assert_eq!(
            credentials(&headers(value, &csrf)),
            Some(([0xab; TOKEN_BYTES], [0xcd; CSRF_TOKEN_BYTES]))
        );
    }
    for invalid in [
        String::new(),
        format!("session={token}"),
        format!("{COOKIE_NAME}="),
        format!("{cookie}; {cookie}"),
        format!("{cookie}, {cookie}"),
        format!("{cookie};"),
        format!("bad name=x; {cookie}"),
        format!("other=bad,value; {cookie}"),
        format!("other=bad\\value; {cookie}"),
        format!("other; {cookie}"),
        format!("{COOKIE_NAME}=\"{token}\""),
        format!("{COOKIE_NAME}=%61{}", &token[1..]),
        format!("{COOKIE_NAME}={}", "g".repeat(64)),
        format!("{COOKIE_NAME}={}", "a".repeat(63)),
        format!("{COOKIE_NAME}={}", "a".repeat(65)),
        format!("{COOKIE_NAME}={}", "é".repeat(32)),
        format!("other={}; {cookie}", "x".repeat(MAX_COOKIE_BYTES)),
    ] {
        assert!(credentials(&headers(&invalid, &csrf)).is_none());
    }
    for invalid in [
        String::new(),
        "c".repeat(63),
        "c".repeat(65),
        "z".repeat(64),
        format!("{csrf}, {csrf}"),
        format!(" {csrf}"),
        format!("{csrf} "),
    ] {
        assert!(credentials(&headers(&cookie, &invalid)).is_none());
    }
    for name in [COOKIE.as_str(), CSRF_HEADER] {
        let mut request = headers(&cookie, &csrf);
        request.append(name, request[name].clone());
        assert!(credentials(&request).is_none());
        request.remove(name);
        assert!(credentials(&request).is_none());
        request.insert(name, HeaderValue::from_bytes(b"\xff").unwrap());
        assert!(credentials(&request).is_none());
    }
    let mut mixed = headers(&cookie, &csrf);
    mixed.insert(AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
    assert!(credentials(&mixed).is_none());
}

#[test]
fn cookie_header_limit_counts_all_cookies_including_unrelated_values() {
    let cookie = format!("{COOKIE_NAME}={}; other=", "ab".repeat(TOKEN_BYTES));
    let exact = format!("{cookie}{}", "x".repeat(MAX_COOKIE_BYTES - cookie.len()));
    let csrf = "cd".repeat(CSRF_TOKEN_BYTES);
    assert!(credentials(&headers(&exact, &csrf)).is_some());
    assert!(credentials(&headers(&format!("{exact}x"), &csrf)).is_none());
}

#[test]
fn response_helpers_keep_tokens_separate_and_clear_only_a_successful_logout() {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let auth = OwnerAuthService::new(OwnerStore::open(directory.path()).unwrap());
    let slot = SessionSlot::default();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let password = "cookie transport fixture only";
        let identity = auth.setup(password.into()).await.unwrap();
        let original = fs::read(directory.path().join("owner.bin")).unwrap();
        let mut previous = None;
        for _ in 0..2 {
            let grant = auth.sign_in(password.into()).await.unwrap().unwrap();
            let response = activate(grant, &slot).unwrap();
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
            let (parts, body) = response.into_parts();
            assert!(axum::body::to_bytes(body, 1024).await.unwrap().is_empty());
            assert_eq!(parts.headers[CACHE_CONTROL], "no-store");
            let set_cookie = &parts.headers[SET_COOKIE];
            let csrf = &parts.headers[CSRF_HEADER];
            assert!(set_cookie.is_sensitive());
            assert!(csrf.is_sensitive());
            let (cookie, attributes) = set_cookie.to_str().unwrap().split_once("; ").unwrap();
            assert_eq!(attributes, COOKIE_ATTRIBUTES);
            assert!(
                !set_cookie
                    .to_str()
                    .unwrap()
                    .contains(csrf.to_str().unwrap())
            );
            let current = headers(cookie, csrf.to_str().unwrap());
            assert_eq!(authenticate(&current, &slot), Some(identity));
            if let Some(stale) = previous {
                assert_eq!(authenticate(&stale, &slot), None);
                let rejected = logout(&stale, &slot);
                assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
                assert_eq!(rejected.headers()[CACHE_CONTROL], "no-store");
                assert!(!rejected.headers().contains_key(SET_COOKIE));
                assert!(
                    axum::body::to_bytes(rejected.into_body(), 1024)
                        .await
                        .unwrap()
                        .is_empty()
                );
                assert_eq!(authenticate(&current, &slot), Some(identity));
            }
            previous = Some(current);
        }
        let current = previous.unwrap();
        let response = logout(&current, &slot);
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        assert_eq!(
            response.headers()[SET_COOKIE],
            format!("{COOKIE_NAME}=; {COOKIE_ATTRIBUTES}; Max-Age=0")
        );
        assert!(!response.headers().contains_key(CSRF_HEADER));
        assert!(
            axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(authenticate(&current, &slot), None);
        assert!(!logout(&current, &slot).headers().contains_key(SET_COOKIE));
        assert_eq!(
            fs::read(directory.path().join("owner.bin")).unwrap(),
            original
        );
    });
}
