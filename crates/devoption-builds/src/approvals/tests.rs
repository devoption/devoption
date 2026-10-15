use super::*;
use std::sync::Barrier;

const OWNER: OwnerId = OwnerId::from_bytes([1; 16]);
fn context() -> Sha256 {
    Sha256::from_hex(&"ab".repeat(32)).unwrap()
}
fn job() -> BuildJob {
    let (catalog, input) = crate::tests::fixture();
    BuildJob::queued(
        JobId::from_bytes([1; 16]),
        BuildPlan::validate(input, &catalog).unwrap(),
    )
}

#[test]
fn exact_binding_consumes_once_without_starting_job_or_allowing_retry() {
    let registry = Approvals::default();
    let job = job();
    let handle = registry.approve(OWNER, &job, context()).unwrap();
    let shared = registry.clone();
    assert_eq!(shared.status(&handle, OWNER), Ok(Status::Pending));
    assert_eq!(
        shared.consume(&handle.clone(), OWNER, &job, &context()),
        Ok(())
    );
    assert_eq!(registry.status(&handle, OWNER), Ok(Status::Consumed));
    assert_eq!(
        registry.consume(&handle, OWNER, &job, &context()),
        Err(Error::Inactive(Status::Consumed))
    );
    assert_eq!(
        registry.revoke(&handle, OWNER),
        Err(Error::Inactive(Status::Consumed))
    );
    assert!(matches!(
        registry.approve(OWNER, &job, context()),
        Err(Error::AlreadyReviewed)
    ));
    assert_eq!(job.state(), &JobState::Queued);
    let retry = BuildJob::queued(JobId::from_bytes([2; 16]), job.plan().clone());
    assert!(registry.approve(OWNER, &retry, context()).is_ok());
}

#[test]
fn exact_deadline_expires_and_cannot_be_revived() {
    let job = job();
    for (elapsed, expected) in [
        (LIFETIME - Duration::from_nanos(1), Ok(())),
        (LIFETIME, Err(Error::Inactive(Status::Expired))),
        (
            LIFETIME + Duration::from_secs(1),
            Err(Error::Inactive(Status::Expired)),
        ),
    ] {
        let registry = Approvals::default();
        let now = Instant::now();
        let handle = registry
            .approve_with_clock(OWNER, &job, context(), || now)
            .unwrap();
        assert_eq!(
            registry.consume_with_clock(&handle, OWNER, &job, &context(), || now + elapsed),
            expected
        );
        assert!(
            registry
                .consume_with_clock(&handle, OWNER, &job, &context(), || now)
                .is_err()
        );
    }
}

#[test]
fn wrong_owner_and_attempt_do_not_consume_or_revoke() {
    let registry = Approvals::default();
    let job = job();
    let handle = registry.approve(OWNER, &job, context()).unwrap();
    let other = OwnerId::from_bytes([2; 16]);
    assert_eq!(
        registry.consume(&handle, other, &job, &context()),
        Err(Error::WrongOwner)
    );
    assert_eq!(registry.revoke(&handle, other), Err(Error::WrongOwner));
    assert_eq!(registry.status(&handle, other), Err(Error::WrongOwner));
    let wrong = BuildJob::queued(JobId::from_bytes([3; 16]), job.plan().clone());
    assert_eq!(
        registry.consume(&handle, OWNER, &wrong, &context()),
        Err(Error::WrongJob)
    );
    assert_eq!(registry.consume(&handle, OWNER, &job, &context()), Ok(()));
}

#[test]
fn changing_any_plan_field_or_external_context_permanently_invalidates() {
    let original = job();
    // Adversarial changes need not pass plan validation to prove exact comparison.
    for index in 0..14 {
        let registry = Approvals::default();
        let handle = registry.approve(OWNER, &original, context()).unwrap();
        let mut changed = original.plan().clone();
        match index {
            0 => changed.0.project = devoption_projects::ProjectId::from_bytes([9; 16]),
            1 => changed.0.repository = devoption_projects::RepositoryId::from_bytes([9; 16]),
            2 => crate::tests::change_source_revision(&mut changed.0),
            3 => crate::tests::change_source_bytes(&mut changed.0),
            4 => changed.0.rust_version = "1.1.0".into(),
            5 => changed.0.cargo_executable = "/changed/cargo".into(),
            6 => changed.0.work_directory = "/changed/work".into(),
            7 => changed.0.package = "changed".into(),
            8 => changed.0.binary = "changed".into(),
            9 => changed.0.environment.push(crate::EnvironmentEntry {
                name: "OTHER".into(),
                value: "changed".into(),
            }),
            10 => changed.0.limits.elapsed_seconds += 1,
            11 => changed.0.limits.log_bytes += 1,
            12 => {
                changed.0.dependencies = devoption_registry::CargoMaterialization::new(
                    changed.0.dependencies.request().clone(),
                    Sha256::from_hex(&"ef".repeat(32)).unwrap(),
                    changed.0.dependencies.bundle_bytes() + 1,
                )
            }
            _ => {}
        }
        let digest = if index == 13 {
            Sha256::from_hex(&"cd".repeat(32)).unwrap()
        } else {
            context()
        };
        let candidate = BuildJob::queued(original.id(), changed);
        assert_eq!(
            registry.consume(&handle, OWNER, &candidate, &digest),
            Err(Error::ReviewChanged)
        );
        assert_eq!(
            registry.consume(&handle, OWNER, &original, &context()),
            Err(Error::Inactive(Status::Invalidated))
        );
    }
}

#[test]
fn cancellation_and_failed_external_verification_cannot_be_undone() {
    let registry = Approvals::default();
    let mut job = job();
    let handle = registry.approve(OWNER, &job, context()).unwrap();
    job.cancel_queued().unwrap();
    assert_eq!(
        registry.consume(&handle, OWNER, &job, &context()),
        Err(Error::NotQueued)
    );
    assert_eq!(registry.status(&handle, OWNER), Ok(Status::Invalidated));
    let fresh = Approvals::default();
    assert!(matches!(
        fresh.approve(OWNER, &job, context()),
        Err(Error::NotQueued)
    ));
    let queued = BuildJob::queued(job.id(), job.plan().clone());
    let handle = fresh.approve(OWNER, &queued, context()).unwrap();
    assert_eq!(fresh.revoke(&handle, OWNER), Ok(()));
    assert_eq!(
        fresh.consume(&handle, OWNER, &queued, &context()),
        Err(Error::Inactive(Status::Revoked))
    );
}

#[test]
fn replacement_registry_rejects_old_handles_even_with_identical_records() {
    let job = job();
    let old = Approvals::default();
    let handle = old.approve(OWNER, &job, context()).unwrap();
    let fresh = Approvals::default();
    let current = fresh.approve(OWNER, &job, context()).unwrap();
    assert_eq!(
        fresh.consume(&handle, OWNER, &job, &context()),
        Err(Error::WrongRegistry)
    );
    drop(old);
    assert_eq!(fresh.revoke(&handle, OWNER), Err(Error::WrongRegistry));
    assert_eq!(fresh.consume(&current, OWNER, &job, &context()), Ok(()));
}

#[test]
fn concurrent_consumption_and_revocation_have_exactly_one_winner() {
    let job = Arc::new(job());
    for race_revoke in [false, true] {
        let registry = Approvals::default();
        let handle = registry.approve(OWNER, &job, context()).unwrap();
        let barrier = Arc::new(Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let (registry, handle, barrier, job) = (
                    registry.clone(),
                    handle.clone(),
                    barrier.clone(),
                    job.clone(),
                );
                std::thread::spawn(move || {
                    barrier.wait();
                    if race_revoke && i % 2 == 0 {
                        registry.revoke(&handle, OWNER)
                    } else {
                        registry.consume(&handle, OWNER, &job, &context())
                    }
                })
            })
            .collect();
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(job.state(), &JobState::Queued);
    }
}

#[test]
fn terminal_tombstones_bound_capacity_and_prevent_reissue() {
    let registry = Approvals::default();
    let original = job();
    for id in 0..MAX_ATTEMPTS {
        let job = BuildJob::queued(JobId::from_bytes([id as u8; 16]), original.plan().clone());
        let handle = registry.approve(OWNER, &job, context()).unwrap();
        registry.revoke(&handle, OWNER).unwrap();
        assert!(matches!(
            registry.approve(OWNER, &job, context()),
            Err(Error::AlreadyReviewed)
        ));
    }
    let next = BuildJob::queued(JobId::from_bytes([255; 16]), original.plan().clone());
    assert!(matches!(
        registry.approve(OWNER, &next, context()),
        Err(Error::Capacity)
    ));
    assert!(
        registry
            .records
            .lock()
            .unwrap()
            .entries
            .values()
            .all(|e| e.review.is_none())
    );
}

#[test]
fn poisoned_registry_fails_closed() {
    let registry = Approvals::default();
    let job = job();
    let handle = registry.approve(OWNER, &job, context()).unwrap();
    let clone = registry.clone();
    let _ = std::thread::spawn(move || {
        let _lock = clone.records.lock().unwrap();
        panic!("fixture poison");
    })
    .join();
    assert_eq!(
        registry.consume(&handle, OWNER, &job, &context()),
        Err(Error::Unavailable)
    );
    assert_eq!(registry.revoke(&handle, OWNER), Err(Error::Unavailable));
    assert!(matches!(
        registry.approve(OWNER, &job, context()),
        Err(Error::Unavailable)
    ));
}

#[test]
fn duplicate_concurrent_review_has_one_record_and_does_not_renew_expiry() {
    let registry = Approvals::default();
    let job = Arc::new(job());
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let (registry, job, barrier) = (registry.clone(), job.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                registry.approve(OWNER, &job, context())
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(Error::AlreadyReviewed)))
            .count(),
        1
    );
    assert_eq!(registry.records.lock().unwrap().entries.len(), 1);
}

#[test]
fn clock_is_sampled_under_lock_and_status_observes_expiry_without_renewal() {
    let registry = Approvals::default();
    let job = job();
    let now = Instant::now();
    let handle = registry
        .approve_with_clock(OWNER, &job, context(), || now)
        .unwrap();
    let clock = || {
        assert!(matches!(
            registry.records.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        now + LIFETIME
    };
    assert_eq!(
        registry.with_entry(&handle, OWNER, clock, |entry| Ok(entry.status)),
        Ok(Status::Expired)
    );
    assert_eq!(registry.status(&handle, OWNER), Ok(Status::Expired));
    assert!(
        registry.records.lock().unwrap().entries[&job.id()]
            .review
            .is_none()
    );
    assert!(matches!(
        registry.approve(OWNER, &job, context()),
        Err(Error::AlreadyReviewed)
    ));
}
