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
        assert_eq!(service.state.store.load().unwrap().unwrap().identity(), identity);
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
