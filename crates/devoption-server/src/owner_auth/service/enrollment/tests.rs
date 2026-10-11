use super::*;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{Barrier, mpsc},
};
use tokio::runtime::{Builder, Runtime};

const PASSWORD: &str = "local enrollment fixture only";

fn runtime() -> Runtime {
    Builder::new_current_thread().enable_all().build().unwrap()
}

fn fixture() -> (tempfile::TempDir, OwnerAuthService) {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let auth = OwnerAuthService::new(OwnerStore::open(directory.path()).unwrap());
    (directory, auth)
}

fn kinds(attempt: &EnrollmentAttempt) -> Vec<AuditKind> {
    attempt
        .audit()
        .unwrap()
        .events
        .iter()
        .map(|event| event.kind)
        .collect()
}

#[test]
fn local_attempt_binds_one_browser_pair_to_its_own_store_and_never_a_session() {
    let (_directory, auth) = fixture();
    let (_other_directory, other_auth) = fixture();
    runtime().block_on(async {
        let (attempt, launch) = EnrollmentAttempt::begin_local(auth.clone()).await.unwrap();
        let (other, other_launch) = EnrollmentAttempt::begin_local(other_auth.clone())
            .await
            .unwrap();
        assert!(matches!(
            attempt.claim(other_launch.as_bytes()).await,
            Err(EnrollmentError::Denied)
        ));
        assert!(matches!(
            attempt
                .claim(&attempt.audit().unwrap().attempt_id.to_be_bytes())
                .await,
            Err(EnrollmentError::Denied)
        ));
        let browser = attempt.claim(launch.as_bytes()).await.unwrap();
        let other_browser = other.claim(other_launch.as_bytes()).await.unwrap();
        assert_ne!(launch.as_bytes(), browser.cookie());
        assert_ne!(browser.cookie(), browser.csrf());
        assert!(matches!(
            attempt.claim(launch.as_bytes()).await,
            Err(EnrollmentError::Denied)
        ));
        for (cookie, csrf) in [
            (other_browser.cookie().as_slice(), browser.csrf().as_slice()),
            (browser.cookie().as_slice(), other_browser.csrf().as_slice()),
            (&[][..], browser.csrf().as_slice()),
        ] {
            assert_eq!(
                attempt.enroll(cookie, csrf, PASSWORD.into()).await,
                Err(EnrollmentError::Denied)
            );
        }
        let identity = attempt
            .enroll(browser.cookie(), browser.csrf(), PASSWORD.into())
            .await
            .unwrap();
        assert_eq!(
            auth.state.store.load().unwrap().unwrap().identity(),
            identity
        );
        assert!(other_auth.state.store.load().unwrap().is_none());
        assert_eq!(
            attempt
                .enroll(
                    browser.cookie(),
                    browser.csrf(),
                    "replacement fixture".into()
                )
                .await,
            Err(EnrollmentError::Denied)
        );
        assert_eq!(
            auth.authenticate(PASSWORD.into()).await.unwrap(),
            Some(identity)
        );
        assert!(kinds(&attempt).contains(&AuditKind::OwnerCreated));
    });
}

#[test]
fn expiry_is_shared_by_claim_and_submission_without_renewal() {
    for after_claim in [false, true] {
        let (_directory, auth) = fixture();
        runtime().block_on(async {
            let (attempt, launch) = EnrollmentAttempt::begin_local(auth.clone()).await.unwrap();
            let deadline = attempt.state.lock().unwrap().expires_at;
            let browser = if after_claim {
                Some(attempt.claim(launch.as_bytes()).await.unwrap())
            } else {
                None
            };
            assert_eq!(attempt.state.lock().unwrap().expires_at, deadline);
            attempt.state.lock().unwrap().expires_at = Instant::now();
            if let Some(browser) = browser {
                assert_eq!(
                    attempt
                        .enroll(browser.cookie(), browser.csrf(), PASSWORD.into())
                        .await,
                    Err(EnrollmentError::Expired)
                );
            } else {
                assert!(matches!(
                    attempt.claim(launch.as_bytes()).await,
                    Err(EnrollmentError::Expired)
                ));
            }
            assert!(matches!(
                attempt.claim(launch.as_bytes()).await,
                Err(EnrollmentError::Denied)
            ));
            assert!(auth.state.store.load().unwrap().is_none());
            assert!(kinds(&attempt).contains(&AuditKind::Expired));
        });
    }
}

#[test]
fn owner_present_or_corrupt_at_every_stage_fails_closed_without_replacement() {
    for stage in 0..3 {
        for corrupt in [false, true] {
            let (directory, auth) = fixture();
            runtime().block_on(async {
                let attempt = if stage > 0 {
                    Some(EnrollmentAttempt::begin_local(auth.clone()).await.unwrap())
                } else {
                    None
                };
                let browser = if stage == 2 {
                    let (attempt, launch) = attempt.as_ref().unwrap();
                    Some(attempt.claim(launch.as_bytes()).await.unwrap())
                } else {
                    None
                };
                owner_auth::setup(&auth.state.store, PASSWORD).unwrap();
                let path = directory.path().join("owner.bin");
                if corrupt { fs::write(&path, b"private invalid fixture record").unwrap(); }
                let original = fs::read(&path).unwrap();
                let expected = if corrupt { EnrollmentError::Unavailable } else { EnrollmentError::AlreadyConfigured };
                match attempt {
                    None => {
                        let result = EnrollmentAttempt::begin_local(auth.clone()).await;
                        assert!(matches!(result, Err(error) if error == expected));
                    }
                    Some((attempt, launch)) => {
                        if let Some(browser) = browser {
                            let result = attempt.enroll(
                                browser.cookie(), browser.csrf(), "replacement fixture".into()
                            ).await;
                            assert_eq!(result, Err(expected));
                        } else {
                            assert!(matches!(attempt.claim(launch.as_bytes()).await, Err(error) if error == expected));
                        }
                        assert!(matches!(attempt.claim(launch.as_bytes()).await, Err(EnrollmentError::Denied)));
                    }
                }
                assert_eq!(fs::read(path).unwrap(), original);
                assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
            });
        }
    }
}

#[test]
fn entropy_failure_never_issues_a_grant_or_reopens_a_consumed_launch() {
    let (_directory, auth) = fixture();
    runtime().block_on(async {
        assert!(matches!(
            EnrollmentAttempt::begin_using(auth.clone(), |_| Err(getrandom::Error::UNSUPPORTED))
                .await,
            Err(EnrollmentError::Unavailable)
        ));
        assert!(auth.state.store.load().unwrap().is_none());
    });
    for fail_at in [1, 2] {
        let (_directory, auth) = fixture();
        runtime().block_on(async {
            let (attempt, launch) = EnrollmentAttempt::begin_local(auth.clone()).await.unwrap();
            let mut draws = 0;
            assert!(matches!(
                attempt
                    .claim_using(launch.as_bytes(), move |bytes| {
                        draws += 1;
                        bytes.fill(42);
                        if draws == fail_at {
                            Err(getrandom::Error::UNSUPPORTED)
                        } else {
                            Ok(())
                        }
                    })
                    .await,
                Err(EnrollmentError::Unavailable)
            ));
            assert!(matches!(
                attempt.claim(launch.as_bytes()).await,
                Err(EnrollmentError::Denied)
            ));
            assert!(auth.state.store.load().unwrap().is_none());
            assert_eq!(
                kinds(&attempt)
                    .iter()
                    .filter(|kind| **kind == AuditKind::BrowserBound)
                    .count(),
                0
            );
        });
    }
}

#[test]
fn concurrent_claims_and_enrollments_each_have_one_winner() {
    let (_directory, auth) = fixture();
    let (attempt, launch) = runtime()
        .block_on(EnrollmentAttempt::begin_local(auth.clone()))
        .unwrap();
    let start = Arc::new(Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let attempt = attempt.clone();
            let launch = *launch.as_bytes();
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                runtime().block_on(attempt.claim(&launch))
            })
        })
        .collect();
    let browsers: Vec<_> = workers
        .into_iter()
        .filter_map(|worker| worker.join().unwrap().ok())
        .collect();
    assert_eq!(browsers.len(), 1);
    let browser = &browsers[0];
    let start = Arc::new(Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let attempt = attempt.clone();
            let cookie = *browser.cookie();
            let csrf = *browser.csrf();
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                runtime().block_on(attempt.enroll(&cookie, &csrf, PASSWORD.into()))
            })
        })
        .collect();
    let identities: Vec<_> = workers
        .into_iter()
        .filter_map(|worker| worker.join().unwrap().ok())
        .collect();
    assert_eq!(
        identities,
        [auth.state.store.load().unwrap().unwrap().identity()]
    );
    assert_eq!(
        kinds(&attempt)
            .iter()
            .filter(|kind| **kind == AuditKind::OwnerCreated)
            .count(),
        1
    );
}

#[test]
fn invalid_input_and_admission_failure_consume_browser_authority() {
    for failure in ["input", "busy", "limited"] {
        let (_directory, auth) = fixture();
        runtime().block_on(async {
            let (attempt, launch) = EnrollmentAttempt::begin_local(auth.clone()).await.unwrap();
            let browser = attempt.claim(launch.as_bytes()).await.unwrap();
            let permit = if failure == "busy" {
                Some(auth.admit_at(Instant::now()).unwrap())
            } else {
                None
            };
            if failure == "limited" {
                for _ in 0..3 {
                    drop(auth.admit_at(Instant::now()).unwrap());
                }
            }
            let password = if failure == "input" {
                String::new()
            } else {
                PASSWORD.into()
            };
            let rejected = attempt
                .enroll(browser.cookie(), browser.csrf(), password)
                .await;
            assert!(matches!(
                (failure, rejected),
                ("input", Err(EnrollmentError::InvalidPassword))
                    | ("busy", Err(EnrollmentError::Busy))
                    | ("limited", Err(EnrollmentError::RateLimited { .. }))
            ));
            drop(permit);
            assert_eq!(
                attempt
                    .enroll(browser.cookie(), browser.csrf(), PASSWORD.into())
                    .await,
                Err(EnrollmentError::Denied)
            );
            assert!(auth.state.store.load().unwrap().is_none());
        });
    }
}

#[test]
fn cancellation_retains_the_worker_and_records_completion_without_reopening() {
    let (_directory, auth) = fixture();
    runtime().block_on(async {
        let (attempt, launch) = EnrollmentAttempt::begin_local(auth.clone()).await.unwrap();
        let browser = attempt.claim(launch.as_bytes()).await.unwrap();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, blocked) = mpsc::channel();
        let worker_attempt = attempt.clone();
        let cookie = *browser.cookie();
        let csrf = *browser.csrf();
        let task = tokio::spawn(async move {
            worker_attempt
                .enroll_using(&cookie, &csrf, PASSWORD.into(), move |store, password| {
                    started.send(()).unwrap();
                    blocked.recv().unwrap();
                    owner_auth::setup(store, password)
                })
                .await
        });
        ready.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(
            attempt
                .enroll(browser.cookie(), browser.csrf(), PASSWORD.into())
                .await,
            Err(EnrollmentError::Denied)
        );
        assert!(matches!(
            auth.authenticate(PASSWORD.into()).await,
            Err(ServiceError::Busy)
        ));
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !kinds(&attempt).contains(&AuditKind::OwnerCreated) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(auth.state.store.load().unwrap().is_some());
    });
}

#[test]
fn claim_rechecks_expiry_after_blocking_work_without_reviving_the_launch() {
    let (_directory, auth) = fixture();
    runtime().block_on(async {
        let (attempt, launch) = EnrollmentAttempt::begin_local(auth.clone()).await.unwrap();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, blocked) = mpsc::channel();
        let worker = attempt.clone();
        let task = tokio::spawn(async move {
            let mut started = Some(started);
            worker
                .claim_using(launch.as_bytes(), move |bytes| {
                    if let Some(started) = started.take() {
                        started.send(()).unwrap();
                        blocked.recv().unwrap();
                    }
                    bytes.fill(42);
                    Ok(())
                })
                .await
        });
        ready.await.unwrap();
        attempt.state.lock().unwrap().expires_at = Instant::now();
        release.send(()).unwrap();
        assert!(matches!(task.await.unwrap(), Err(EnrollmentError::Expired)));
        assert!(matches!(
            attempt.claim(&[42; 32]).await,
            Err(EnrollmentError::Denied)
        ));
        assert!(!kinds(&attempt).contains(&AuditKind::BrowserBound));
        assert!(auth.state.store.load().unwrap().is_none());
    });
}

#[test]
fn failure_after_publication_is_audited_as_uncertain_and_never_resets_storage() {
    let (_directory, auth) = fixture();
    runtime().block_on(async {
        let (attempt, launch) = EnrollmentAttempt::begin_local(auth.clone()).await.unwrap();
        let browser = attempt.claim(launch.as_bytes()).await.unwrap();
        let result = attempt
            .enroll_using(
                browser.cookie(),
                browser.csrf(),
                PASSWORD.into(),
                |store, password| {
                    owner_auth::setup(store, password)?;
                    // Model the store's documented possibility of an I/O error after
                    // create-once publication (for example a directory-sync failure).
                    Err(owner_auth::SetupError::Store(
                        crate::owner_store::StoreError::Io(std::io::Error::other(
                            "fixture post-publication failure",
                        )),
                    ))
                },
            )
            .await;
        assert_eq!(result, Err(EnrollmentError::Unavailable));
        assert_eq!(
            kinds(&attempt).last(),
            Some(&AuditKind::CompletionUncertain)
        );
        let identity = auth.state.store.load().unwrap().unwrap().identity();
        assert_eq!(
            attempt
                .enroll(
                    browser.cookie(),
                    browser.csrf(),
                    "replacement fixture".into()
                )
                .await,
            Err(EnrollmentError::Denied)
        );
        assert_eq!(
            auth.state.store.load().unwrap().unwrap().identity(),
            identity
        );
    });
}

#[test]
fn audit_is_bounded_reports_loss_and_contains_no_secret_inputs() {
    let (directory, auth) = fixture();
    runtime().block_on(async {
        let (attempt, launch) = EnrollmentAttempt::begin_local(auth).await.unwrap();
        for _ in 0..40 {
            assert!(matches!(
                attempt.claim(&[0; 31]).await,
                Err(EnrollmentError::Denied)
            ));
        }
        let snapshot = attempt.audit().unwrap();
        assert_eq!(snapshot.events.len(), AUDIT_LIMIT);
        assert_eq!(snapshot.omitted_events, 41 - AUDIT_LIMIT as u64);
        let browser = attempt.claim(launch.as_bytes()).await.unwrap();
        attempt
            .enroll(browser.cookie(), browser.csrf(), PASSWORD.into())
            .await
            .unwrap();
        let snapshot = attempt.audit().unwrap();
        let rendered = format!("{snapshot:?}");
        for secret in [
            PASSWORD.to_owned(),
            directory.path().display().to_string(),
            format!("{:?}", launch.as_bytes()),
            format!("{:?}", browser.cookie()),
            format!("{:?}", browser.csrf()),
        ] {
            assert!(!rendered.contains(&secret));
        }
        assert_eq!(
            snapshot.events.last().unwrap().kind,
            AuditKind::OwnerCreated
        );
        assert!(
            snapshot
                .events
                .iter()
                .all(|event| event.at <= SystemTime::now())
        );
    });
}

#[test]
fn poisoned_attempt_state_fails_closed() {
    let (_directory, auth) = fixture();
    runtime().block_on(async {
        let (attempt, launch) = EnrollmentAttempt::begin_local(auth.clone()).await.unwrap();
        let shared = Arc::clone(&attempt.state);
        assert!(
            std::thread::spawn(move || {
                let _guard = shared.lock().unwrap();
                panic!("fixture poison");
            })
            .join()
            .is_err()
        );
        assert!(matches!(
            attempt.claim(launch.as_bytes()).await,
            Err(EnrollmentError::Unavailable)
        ));
        assert!(matches!(attempt.audit(), Err(EnrollmentError::Unavailable)));
        assert!(auth.state.store.load().unwrap().is_none());
    });
}
