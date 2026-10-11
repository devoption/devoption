use super::*;
use axum::http::header::{AUTHORIZATION, CACHE_CONTROL, COOKIE};
use std::sync::{Arc, Barrier};

fn request(response: &Response) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        response.headers()[SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .parse()
            .unwrap(),
    );
    headers.insert(CSRF_HEADER, response.headers()[CSRF_HEADER].clone());
    headers
}

fn issue_at(store: &PreAuthCsrf, now: Instant, seed: u8) -> Response {
    let mut value = seed;
    store.issue_using(
        || now,
        |bytes| {
            bytes.fill(value);
            value += 1;
            Ok(())
        },
    )
}

#[test]
fn issuance_has_separate_secrets_sensitive_headers_and_no_cached_body() {
    let store = PreAuthCsrf::default();
    let first = store.issue();
    let second = store.issue();
    assert_eq!(first.status(), StatusCode::NO_CONTENT);
    assert_eq!(first.headers()[CACHE_CONTROL], "no-store");
    let cookie = &first.headers()[SET_COOKIE];
    let csrf = &first.headers()[CSRF_HEADER];
    assert!(cookie.is_sensitive());
    assert!(csrf.is_sensitive());
    assert_eq!(
        cookie.to_str().unwrap().split_once("; ").unwrap().1,
        "Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=300"
    );
    assert!(!cookie.to_str().unwrap().contains(csrf.to_str().unwrap()));
    assert_ne!(cookie, &second.headers()[SET_COOKIE]);
    assert_ne!(csrf, &second.headers()[CSRF_HEADER]);
    assert!(store.consume(&request(&first)));
    assert!(!store.consume(&request(&first)));
    assert!(store.consume(&request(&second)));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    assert!(
        runtime
            .block_on(axum::body::to_bytes(first.into_body(), 1))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn invalid_or_mixed_credentials_preserve_other_challenges() {
    let store = PreAuthCsrf::default();
    let first = request(&store.issue());
    let second = request(&store.issue());
    for name in [COOKIE.as_str(), CSRF_HEADER] {
        let mut missing = first.clone();
        missing.remove(name);
        assert!(!store.consume(&missing));
        let mut duplicate = first.clone();
        duplicate.append(name, duplicate[name].clone());
        assert!(!store.consume(&duplicate));
        let mut malformed = first.clone();
        malformed.insert(name, "malformed".parse().unwrap());
        assert!(!store.consume(&malformed));
        let mut mixed = first.clone();
        mixed.insert(name, second[name].clone());
        assert!(!store.consume(&mixed));
    }
    let mut bearer = first.clone();
    bearer.insert(AUTHORIZATION, "Bearer ignored".parse().unwrap());
    assert!(!store.consume(&bearer));
    let mut authenticated_cookie = first.clone();
    authenticated_cookie.insert(
        COOKIE,
        first[COOKIE]
            .to_str()
            .unwrap()
            .replace(COOKIE_NAME, "__Host-devoption_session")
            .parse()
            .unwrap(),
    );
    assert!(!store.consume(&authenticated_cookie));
    assert!(!PreAuthCsrf::default().consume(&first));
    assert!(store.consume(&first));
    assert!(store.consume(&second));
}

#[test]
fn absolute_expiry_reclaims_capacity_without_evicting_live_challenges() {
    let store = PreAuthCsrf::default();
    let now = Instant::now();
    let requests: Vec<_> = (0..MAX_PENDING)
        .map(|index| request(&issue_at(&store, now, (index * 2) as u8)))
        .collect();
    let full = store.issue_using(|| now, |_| panic!("capacity checked before entropy"));
    assert_eq!(full.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(full.headers()[CACHE_CONTROL], "no-store");
    assert!(!full.headers().contains_key(SET_COOKIE));
    assert!(!full.headers().contains_key(CSRF_HEADER));
    let expiry = now + LIFETIME;
    assert!(store.consume_using(&requests[0], || expiry - Duration::from_nanos(1)));
    let later = request(&issue_at(&store, expiry - Duration::from_secs(1), 100));
    assert!(!store.consume_using(&requests[1], || expiry));
    let fresh = request(&issue_at(&store, expiry, 102));
    assert_eq!(store.pending.lock().unwrap().len(), 2);
    assert!(store.consume_using(&later, || expiry));
    assert!(store.consume_using(&fresh, || expiry));
}

#[test]
fn entropy_failure_at_either_draw_and_lock_poison_fail_closed() {
    let store = Arc::new(PreAuthCsrf::default());
    let valid = request(&store.issue());
    for fail_at in [1, 2] {
        let mut calls = 0;
        let failed = store.issue_using(Instant::now, |bytes| {
            calls += 1;
            bytes.fill(42);
            if calls == fail_at {
                Err(getrandom::Error::UNSUPPORTED)
            } else {
                Ok(())
            }
        });
        assert_eq!(calls, fail_at);
        assert_eq!(failed.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(!failed.headers().contains_key(SET_COOKIE));
        assert!(!failed.headers().contains_key(CSRF_HEADER));
        assert_eq!(store.pending.lock().unwrap().len(), 1);
    }
    let poison = Arc::clone(&store);
    assert!(
        std::thread::spawn(move || {
            let _guard = poison.pending.lock().unwrap();
            panic!("fixture lock poison");
        })
        .join()
        .is_err()
    );
    assert_eq!(store.issue().status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!store.consume(&valid));
}

#[test]
fn time_is_sampled_inside_the_shared_lock() {
    let store = PreAuthCsrf::default();
    let now = || {
        assert!(matches!(
            store.pending.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        Instant::now()
    };
    let headers = request(&store.issue_using(now, getrandom::fill));
    assert!(store.consume_using(&headers, now));
}

#[test]
fn concurrent_replays_have_exactly_one_winner() {
    let store = Arc::new(PreAuthCsrf::default());
    let headers = request(&store.issue());
    let start = Arc::new(Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let store = Arc::clone(&store);
            let start = Arc::clone(&start);
            let headers = headers.clone();
            std::thread::spawn(move || {
                start.wait();
                store.consume(&headers)
            })
        })
        .collect();
    assert_eq!(
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|accepted| *accepted)
            .count(),
        1
    );
}
