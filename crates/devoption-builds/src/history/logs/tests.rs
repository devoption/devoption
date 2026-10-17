use super::*;
use crate::output::Stream;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};
fn fixture() -> (tempfile::TempDir, String, History, BuildJob, BuildOutput) {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let path = temp
        .path()
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let mut store = History::open(&path).unwrap();
    let job = store.queue(crate::review_context::tests::plan()).unwrap();
    let output = BuildOutput::for_plan(job.plan()).unwrap();
    (temp, path, store, job, output)
}
#[test]
fn checkpoints_survive_recovery_with_streams_and_escaping_intact() {
    let (_temp, path, mut store, job, mut output) = fixture();
    assert!(store.load_output(job.id()).unwrap().is_none());
    store.save_output(&job, &output).unwrap();
    assert_eq!(
        store
            .load_output(job.id())
            .unwrap()
            .unwrap()
            .retained_bytes(),
        0
    );
    output.append(Stream::Stdout, b"<script>\x1b[31m\xff");
    output.append(Stream::Stderr, "error\u{202e}\n".as_bytes());
    store.save_output(&job, &output).unwrap();
    drop(store);
    let mut store = History::open(&path).unwrap();
    assert_eq!(store.recover_interrupted(), Ok(1));
    let saved = store.load_output(job.id()).unwrap().unwrap();
    for stream in [Stream::Stdout, Stream::Stderr] {
        assert_eq!(saved.escaped_text(stream), output.escaped_text(stream));
    }
    assert_eq!(saved.retained_bytes(), output.retained_bytes());
    assert_eq!(store.save_output(&job, &output), Err(Error::Transition));
}
#[test]
fn checkpoints_cannot_shrink_replace_or_clear_truncation() {
    let (_temp, _path, mut store, job, mut output) = fixture();
    output.append(Stream::Stdout, b"first");
    store.save_output(&job, &output).unwrap();
    let mut replacement = BuildOutput::for_plan(job.plan()).unwrap();
    assert_eq!(store.save_output(&job, &replacement), Err(Error::Changed));
    replacement.append(Stream::Stdout, b"other");
    assert_eq!(store.save_output(&job, &replacement), Err(Error::Changed));
    output.append(Stream::Stderr, b"second");
    store.save_output(&job, &output).unwrap();
    output.append(Stream::Stdout, &vec![b'x'; crate::output::MAX_BYTES]);
    assert!(output.truncated());
    store.save_output(&job, &output).unwrap();
    let saved = store.load_output(job.id()).unwrap().unwrap();
    assert!(saved.truncated());
    assert!(saved.limit_reached());
    let mut bytes = saved.snapshot(job.id());
    bytes[37] = 0;
    resign(&mut bytes);
    let cleared = BuildOutput::restore(&bytes, job.id()).unwrap();
    assert_eq!(store.save_output(&job, &cleared), Err(Error::Changed));
}
fn resign(bytes: &mut [u8]) {
    let end = bytes.len() - 32;
    let digest = devoption_registry::Sha256::digest(&bytes[..end]);
    bytes[end..].copy_from_slice(digest.bytes());
}
#[test]
fn identity_limit_and_terminal_checks_preserve_bytes() {
    let (temp, _path, mut store, mut job, output) = fixture();
    store.save_output(&job, &output).unwrap();
    let name = temp.path().join(filename(job.id()));
    let before = fs::read(&name).unwrap();
    let unknown = BuildJob::queued(JobId::from_bytes([0; 16]), job.plan().clone());
    assert!(matches!(
        store.load_output(unknown.id()),
        Err(Error::UnknownAttempt)
    ));
    assert_eq!(
        store.save_output(&unknown, &output),
        Err(Error::UnknownAttempt)
    );
    let mut changed = job.plan().clone();
    crate::tests::change_source_revision(&mut changed.0);
    assert_eq!(
        store.save_output(&BuildJob::queued(job.id(), changed), &output),
        Err(Error::Changed)
    );
    let mut bytes = output.snapshot(job.id());
    bytes[25..29].copy_from_slice(&1u32.to_be_bytes());
    resign(&mut bytes);
    let different_limit = BuildOutput::restore(&bytes, job.id()).unwrap();
    assert_eq!(
        store.save_output(&job, &different_limit),
        Err(Error::Changed)
    );
    job.cancel_queued().unwrap();
    store.record(&job).unwrap();
    assert_eq!(store.save_output(&job, &output), Err(Error::Transition));
    assert_eq!(fs::read(name).unwrap(), before);
}
#[test]
fn codec_rejects_identity_corruption_and_invalid_structure() {
    let (_temp, _path, _store, job, mut output) = fixture();
    output.append(Stream::Stdout, b"test");
    let original = output.snapshot(job.id());
    for (offset, value) in [
        (0, 0),
        (9, 42),
        (25, 255),
        (29, 255),
        (33, 255),
        (37, 2),
        (37, 1),
    ] {
        let mut bytes = original.clone();
        bytes[offset] = value;
        resign(&mut bytes);
        assert!(BuildOutput::restore(&bytes, job.id()).is_err());
    }
    let mut corrupt = original.clone();
    corrupt[38] ^= 1;
    assert!(BuildOutput::restore(&corrupt, job.id()).is_err());
    for n in 0..original.len() {
        assert!(BuildOutput::restore(&original[..n], job.id()).is_err());
    }
    let mut extra = original;
    extra.push(0);
    assert!(BuildOutput::restore(&extra, job.id()).is_err());
}
#[test]
fn unsafe_log_files_are_rejected_and_preserved() {
    for case in 0..6 {
        let (temp, _path, mut store, job, output) = fixture();
        store.save_output(&job, &output).unwrap();
        let name = temp.path().join(filename(job.id()));
        match case {
            0 => fs::set_permissions(&name, fs::Permissions::from_mode(0o644)).unwrap(),
            1 => {
                fs::rename(&name, temp.path().join("keep")).unwrap();
                symlink(temp.path().join("keep"), &name).unwrap();
            }
            2 => fs::hard_link(&name, temp.path().join("keep")).unwrap(),
            3 => fs::write(&name, b"invalid").unwrap(),
            4 => fs::write(&name, vec![0; MAX_SNAPSHOT + 1]).unwrap(),
            _ => {
                fs::remove_file(&name).unwrap();
                fs::create_dir(&name).unwrap();
            }
        }
        assert!(store.load_output(job.id()).is_err());
        assert!(store.save_output(&job, &output).is_err());
        assert!(fs::symlink_metadata(name).is_ok());
    }
}
#[test]
fn failed_checkpoint_preserves_prior_snapshot_and_poisons_writer() {
    let (temp, _path, mut store, job, mut output) = fixture();
    store.save_output(&job, &output).unwrap();
    let name = temp.path().join(filename(job.id()));
    let before = fs::read(&name).unwrap();
    fs::write(temp.path().join(PENDING), b"keep").unwrap();
    output.append(Stream::Stdout, b"next");
    assert_eq!(store.save_output(&job, &output), Err(Error::Unavailable));
    assert!(store.load().is_err());
    assert_eq!(fs::read(name).unwrap(), before);
    assert_eq!(fs::read(temp.path().join(PENDING)).unwrap(), b"keep");
}
#[cfg(target_os = "linux")]
#[test]
fn fifo_log_does_not_block_or_get_replaced() {
    let (temp, _path, mut store, job, output) = fixture();
    let name = temp.path().join(filename(job.id()));
    rustix::fs::mkfifoat(rustix::fs::CWD, &name, Mode::RUSR | Mode::WUSR).unwrap();
    assert!(matches!(store.load_output(job.id()), Err(Error::Unsafe)));
    assert_eq!(store.save_output(&job, &output), Err(Error::Unsafe));
    assert!(name.exists());
}

#[test]
fn swapping_two_attempt_logs_is_rejected() {
    let (temp, _path, mut store, first, output) = fixture();
    let second = store.queue(first.plan().clone()).unwrap();
    store.save_output(&first, &output).unwrap();
    fs::rename(
        temp.path().join(filename(first.id())),
        temp.path().join(filename(second.id())),
    )
    .unwrap();
    assert!(matches!(
        store.load_output(second.id()),
        Err(Error::Corrupt)
    ));
    assert_eq!(store.save_output(&second, &output), Err(Error::Corrupt));
    assert!(store.load_output(first.id()).unwrap().is_none());
}

#[test]
fn maximum_exact_fill_roundtrips_without_inventing_truncation() {
    let (_temp, _path, mut store, original, _output) = fixture();
    let mut plan = original.plan().clone();
    plan.0.limits.log_bytes = crate::output::MAX_BYTES as u32;
    let job = store.queue(plan).unwrap();
    let mut output = BuildOutput::for_plan(job.plan()).unwrap();
    output.append(Stream::Stdout, &vec![b'x'; crate::output::MAX_BYTES / 2]);
    output.append(Stream::Stderr, &vec![b'y'; crate::output::MAX_BYTES / 2]);
    store.save_output(&job, &output).unwrap();
    let saved = store.load_output(job.id()).unwrap().unwrap();
    assert_eq!(saved.snapshot(job.id()).len(), MAX_SNAPSHOT);
    assert!(saved.limit_reached());
    assert!(!saved.truncated());
    assert_eq!(
        saved.escaped_text(Stream::Stderr),
        output.escaped_text(Stream::Stderr)
    );
}

#[test]
fn runner_checkpoint_saves_final_output_before_sealing_each_terminal_result() {
    use crate::jobs::{ArtifactMetadata, Failure, RunnerEvent};
    let digest = devoption_registry::Sha256::digest(b"synthetic artifact");
    for terminal in [
        State::Succeeded(ArtifactMetadata::new(digest, 18).unwrap()),
        State::Failed(Failure::ExitFailure),
        State::Cancelled,
    ] {
        let (_temp, _path, mut store, mut job, mut output) = fixture();
        job.apply(RunnerEvent::observed(&job, State::Running))
            .unwrap();
        store.checkpoint(&job, &output).unwrap();
        output.append(Stream::Stderr, b"final compiler diagnostic");
        job.apply(RunnerEvent::observed(&job, terminal.clone()))
            .unwrap();
        store.checkpoint(&job, &output).unwrap();
        assert_eq!(store.load().unwrap()[0].state(), &terminal);
        assert_eq!(
            store
                .load_output(job.id())
                .unwrap()
                .unwrap()
                .retained_bytes(),
            output.retained_bytes()
        );
        // Sealing remains enforced; a completed attempt cannot append more logs.
        assert_eq!(store.save_output(&job, &output), Err(Error::Transition));
    }
}
