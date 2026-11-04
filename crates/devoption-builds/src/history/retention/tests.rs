use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

fn fixture() -> (tempfile::TempDir, tempfile::TempDir, History, BuildPlan) {
    let base = std::env::temp_dir().canonicalize().unwrap();
    let history = tempfile::tempdir_in(&base).unwrap();
    let work = tempfile::tempdir_in(&base).unwrap();
    for dir in [&history, &work] {
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let store = History::open(history.path().canonicalize().unwrap().to_str().unwrap()).unwrap();
    (history, work, store, crate::review_context::tests::plan())
}

fn stage(
    store: &mut History,
    work: &Path,
    plan: BuildPlan,
    n: u128,
) -> (BuildJob, std::path::PathBuf) {
    let job = store.queue(plan).unwrap();
    let path = work.join(format!("{n:032x}"));
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["artifact", "source", "target", "tmp", "cargo", "home"] {
        let child = path.join(name);
        std::fs::create_dir(&child).unwrap();
        std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(child.join("bytes"), vec![42; 4096]).unwrap();
    }
    store.bind_work(job.id(), work, &path).unwrap();
    (job, path)
}

fn cancel(store: &mut History, job: &mut BuildJob) {
    job.cancel_queued().unwrap();
    let mut output = crate::output::BuildOutput::for_plan(job.plan()).unwrap();
    output.append(crate::output::Stream::Stderr, b"compiler output");
    store.checkpoint(job, &output).unwrap();
}

#[test]
fn disk_and_capacity_are_reclaimed_beyond_former_ceiling_without_reusing_ids() {
    let (_dir, work, mut store, plan) = fixture();
    let project = plan.details().project;
    assert_eq!(store.retention_policy(project).unwrap(), Policy::default());
    let mut first = None;
    for n in 1..=125 {
        let (mut job, path) = stage(&mut store, work.path(), plan.clone(), n);
        if first.is_none() {
            first = Some((job.id(), path));
        }
        let approvals = crate::approvals::Approvals::default();
        let owner = crate::approvals::OwnerId::from_bytes([1; 16]);
        let context = crate::review_context::ReviewContext::validate(
            job.plan(),
            crate::review_context::tests::input(job.plan()),
        )
        .unwrap();
        let approval = approvals.approve(owner, &job, context.clone()).unwrap();
        approvals.consume(&approval, owner, &job, &context).unwrap();
        assert!(approvals.consume(&approval, owner, &job, &context).is_err());
        cancel(&mut store, &mut job);
        let result = store
            .reconcile_retention(project, Some(work.path()), || Ok(vec![]))
            .unwrap();
        assert!(!result.failed);
        assert!(store.load().unwrap().len() <= 10);
    }
    let (old_id, old_path) = first.unwrap();
    assert!(!old_path.exists());
    assert!(store.load_output(old_id).is_err());
    assert!(store.audit_exists(old_id).unwrap());
    let mut calls = 0;
    let job = store
        .queue_using(plan, |b| {
            calls += 1;
            *b = if calls == 1 {
                old_id.to_bytes()
            } else {
                [99; 16]
            };
            Ok(())
        })
        .unwrap();
    assert_eq!(calls, 2);
    assert_ne!(job.id(), old_id);
    let records = store.load().unwrap();
    assert!(
        records
            .iter()
            .filter_map(|r| r.completion())
            .all(|n| n > 115)
    );
}

#[test]
fn independent_policies_and_release_references_preserve_exact_bytes() {
    let (dir, work, mut store, plan) = fixture();
    let project = plan.details().project;
    let mut other_plan = plan.clone();
    other_plan.0.project = ProjectId::from_bytes([71; 16]);
    let other = other_plan.details().project;
    for n in 1..=12 {
        let (mut job, _) = stage(&mut store, work.path(), plan.clone(), n);
        cancel(&mut store, &mut job);
        let (mut job, _) = stage(&mut store, work.path(), other_plan.clone(), n + 100);
        cancel(&mut store, &mut job);
    }
    let records = store.load().unwrap();
    let protected = records[0].id;
    let protected_bytes = std::fs::read(
        work.path()
            .join(format!("{:032x}", 1))
            .join("artifact/bytes"),
    )
    .unwrap();
    store
        .set_retention_policy(project, Policy::new(1).unwrap())
        .unwrap();
    let status = store
        .reconcile_retention(project, Some(work.path()), || Ok(vec![protected]))
        .unwrap();
    assert_eq!(
        (status.retained, status.protected, status.removed),
        (2, 1, 10)
    );
    assert!(status.reclaimed_bytes >= 10 * 6 * 4096);
    assert_eq!(
        std::fs::read(
            work.path()
                .join(format!("{:032x}", 1))
                .join("artifact/bytes")
        )
        .unwrap(),
        protected_bytes
    );
    assert_eq!(
        store
            .load()
            .unwrap()
            .iter()
            .filter(|r| r.project == other.to_bytes())
            .count(),
        12
    );
    store.save_retention_status(project, &status).unwrap();
    drop(store);
    let store = History::open(dir.path().canonicalize().unwrap().to_str().unwrap()).unwrap();
    assert_eq!(store.retention_policy(project).unwrap().count(), 1);
    assert_eq!(store.retention_policy(other).unwrap().count(), 10);
    assert_eq!(store.retention_status(project).unwrap(), Some(status));
}

#[test]
fn unsafe_paths_and_unconfirmed_processes_are_preserved_then_retried() {
    let (_dir, work, mut store, plan) = fixture();
    let project = plan.details().project;
    let (mut old, path) = stage(&mut store, work.path(), plan.clone(), 1);
    old.interrupt().unwrap();
    store.record(&old).unwrap();
    let (mut newest, _) = stage(&mut store, work.path(), plan, 2);
    cancel(&mut store, &mut newest);
    store
        .set_retention_policy(project, Policy::new(1).unwrap())
        .unwrap();
    assert_eq!(
        store
            .reconcile_retention(project, Some(work.path()), || Ok(vec![]))
            .unwrap()
            .protected,
        1
    );
    assert!(path.exists());
    store.confirm_interrupted_cleanup().unwrap();
    let external = tempfile::NamedTempFile::new().unwrap();
    symlink(external.path(), path.join("source/link")).unwrap();
    let status = store
        .reconcile_retention(project, Some(work.path()), || Ok(vec![]))
        .unwrap();
    assert!(status.failed);
    assert_eq!(status.reclaimed_bytes, 0);
    assert!(path.join("artifact/bytes").exists());
    assert!(external.path().exists());
    std::fs::remove_file(path.join("source/link")).unwrap();
    std::fs::hard_link(path.join("source/bytes"), path.join("source/alias")).unwrap();
    assert!(
        store
            .reconcile_retention(project, Some(work.path()), || Ok(vec![]))
            .unwrap()
            .failed
    );
    std::fs::remove_file(path.join("source/alias")).unwrap();
    assert!(
        !store
            .reconcile_retention(project, Some(work.path()), || Ok(vec![]))
            .unwrap()
            .failed
    );
    assert!(!path.exists());
}

#[test]
fn interruption_after_directory_removal_retries_from_audit_and_owned_binding() {
    let (dir, work, mut store, plan) = fixture();
    let project = plan.details().project;
    let (mut old, path) = stage(&mut store, work.path(), plan.clone(), 1);
    cancel(&mut store, &mut old);
    let (mut new, _) = stage(&mut store, work.path(), plan, 2);
    cancel(&mut store, &mut new);
    store
        .set_retention_policy(project, Policy::new(1).unwrap())
        .unwrap();
    // Reproduce crash after durable audit and directory deletion but before
    // log/history compaction. The old record remains the retry checkpoint.
    let record = store.load().unwrap().remove(0);
    let audit = codec::encode_with_watermark(&[record], 0).unwrap();
    store
        .publish_retention_snapshot(&key("audit", old.id()), &audit, codec::MAX_BYTES, |f, b| {
            f.write_all(b)
        })
        .unwrap();
    std::fs::remove_dir_all(&path).unwrap();
    drop(store);
    let mut store = History::open(dir.path().canonicalize().unwrap().to_str().unwrap()).unwrap();
    let status = store
        .reconcile_retention(project, Some(work.path()), || Ok(vec![]))
        .unwrap();
    assert!(!status.failed);
    assert_eq!(status.removed, 1);
    assert_eq!(store.load().unwrap().len(), 1);
    assert!(store.audit_exists(old.id()).unwrap());
}

#[test]
fn partial_retention_publication_is_retryable_without_resetting_history() {
    let (_dir, _work, mut store, plan) = fixture();
    let project = plan.details().project;
    let bytes = 1u16.to_be_bytes();
    let name = key("policy", JobId::from_bytes(project.to_bytes()));
    assert_eq!(
        store.publish_retention_snapshot(&name, &bytes, 2, |f, b| {
            f.write_all(&b[..1])?;
            Err(io::Error::other("injected interruption"))
        }),
        Err(Error::Unavailable)
    );
    assert_eq!(store.retention_policy(project).unwrap().count(), 10);
    store
        .set_retention_policy(project, Policy::new(1).unwrap())
        .unwrap();
    assert_eq!(store.retention_policy(project).unwrap().count(), 1);
}

#[test]
fn references_are_rechecked_before_deletion_and_changed_directories_are_refused() {
    let (_dir, work, mut store, plan) = fixture();
    let project = plan.details().project;
    let (mut old, path) = stage(&mut store, work.path(), plan.clone(), 1);
    cancel(&mut store, &mut old);
    let (mut new, _) = stage(&mut store, work.path(), plan, 2);
    cancel(&mut store, &mut new);
    store
        .set_retention_policy(project, Policy::new(1).unwrap())
        .unwrap();
    let mut calls = 0;
    let status = store
        .reconcile_retention(project, Some(work.path()), || {
            calls += 1;
            Ok(if calls == 1 { vec![] } else { vec![old.id()] })
        })
        .unwrap();
    assert_eq!(calls, 2);
    assert_eq!((status.removed, status.protected), (0, 1));
    assert!(path.join("artifact/bytes").exists());
    let preserved = path.with_extension("preserved");
    std::fs::rename(&path, &preserved).unwrap();
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(path.join("unrelated"), b"preserve").unwrap();
    let status = store
        .reconcile_retention(project, Some(work.path()), || Ok(vec![]))
        .unwrap();
    assert!(status.failed);
    assert_eq!(status.reclaimed_bytes, 0);
    assert_eq!(std::fs::read(path.join("unrelated")).unwrap(), b"preserve");
    assert!(preserved.join("artifact/bytes").exists());
}

#[test]
fn failed_status_publication_does_not_expose_an_older_success_as_current_evidence() {
    let (dir, _work, mut store, plan) = fixture();
    let project = plan.details().project;
    let old = RetentionStatus {
        removed: 1,
        reclaimed_bytes: 24_576,
        ..Default::default()
    };
    store.save_retention_status(project, &old).unwrap();
    let failed = RetentionStatus {
        failed: true,
        retained: 12,
        ..Default::default()
    };
    let name = key("retention", JobId::from_bytes(project.to_bytes()));
    let mut bytes = Vec::new();
    for n in [12u64, 0, 0, 0] {
        bytes.extend(n.to_be_bytes());
    }
    bytes.push(1);
    let digest = devoption_registry::Sha256::digest(&[name.as_bytes(), bytes.as_slice()].concat());
    let pending = dir.path().join(format!(
        ".retention-{}.tmp",
        digest
            .bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ));
    // A conflicting pending file is preserved, while the previous valid result
    // remains on disk. The live reader must report unavailable, not that old
    // success, until this project's result can be published successfully.
    std::fs::write(&pending, b"unrelated").unwrap();
    std::fs::set_permissions(&pending, std::fs::Permissions::from_mode(0o600)).unwrap();
    let before = std::fs::read(dir.path().join(&name)).unwrap();
    assert!(store.save_retention_status(project, &failed).is_err());
    assert_eq!(store.retention_status(project), Err(Error::Unavailable));
    assert_eq!(std::fs::read(dir.path().join(&name)).unwrap(), before);
    assert_eq!(std::fs::read(&pending).unwrap(), b"unrelated");
    assert!(
        store
            .retention_status(ProjectId::from_bytes([72; 16]))
            .unwrap()
            .is_none()
    );
    std::fs::remove_file(pending).unwrap();
    store.save_retention_status(project, &failed).unwrap();
    assert_eq!(store.retention_status(project).unwrap(), Some(failed));
}
