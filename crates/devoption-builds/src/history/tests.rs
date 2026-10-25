use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};
fn fixture() -> (tempfile::TempDir, String, BuildJob) {
    let temporary = tempfile::tempdir().unwrap();
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let path = temporary
        .path()
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let job = BuildJob::queued(
        JobId::from_bytes([1; 16]),
        crate::review_context::tests::plan(),
    );
    (temporary, path, job)
}
#[test]
fn queued_history_survives_reopen_and_recovers_without_authority() {
    let (_temp, path, job) = fixture();
    let mut store = History::open(&path).unwrap();
    assert!(store.load().unwrap().is_empty());
    let job = store.queue(job.plan().clone()).unwrap();
    store.record(&job).unwrap();
    drop(store);
    let mut store = History::open(&path).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].state(), &State::Queued);
    assert_eq!(loaded[0].id(), job.id());
    assert_eq!(loaded[0].commit(), job.plan().details().commit);
    assert_eq!(store.recover_interrupted(), Ok(1));
    assert_eq!(store.recover_interrupted(), Ok(0));
    assert_eq!(store.record(&job), Err(Error::Transition));
    drop(store);
    assert_eq!(
        History::open(&path).unwrap().load().unwrap()[0].state(),
        &State::Interrupted
    );
}
#[test]
fn cancelled_history_is_terminal_and_identity_changes_are_rejected() {
    let (_temp, path, job) = fixture();
    let mut store = History::open(&path).unwrap();
    let mut job = store.queue(job.plan().clone()).unwrap();
    job.cancel_queued().unwrap();
    store.record(&job).unwrap();
    assert_eq!(store.recover_interrupted(), Ok(0));
    let mut changed = job.plan().clone();
    crate::tests::change_source_revision(&mut changed.0);
    assert_eq!(
        store.record(&BuildJob::queued(job.id(), changed)),
        Err(Error::Changed)
    );
    let recovered = BuildJob::recover_interrupted(job.id(), job.plan().clone());
    assert_eq!(store.record(&recovered), Err(Error::Transition));
    assert_eq!(store.load().unwrap()[0].state(), &State::Cancelled);
}
#[test]
fn exclusive_writer_and_directory_or_lock_rebinding_fail_closed() {
    let (temp, path, job) = fixture();
    let mut store = History::open(&path).unwrap();
    assert!(matches!(History::open(&path), Err(Error::Busy)));
    let job = store.queue(job.plan().clone()).unwrap();
    fs::rename(temp.path().join(LOCK), temp.path().join("preserved-lock")).unwrap();
    fs::write(temp.path().join(LOCK), b"").unwrap();
    fs::set_permissions(temp.path().join(LOCK), fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(store.load(), Err(Error::Changed)));
    drop(store);
    let mut store = History::open(&path).unwrap();
    let relocated = temp.path().with_extension("moved");
    fs::rename(temp.path(), &relocated).unwrap();
    assert!(store.record(&job).is_err());
    fs::rename(relocated, temp.path()).unwrap();
}
#[test]
fn stale_pending_is_preserved_and_poisons_writer_without_replacing_history() {
    let (temp, path, job) = fixture();
    let mut store = History::open(&path).unwrap();
    let mut job = store.queue(job.plan().clone()).unwrap();
    let before = fs::read(temp.path().join(RECORD)).unwrap();
    fs::write(temp.path().join(PENDING), b"preserved").unwrap();
    job.cancel_queued().unwrap();
    assert_eq!(store.record(&job), Err(Error::Unavailable));
    assert!(matches!(store.load(), Err(Error::Unavailable)));
    assert_eq!(fs::read(temp.path().join(RECORD)).unwrap(), before);
    assert_eq!(fs::read(temp.path().join(PENDING)).unwrap(), b"preserved");
    drop(store);
    assert_eq!(
        History::open(&path).unwrap().load().unwrap()[0].state(),
        &State::Queued
    );
}
#[test]
fn unsafe_files_corruption_and_oversize_are_never_reset() {
    for case in 0..6 {
        let (temp, path, job) = fixture();
        let mut store = History::open(&path).unwrap();
        store.queue(job.plan().clone()).unwrap();
        drop(store);
        let record = temp.path().join(RECORD);
        match case {
            0 => fs::set_permissions(&record, fs::Permissions::from_mode(0o644)).unwrap(),
            1 => {
                fs::rename(&record, temp.path().join("preserved")).unwrap();
                symlink(temp.path().join("preserved"), &record).unwrap();
            }
            2 => fs::hard_link(&record, temp.path().join("alias")).unwrap(),
            3 => fs::write(&record, b"corrupt").unwrap(),
            4 => fs::write(&record, vec![0; codec::MAX_BYTES + 1]).unwrap(),
            _ => {
                fs::remove_file(&record).unwrap();
                fs::create_dir(&record).unwrap();
            }
        }
        assert!(History::open(&path).is_err());
        assert!(fs::symlink_metadata(record).is_ok());
    }
}
#[test]
fn ten_record_store_reopens_and_accepts_an_eleventh_without_losing_history() {
    let (temp, path, job) = fixture();
    let mut store = History::open(&path).unwrap();
    for _ in 0..10 {
        let mut attempt = store.queue(job.plan().clone()).unwrap();
        attempt.cancel_queued().unwrap();
        store.record(&attempt).unwrap();
    }
    let original = store.load().unwrap();
    let encoded = fs::read(temp.path().join(RECORD)).unwrap();
    assert_eq!(&encoded[..10], b"DVHISTRY\x01\x0a");
    assert_eq!(encoded.len(), 1652);
    drop(store);

    let mut store = History::open(&path).unwrap();
    assert!(store.load().unwrap() == original);
    let added = store.queue(job.plan().clone()).unwrap();
    drop(store);
    let loaded = History::open(&path).unwrap().load().unwrap();
    assert_eq!(loaded.len(), 11);
    assert!(loaded[..10] == original);
    assert_eq!(loaded[10].id(), added.id());
    assert!(!original.iter().any(|record| record.id() == added.id()));
}
#[test]
fn retained_attempt_bound_never_evicts_or_overwrites() {
    let (temp, path, job) = fixture();
    let mut store = History::open(&path).unwrap();
    for _ in 0..MAX_JOBS {
        store.queue(job.plan().clone()).unwrap();
    }
    let before = fs::read(temp.path().join(RECORD)).unwrap();
    assert!(matches!(
        store.queue(job.plan().clone()),
        Err(Error::Capacity)
    ));
    assert!(matches!(
        store.queue_using(job.plan().clone(), |_| panic!(
            "capacity must precede entropy"
        )),
        Err(Error::Capacity)
    ));
    assert_eq!(fs::read(temp.path().join(RECORD)).unwrap(), before);
}
#[test]
fn codec_roundtrips_states_rejects_corruption_duplicates_and_trailing_bytes() {
    use crate::jobs::{ArtifactMetadata, Failure};
    use devoption_registry::Sha256;
    let (_temp, _path, job) = fixture();
    let base = Record::from_job(&job);
    for state in [
        State::Queued,
        State::Running,
        State::Cancelled,
        State::Interrupted,
        State::Succeeded(ArtifactMetadata::new(Sha256::digest(b"fixture"), 123).unwrap()),
        State::Failed(Failure::Preparation),
        State::Failed(Failure::ExitFailure),
        State::Failed(Failure::TimedOut),
        State::Failed(Failure::Terminated),
        State::Failed(Failure::ArtifactRejected),
    ] {
        let mut r = base.clone();
        r.state = state;
        let bytes = codec::encode(&[r.clone()]).unwrap();
        assert!(codec::decode(&bytes).unwrap() == vec![r]);
        for n in [0, 8, 9, 10, 130, bytes.len() - 1] {
            let mut bad = bytes.clone();
            bad[n] ^= 1;
            assert!(codec::decode(&bad).is_err());
        }
        assert!(codec::decode(&bytes[..bytes.len() - 1]).is_err());
        let mut extra = bytes;
        extra.push(0);
        assert!(codec::decode(&extra).is_err());
    }
    assert!(codec::encode(&[base.clone(), base]).is_err());
}
#[test]
fn recovery_preserves_completed_claims_and_interrupts_running() {
    let (_temp, path, job) = fixture();
    let mut store = History::open(&path).unwrap();
    let mut running = Record::from_job(&job);
    running.state = State::Running;
    let mut terminal = running.clone();
    terminal.id = JobId::from_bytes([2; 16]);
    terminal.state = State::Cancelled;
    // Synthetic persisted claims only; no production RunnerEvent is issued.
    store.publish(&[running, terminal]).unwrap();
    assert_eq!(store.recover_interrupted(), Ok(1));
    let records = store.load().unwrap();
    assert_eq!(records[0].state(), &State::Interrupted);
    assert_eq!(records[1].state(), &State::Cancelled);
}
#[test]
fn paths_and_directory_permissions_are_checked_without_repair() {
    let (temp, path, _job) = fixture();
    assert!(History::open("relative").is_err());
    let link = temp.path().join("link");
    symlink(temp.path(), &link).unwrap();
    assert!(History::open(link.to_str().unwrap()).is_err());
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(History::open(&path).is_err());
    assert_eq!(fs::metadata(temp.path()).unwrap().mode() & 0o777, 0o755);
}

#[test]
fn interrupted_write_preserves_old_snapshot_and_leaves_pending_for_inspection() {
    let (temp, path, job) = fixture();
    let mut store = History::open(&path).unwrap();
    let job = store.queue(job.plan().clone()).unwrap();
    let before = fs::read(temp.path().join(RECORD)).unwrap();
    let mut record = Record::from_job(&job);
    record.state = State::Interrupted;
    assert_eq!(
        store.publish_using(&[record], |file, bytes| {
            file.write_all(&bytes[..12])?;
            Err(std::io::Error::other("injected fixture failure"))
        }),
        Err(Error::Unavailable)
    );
    assert_eq!(fs::read(temp.path().join(RECORD)).unwrap(), before);
    assert_eq!(fs::metadata(temp.path().join(PENDING)).unwrap().len(), 12);
    assert!(store.load().is_err());
}
#[test]
fn checksum_does_not_bypass_structural_validation() {
    let (_temp, _path, job) = fixture();
    let original = codec::encode(&[Record::from_job(&job)]).unwrap();
    for (offset, value) in [(10 + 120, 255), (10 + 48, b'z'), (10 + 121, 1)] {
        let mut bytes = original.clone();
        bytes[offset] = value;
        let end = bytes.len() - 32;
        let digest = devoption_registry::Sha256::digest(&bytes[..end]);
        bytes[end..].copy_from_slice(digest.bytes());
        assert!(matches!(codec::decode(&bytes), Err(Error::Corrupt)));
    }
}
#[cfg(target_os = "linux")]
#[test]
fn fifo_record_is_rejected_without_blocking_or_removal() {
    let (temp, path, _job) = fixture();
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        temp.path().join(RECORD),
        Mode::RUSR | Mode::WUSR,
    )
    .unwrap();
    assert!(matches!(History::open(&path), Err(Error::Unsafe)));
    assert!(temp.path().join(RECORD).exists());
}

#[test]
fn allocation_rejects_collisions_across_recovery_and_reopen() {
    let (_temp, path, original) = fixture();
    let plan = original.plan().clone();
    let mut store = History::open(&path).unwrap();
    let first = store
        .queue_using(plan.clone(), |bytes| {
            *bytes = [7; 16];
            Ok(())
        })
        .unwrap();
    assert_eq!(first.state(), &State::Queued);
    assert!(first.plan() == &plan);
    assert_eq!(store.load().unwrap()[0].id(), first.id());
    drop(store);
    let mut store = History::open(&path).unwrap();
    store.recover_interrupted().unwrap();
    let mut calls = 0;
    let retry = store
        .queue_using(plan.clone(), |bytes| {
            calls += 1;
            *bytes = if calls == 1 { [7; 16] } else { [8; 16] };
            Ok(())
        })
        .unwrap();
    assert_eq!(calls, 2);
    assert_ne!(retry.id(), first.id());
    let records = store.load().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].state(), &State::Interrupted);
    assert_eq!(records[1].state(), &State::Queued);
    assert_eq!(store.record(&first), Err(Error::Transition));
}

#[test]
fn entropy_failure_and_collision_exhaustion_preserve_history() {
    let (temp, path, job) = fixture();
    let mut store = History::open(&path).unwrap();
    let first = store.queue(job.plan().clone()).unwrap();
    let before = fs::read(temp.path().join(RECORD)).unwrap();
    assert!(matches!(
        store.queue_using(job.plan().clone(), |_| Err(Error::Randomness)),
        Err(Error::Randomness)
    ));
    let mut calls = 0;
    assert!(matches!(
        store.queue_using(job.plan().clone(), |bytes| {
            calls += 1;
            *bytes = first.id().to_bytes();
            Ok(())
        }),
        Err(Error::Randomness)
    ));
    assert_eq!(calls, 16);
    assert_eq!(fs::read(temp.path().join(RECORD)).unwrap(), before);
    assert!(!temp.path().join(PENDING).exists());
    // Entropy failure does not poison an otherwise healthy writer.
    store.queue(job.plan().clone()).unwrap();
}

#[test]
fn unknown_updates_cannot_insert_attempts() {
    let (temp, path, mut job) = fixture();
    let mut store = History::open(&path).unwrap();
    assert_eq!(store.record(&job), Err(Error::UnknownAttempt));
    job.cancel_queued().unwrap();
    assert_eq!(store.record(&job), Err(Error::UnknownAttempt));
    assert!(store.load().unwrap().is_empty());
    assert!(!temp.path().join(RECORD).exists());
}

#[test]
fn failed_queue_returns_no_attempt_and_preserves_existing_history() {
    let (temp, path, job) = fixture();
    let mut store = History::open(&path).unwrap();
    store.queue(job.plan().clone()).unwrap();
    let before = fs::read(temp.path().join(RECORD)).unwrap();
    fs::write(temp.path().join(PENDING), b"inspect me").unwrap();
    assert!(matches!(
        store.queue(job.plan().clone()),
        Err(Error::Unavailable)
    ));
    assert!(matches!(
        store.queue(job.plan().clone()),
        Err(Error::Unavailable)
    ));
    assert_eq!(fs::read(temp.path().join(RECORD)).unwrap(), before);
    assert_eq!(fs::read(temp.path().join(PENDING)).unwrap(), b"inspect me");
    drop(store);
    assert_eq!(History::open(&path).unwrap().load().unwrap().len(), 1);
}
