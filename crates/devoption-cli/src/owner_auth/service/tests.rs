use super::*;
use std::{fs, os::unix::fs::PermissionsExt, sync::mpsc, thread};
use tokio::{
    runtime::{Builder, Runtime},
    sync::oneshot,
};

const PASSWORD: &str = "bounded authentication fixture only";

fn fixture() -> (tempfile::TempDir, OwnerAuthService) {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let store = OwnerStore::open(directory.path()).unwrap();
    (directory, OwnerAuthService::new(store))
}

fn runtime() -> Runtime {
    Builder::new_current_thread().enable_all().build().unwrap()
}

fn assert_limited(service: &OwnerAuthService, now: Instant, expected: Duration) {
    match service.admit_at(now) {
        Err(ServiceError::RateLimited { retry_after }) => assert_eq!(retry_after, expected),
        _ => panic!("expected the shared attempt limit"),
    }
}

#[test]
fn rolling_window_expires_each_attempt_at_its_exact_boundary() {
    let (_directory, service) = fixture();
    let clone = service.clone();
    let now = Instant::now();
    for seconds in 0..MAX_ATTEMPTS as u64 {
        drop(
            service
                .admit_at(now + Duration::from_secs(seconds))
                .unwrap(),
        );
    }
    assert_limited(
        &clone,
        now + Duration::from_secs(59),
        Duration::from_secs(1),
    );
    for _ in 0..20 {
        assert_limited(
            &clone,
            now + Duration::from_millis(59_500),
            Duration::from_millis(500),
        );
    }
    assert_eq!(service.state.attempts.lock().unwrap().len(), MAX_ATTEMPTS);
    drop(clone.admit_at(now + ATTEMPT_WINDOW).unwrap());
    assert_limited(&service, now + ATTEMPT_WINDOW, Duration::from_secs(1));
    drop(service.admit_at(now + ATTEMPT_WINDOW * 3).unwrap());
    assert_eq!(service.state.attempts.lock().unwrap().len(), 1);
}

#[test]
fn busy_clones_do_not_queue_workers_or_consume_attempts() {
    let (_directory, service) = fixture();
    let now = Instant::now();
    let active = service.admit_at(now).unwrap();
    for _ in 0..20 {
        assert!(matches!(
            service.clone().admit_at(now),
            Err(ServiceError::Busy)
        ));
    }
    assert_eq!(service.state.attempts.lock().unwrap().len(), 1);
    drop(active);
    drop(service.clone().admit_at(now).unwrap());
    assert_eq!(service.state.attempts.lock().unwrap().len(), 2);
}

#[test]
fn setup_and_sign_in_share_the_limit_including_successes_and_failures() {
    let (_directory, service) = fixture();
    runtime().block_on(async {
        let identity = service.setup(PASSWORD.into()).await.unwrap();
        let clone = service.clone();
        assert_eq!(clone.authenticate(PASSWORD.into()).await.unwrap(), Some(identity));
        assert_eq!(service.authenticate("wrong fixture".into()).await.unwrap(), None);
        assert!(matches!(
            clone.setup("replacement fixture".into()).await,
            Err(ServiceError::Setup(SetupError::Store(StoreError::AlreadyExists)))
        ));
        assert_eq!(service.authenticate(String::new()).await.unwrap(), None);
        assert!(matches!(
            clone.authenticate(PASSWORD.into()).await,
            Err(ServiceError::RateLimited { retry_after }) if !retry_after.is_zero() && retry_after <= ATTEMPT_WINDOW
        ));
        assert!(matches!(
            clone.sign_in(PASSWORD.into()).await,
            Err(ServiceError::RateLimited { .. })
        ));
        assert_eq!(service.state.store.load().unwrap().unwrap().identity(), identity);
    });
}

#[test]
fn public_sign_in_issues_a_revocable_session_for_the_persisted_identity() {
    let (directory, service) = fixture();
    runtime().block_on(async {
        let identity = service.setup(PASSWORD.into()).await.unwrap();
        let original = fs::read(directory.path().join("owner.bin")).unwrap();
        assert!(
            service
                .sign_in("wrong fixture".into())
                .await
                .unwrap()
                .is_none()
        );
        let issued = service.sign_in(PASSWORD.into()).await.unwrap().unwrap();
        let (mut session, token) = issued.into_parts();
        let mut wrong = token;
        wrong[0] ^= 1;
        assert_eq!(session.authenticate(None), None);
        assert_eq!(session.authenticate(Some(&wrong)), None);
        assert_eq!(session.authenticate(Some(&token)), Some(identity));
        session.revoke();
        assert_eq!(session.authenticate(Some(&token)), None);
        assert_eq!(service.state.attempts.lock().unwrap().len(), 3);
        assert_eq!(
            fs::read(directory.path().join("owner.bin")).unwrap(),
            original
        );
    });
}

#[test]
fn invalid_grant_returns_no_credential_and_preserves_the_active_session() {
    let (_directory, service) = fixture();
    runtime().block_on(async {
        let identity = service.setup(PASSWORD.into()).await.unwrap();
        let slot = SessionSlot::default();
        let first = service
            .sign_in(PASSWORD.into())
            .await
            .unwrap()
            .unwrap()
            .activate(&slot)
            .unwrap();
        let mut invalid = service.sign_in(PASSWORD.into()).await.unwrap().unwrap();
        invalid.session.revoke();
        assert_eq!(
            invalid.activate(&slot),
            Err(ActivationError::InvalidSession)
        );
        assert_eq!(slot.authenticate(Some(&first)), Some(identity));
    });
}

#[test]
fn rejected_sign_in_never_requests_session_entropy() {
    let (directory, service) = fixture();
    let store = &service.state.store;
    assert!(
        sign_in_with_entropy(store, PASSWORD, |_| panic!(
            "absent owner requested entropy"
        ))
        .unwrap()
        .is_none()
    );
    owner_auth::setup(store, PASSWORD).unwrap();
    for wrong in [
        "wrong fixture".to_owned(),
        String::new(),
        "x".repeat(crate::password::MAX_PASSWORD_BYTES + 1),
    ] {
        assert!(
            sign_in_with_entropy(store, &wrong, |_| panic!(
                "rejected password requested entropy"
            ))
            .unwrap()
            .is_none()
        );
    }
    let record = directory.path().join("owner.bin");
    fs::write(&record, b"corrupt fixture").unwrap();
    assert!(matches!(
        sign_in_with_entropy(store, PASSWORD, |_| panic!(
            "corrupt store requested entropy"
        )),
        Err(ServiceError::Store(StoreError::InvalidRecord))
    ));
    assert_eq!(fs::read(record).unwrap(), b"corrupt fixture");
}

#[test]
fn each_sign_in_requests_independent_session_and_csrf_credentials() {
    let (_directory, service) = fixture();
    let store = &service.state.store;
    let identity = owner_auth::setup(store, PASSWORD).unwrap();
    let mut calls = 0;
    let mut sessions = Vec::new();
    for value in [0x35, 0xa7] {
        let mut draws = 0;
        let issued = sign_in_with_entropy(store, PASSWORD, |bytes| {
            calls += 1;
            draws += 1;
            assert_eq!(bytes.len(), TOKEN_BYTES);
            bytes.fill(if draws == 1 { value } else { value ^ 0xff });
            Ok(())
        })
        .unwrap()
        .unwrap();
        assert_eq!(issued.csrf_token, [value ^ 0xff; CSRF_TOKEN_BYTES]);
        let csrf = issued.csrf_token;
        let (mut session, token) = issued.into_parts();
        assert_eq!(token, [value; TOKEN_BYTES]);
        assert_eq!(
            session.authenticate_browser(Some(&token), Some(&csrf)),
            Some(identity)
        );
        assert_eq!(
            session.authenticate_browser(Some(&token), Some(&token)),
            None
        );
        sessions.push((session, token));
    }
    assert_eq!(calls, 4);
    let [(mut first, first_token), (mut second, second_token)]: [_; 2] =
        sessions.try_into().ok().unwrap();
    assert_eq!(first.authenticate(Some(&first_token)), Some(identity));
    assert_eq!(second.authenticate(Some(&second_token)), Some(identity));
    assert_eq!(first.authenticate(Some(&second_token)), None);
    assert_eq!(second.authenticate(Some(&first_token)), None);
}

#[test]
fn either_token_entropy_failure_does_not_return_a_grant_or_change_the_owner() {
    let (directory, service) = fixture();
    let store = &service.state.store;
    let identity = owner_auth::setup(store, PASSWORD).unwrap();
    let original = fs::read(directory.path().join("owner.bin")).unwrap();
    for fail_on in [1, 2] {
        let mut calls = 0;
        assert!(matches!(
            sign_in_with_entropy(store, PASSWORD, |bytes| {
                calls += 1;
                if calls == fail_on {
                    bytes[..8].fill(0x5a); // Partial entropy must never become a credential.
                    Err(ServiceError::Unavailable)
                } else {
                    bytes.fill(0x35);
                    Ok(())
                }
            }),
            Err(ServiceError::Unavailable)
        ));
        assert_eq!(calls, fail_on);
    }
    assert_eq!(
        fs::read(directory.path().join("owner.bin")).unwrap(),
        original
    );
    assert_eq!(
        owner_auth::authenticate(store, PASSWORD).unwrap(),
        Some(identity)
    );
}

#[test]
fn browser_grants_activate_rotate_and_logout_with_fixture_owner_data_only() {
    let (directory, service) = fixture();
    runtime().block_on(async {
        let identity = service.setup(PASSWORD.into()).await.unwrap();
        let original = fs::read(directory.path().join("owner.bin")).unwrap();
        let slot = SessionSlot::default();
        let first = service
            .sign_in(PASSWORD.into())
            .await
            .unwrap()
            .unwrap()
            .activate_browser(&slot)
            .unwrap();
        assert_ne!(first.session_token(), first.csrf_token());
        assert_eq!(
            slot.authenticate_browser(Some(first.session_token()), Some(first.csrf_token())),
            Some(identity)
        );
        assert!(
            service
                .sign_in("wrong fixture".into())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            slot.authenticate_browser(Some(first.session_token()), Some(first.csrf_token())),
            Some(identity)
        );
        let second = service
            .sign_in(PASSWORD.into())
            .await
            .unwrap()
            .unwrap()
            .activate_browser(&slot)
            .unwrap();
        assert_ne!(first.session_token(), second.session_token());
        assert_ne!(first.csrf_token(), second.csrf_token());
        assert_ne!(second.session_token(), second.csrf_token());
        assert_eq!(
            slot.authenticate_browser(Some(first.session_token()), Some(first.csrf_token())),
            None
        );
        assert_eq!(
            slot.authenticate_browser(Some(second.session_token()), Some(first.csrf_token())),
            None
        );
        assert_eq!(
            slot.authenticate_browser(Some(first.session_token()), Some(second.csrf_token())),
            None
        );
        let mut invalid = service.sign_in(PASSWORD.into()).await.unwrap().unwrap();
        invalid.session.revoke();
        assert!(matches!(
            invalid.activate_browser(&slot),
            Err(ActivationError::InvalidSession)
        ));
        assert_eq!(
            slot.authenticate_browser(Some(second.session_token()), Some(second.csrf_token())),
            Some(identity)
        );
        assert!(!slot.logout_browser(Some(first.session_token()), Some(first.csrf_token())));
        assert!(slot.logout_browser(Some(second.session_token()), Some(second.csrf_token())));
        assert_eq!(
            slot.authenticate_browser(Some(second.session_token()), Some(second.csrf_token())),
            None
        );
        assert_eq!(service.state.attempts.lock().unwrap().len(), 5);
        assert_eq!(
            fs::read(directory.path().join("owner.bin")).unwrap(),
            original
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    });
}

#[test]
fn cancelled_sign_in_discards_the_undelivered_grant_and_keeps_its_attempt() {
    struct ObservedGrant {
        _grant: IssuedSession,
        dropped: Option<oneshot::Sender<()>>,
    }
    impl Drop for ObservedGrant {
        fn drop(&mut self) {
            let _ = self.dropped.take().unwrap().send(());
        }
    }
    let (directory, service) = fixture();
    owner_auth::setup(&service.state.store, PASSWORD).unwrap();
    let original = fs::read(directory.path().join("owner.bin")).unwrap();
    runtime().block_on(async {
        let (started, worker_started) = oneshot::channel();
        let (release, worker_release) = mpsc::channel();
        let (dropped, grant_dropped) = oneshot::channel();
        let clone = service.clone();
        let caller = tokio::spawn(async move {
            clone
                .run(move |store| {
                    started.send(()).unwrap();
                    worker_release
                        .recv_timeout(Duration::from_secs(10))
                        .unwrap();
                    let grant = sign_in_with_entropy(store, PASSWORD, |bytes| {
                        bytes.fill(0x5a);
                        Ok(())
                    })?
                    .unwrap();
                    Ok(ObservedGrant {
                        _grant: grant,
                        dropped: Some(dropped),
                    })
                })
                .await
        });
        worker_started.await.unwrap();
        caller.abort();
        assert!(matches!(caller.await, Err(error) if error.is_cancelled()));
        assert!(matches!(
            service.sign_in(PASSWORD.into()).await,
            Err(ServiceError::Busy)
        ));
        release.send(()).unwrap();
        grant_dropped.await.unwrap();
        assert_eq!(service.state.attempts.lock().unwrap().len(), 1);
        assert_eq!(
            fs::read(directory.path().join("owner.bin")).unwrap(),
            original
        );
    });
}

#[test]
fn missing_and_corrupt_storage_fail_without_resetting_history() {
    let (directory, service) = fixture();
    runtime().block_on(async {
        assert_eq!(service.authenticate(PASSWORD.into()).await.unwrap(), None);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
        let record = directory.path().join("owner.bin");
        fs::write(&record, b"broken fixture").unwrap();
        fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            service.authenticate(PASSWORD.into()).await,
            Err(ServiceError::Store(StoreError::InvalidRecord))
        ));
        assert!(matches!(
            service.setup(PASSWORD.into()).await,
            Err(ServiceError::Setup(SetupError::Store(
                StoreError::InvalidRecord
            )))
        ));
        assert_eq!(service.state.attempts.lock().unwrap().len(), 3);
        assert_eq!(service.state.worker.available_permits(), 1);
        assert_eq!(fs::read(record).unwrap(), b"broken fixture");
    });
}

#[test]
fn cancelling_the_caller_keeps_work_off_the_runtime_and_holds_its_slot() {
    let (_directory, service) = fixture();
    runtime().block_on(async {
        let runtime_thread = thread::current().id();
        let (started, worker_started) = oneshot::channel();
        let (release, worker_release) = mpsc::channel();
        let clone = service.clone();
        let caller = tokio::spawn(async move {
            clone
                .run(move |store| {
                    started.send(thread::current().id()).unwrap();
                    // A failed assertion drops the sender; this timeout also bounds regressions.
                    worker_release
                        .recv_timeout(Duration::from_secs(10))
                        .unwrap();
                    owner_auth::setup(store, PASSWORD).map_err(ServiceError::Setup)
                })
                .await
        });
        assert_ne!(worker_started.await.unwrap(), runtime_thread);
        // The single runtime thread can continue while the blocking worker waits.
        assert!(matches!(
            service.authenticate(PASSWORD.into()).await,
            Err(ServiceError::Busy)
        ));
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        assert!(matches!(
            service.setup(PASSWORD.into()).await,
            Err(ServiceError::Busy)
        ));
        assert_eq!(service.state.attempts.lock().unwrap().len(), 1);

        release.send(()).unwrap();
        // Wait for actual worker cleanup, not just cancellation of its caller.
        drop(
            Arc::clone(&service.state.worker)
                .acquire_owned()
                .await
                .unwrap(),
        );
        let identity = service.state.store.load().unwrap().unwrap().identity();
        assert_eq!(
            service.authenticate(PASSWORD.into()).await.unwrap(),
            Some(identity)
        );
        assert_eq!(service.state.attempts.lock().unwrap().len(), 2);
    });
}

#[test]
fn worker_panics_release_capacity_but_keep_the_attempt_charged() {
    let (_directory, service) = fixture();
    runtime().block_on(async {
        let error = service
            .run::<()>(|_| panic!("fixture worker failure detail"))
            .await
            .unwrap_err();
        assert!(matches!(error, ServiceError::Unavailable));
        for message in [error.to_string(), format!("{error:?}")] {
            assert!(!message.contains("fixture worker failure detail"));
        }
        assert_eq!(service.state.worker.available_permits(), 1);
        assert_eq!(service.state.attempts.lock().unwrap().len(), 1);
        assert_eq!(service.authenticate(PASSWORD.into()).await.unwrap(), None);
        assert_eq!(service.state.attempts.lock().unwrap().len(), 2);
    });
}

#[test]
fn poisoned_admission_state_fails_closed_without_starting_setup() {
    let (directory, service) = fixture();
    let state = Arc::clone(&service.state);
    assert!(
        thread::spawn(move || {
            let _guard = state.attempts.lock().unwrap();
            panic!("fixture admission failure");
        })
        .join()
        .is_err()
    );
    runtime().block_on(async {
        assert!(matches!(
            service.setup(PASSWORD.into()).await,
            Err(ServiceError::Unavailable)
        ));
        assert!(matches!(
            service.authenticate(PASSWORD.into()).await,
            Err(ServiceError::Unavailable)
        ));
    });
    assert_eq!(service.state.worker.available_permits(), 1);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}
