use super::super::tests::{Fixture, OTHER, OWNER};
use super::*;
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
};

struct Interrupted {
    _root: tempfile::TempDir,
    path: String,
    candidates: PackageStore,
    catalog: Catalog,
    package: CargoPackage,
    source: ProjectId,
    first: ProjectId,
    second: ProjectId,
    event: Event,
}
impl Interrupted {
    fn new(kind: u8, linked: bool) -> Self {
        let mut f = Fixture::new();
        if kind != 0 {
            f.change(
                0,
                Change::Publish {
                    readers: if kind == 3 {
                        vec![]
                    } else {
                        vec![f.first, f.second]
                    },
                },
            );
        }
        let change = match kind {
            0 => Change::Publish {
                readers: vec![f.first, f.second],
            },
            1 => Change::Readers {
                readers: vec![f.second],
            },
            2 => Change::Yank { yanked: true },
            _ => Change::Readers {
                readers: vec![f.first],
            },
        };
        let event = f.review(0, change).event;
        assert_eq!(
            f.policy.store.stage_using(&event, |file, bytes| {
                file.write_all(bytes)?;
                file.sync_all()?;
                Err(std::io::Error::other("injected after complete append"))
            }),
            Err(StorageError::Uncertain)
        );
        drop(f.policy);
        if linked {
            fs::hard_link(
                std::path::Path::new(&f.path).join(Event::PENDING),
                std::path::Path::new(&f.path).join(event.key()),
            )
            .unwrap();
        }
        Self {
            _root: f._root,
            path: f.path,
            candidates: f.candidates,
            catalog: f.catalog,
            package: f.packages[0].clone(),
            source: f.source,
            first: f.first,
            second: f.second,
            event,
        }
    }
    fn pending(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.path).join(Event::PENDING)
    }
    fn review(&self) -> RecoveryReview {
        Publications::review_recovery(&self.path, OWNER, OWNER, &self.catalog, &self.candidates)
            .unwrap()
    }
    fn approve(&self, review: RecoveryReview) -> Result<(), Error> {
        Publications::approve_recovery(
            &self.path,
            OWNER,
            OWNER,
            &self.catalog,
            &self.candidates,
            review,
        )
    }
    fn archive(&self, policy: &Publications, project: ProjectId) -> Result<Vec<u8>, Error> {
        policy.archive(
            OWNER,
            project,
            &self.catalog,
            &self.candidates,
            self.package.source().coordinates(),
            self.package.archive_sha256(),
        )
    }
}

#[test]
fn complete_publication_grant_revocation_and_yank_require_fresh_review_at_both_commit_boundaries() {
    for kind in 0..4 {
        for linked in [false, true] {
            let f = Interrupted::new(kind, linked);
            assert!(matches!(
                Publications::open(&f.path, OWNER).err(),
                Some(Error::Storage(
                    StorageError::Uncertain | StorageError::Unsafe
                ))
            ));
            let before = fs::read(f.pending()).unwrap();
            let inode = fs::metadata(f.pending()).unwrap().ino();
            let review = f.review();
            assert_eq!(review.final_link_present(), linked);
            assert!(review.decision().change() == &f.event.change);
            assert_eq!(review.decision().candidate().archive(), f.package.archive());
            assert_eq!(review.decision().sequence(), f.event.sequence);
            assert_eq!(fs::read(f.pending()).unwrap(), before);
            assert_eq!(fs::metadata(f.pending()).unwrap().ino(), inode);
            assert!(Publications::open(&f.path, OWNER).is_err());
            f.approve(review).unwrap();
            assert!(!f.pending().exists());
            let policy = Publications::open(&f.path, OWNER).unwrap();
            let status = policy
                .status(
                    OWNER,
                    &f.catalog,
                    &f.candidates,
                    f.package.source().coordinates(),
                )
                .unwrap();
            assert_eq!(status.events_used, f.event.sequence as usize + 1);
            assert!(status.readers.contains(&f.source));
            assert_eq!(status.yanked, kind == 2);
            if kind == 1 {
                assert_eq!(f.archive(&policy, f.first), Err(Error::Denied));
            } else {
                assert_eq!(f.archive(&policy, f.first).unwrap(), f.package.archive());
            }
            if kind != 3 {
                assert!(f.archive(&policy, f.second).is_ok());
            }
            drop(policy);
            assert!(
                Publications::review_recovery(&f.path, OWNER, OWNER, &f.catalog, &f.candidates)
                    .is_err()
            );
            assert!(Publications::open(&f.path, OWNER).is_ok());
        }
    }
}

#[test]
fn recovery_checks_owner_expiry_catalog_and_exclusive_lock() {
    let f = Interrupted::new(1, false);
    assert_eq!(
        Publications::review_recovery("/does/not/exist", OWNER, OTHER, &f.catalog, &f.candidates)
            .err(),
        Some(Error::Denied)
    );
    assert_eq!(
        Publications::review_recovery(&f.path, OTHER, OTHER, &f.catalog, &f.candidates).err(),
        Some(Error::Integrity)
    );
    let review = f.review();
    assert_eq!(
        Publications::approve_recovery(&f.path, OWNER, OTHER, &f.catalog, &f.candidates, review),
        Err(Error::Denied)
    );
    let review = f.review();
    let expired = review.review.issued + REVIEW_LIFETIME;
    assert_eq!(
        Publications::recover_at(
            &f.path,
            OWNER,
            OWNER,
            &f.catalog,
            &f.candidates,
            review,
            expired
        ),
        Err(Error::Changed)
    );
    let review = f.review();
    let mut catalog = f.catalog.clone();
    catalog.rename_project(f.first, "Renamed").unwrap();
    assert_eq!(
        Publications::approve_recovery(&f.path, OWNER, OWNER, &catalog, &f.candidates, review),
        Err(Error::Changed)
    );
    let (locked, _) = RecordStore::<Event>::open_pending(&f.path).unwrap();
    assert_eq!(
        Publications::review_recovery(&f.path, OWNER, OWNER, &f.catalog, &f.candidates).err(),
        Some(Error::Storage(StorageError::Busy))
    );
    drop(locked);
    assert!(f.pending().exists());
    f.approve(f.review()).unwrap();
}

#[test]
fn review_cannot_move_to_a_copied_ledger_or_accept_a_replaced_pending_inode() {
    let f = Interrupted::new(1, false);
    let review = f.review();
    let copy = tempfile::tempdir().unwrap();
    fs::set_permissions(copy.path(), fs::Permissions::from_mode(0o700)).unwrap();
    for entry in fs::read_dir(&f.path).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == Event::LOCK {
            continue;
        }
        let path = copy.path().join(entry.file_name());
        fs::copy(entry.path(), &path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert_eq!(
        Publications::approve_recovery(
            copy.path().canonicalize().unwrap().to_str().unwrap(),
            OWNER,
            OWNER,
            &f.catalog,
            &f.candidates,
            review
        ),
        Err(Error::Changed)
    );
    let review = f.review();
    let mut replacement = tempfile::NamedTempFile::new().unwrap();
    replacement
        .write_all(&fs::read(f.pending()).unwrap())
        .unwrap();
    fs::rename(replacement.path(), f.pending()).unwrap();
    assert_eq!(f.approve(review), Err(Error::Changed));
    assert!(f.pending().exists());
    f.approve(f.review()).unwrap();
}

#[test]
fn changed_pending_decision_and_changed_source_or_candidate_invalidate_recovery() {
    let f = Interrupted::new(1, false);
    let review = f.review();
    let mut changed = f.event.clone();
    changed.change = Change::Yank { yanked: true };
    fs::write(f.pending(), changed.encode()).unwrap();
    assert_eq!(f.approve(review), Err(Error::Changed));
    assert_eq!(fs::read(f.pending()).unwrap(), changed.encode());
    assert_eq!(
        Publications::review_recovery(&f.path, OWNER, OWNER, &Catalog::default(), &f.candidates)
            .err(),
        Some(Error::Denied)
    );
    let key = <CargoPackage as Record>::key_for(f.package.source().coordinates());
    fs::remove_file(f._root.path().join("candidates").join(key)).unwrap();
    assert_eq!(
        Publications::review_recovery(&f.path, OWNER, OWNER, &f.catalog, &f.candidates).err(),
        Some(Error::Unavailable)
    );
    assert!(f.pending().exists());
}

#[test]
fn partial_corrupt_conflicting_and_unsafe_evidence_is_never_removed() {
    for mutation in 0..7 {
        let f = Interrupted::new(1, false);
        let final_path = std::path::Path::new(&f.path).join(f.event.key());
        match mutation {
            0 => fs::write(f.pending(), b"partial").unwrap(),
            1 => {
                let mut bytes = fs::read(f.pending()).unwrap();
                bytes[40] ^= 1;
                fs::write(f.pending(), bytes).unwrap();
            }
            2 => {
                fs::copy(f.pending(), &final_path).unwrap();
                fs::set_permissions(&final_path, fs::Permissions::from_mode(0o600)).unwrap();
            }
            3 => fs::set_permissions(f.pending(), fs::Permissions::from_mode(0o644)).unwrap(),
            4 => fs::hard_link(
                f.pending(),
                std::path::Path::new(&f.path).join("extra-link"),
            )
            .unwrap(),
            5 => {
                fs::remove_file(f.pending()).unwrap();
                symlink("missing", f.pending()).unwrap();
            }
            _ => fs::remove_file(std::path::Path::new(&f.path).join(Event::key_for(&0))).unwrap(),
        }
        let before = fs::read(f.pending()).ok();
        let final_before = fs::read(&final_path).ok();
        assert!(
            Publications::review_recovery(&f.path, OWNER, OWNER, &f.catalog, &f.candidates)
                .is_err(),
            "mutation {mutation}"
        );
        assert_eq!(fs::read(f.pending()).ok(), before);
        assert_eq!(fs::read(&final_path).ok(), final_before);
        assert!(fs::symlink_metadata(f.pending()).is_ok());
    }
}

#[test]
fn changed_link_state_and_unknown_reader_cannot_reuse_an_old_review() {
    let f = Interrupted::new(1, false);
    let review = f.review();
    fs::hard_link(
        f.pending(),
        std::path::Path::new(&f.path).join(f.event.key()),
    )
    .unwrap();
    assert_eq!(f.approve(review), Err(Error::Changed));
    let fresh = f.review();
    assert!(fresh.final_link_present());
    f.approve(fresh).unwrap();
    let f = Interrupted::new(1, false);
    let mut changed = f.event.clone();
    let mut readers = vec![f.source, ProjectId::from_bytes([255; 16])];
    readers.sort_by_key(|id| id.to_bytes());
    changed.change = Change::Readers { readers };
    fs::write(f.pending(), changed.encode()).unwrap();
    assert_eq!(
        Publications::review_recovery(&f.path, OWNER, OWNER, &f.catalog, &f.candidates).err(),
        Some(Error::Denied)
    );
    assert_eq!(fs::read(f.pending()).unwrap(), changed.encode());
}

#[test]
#[ignore = "child-only abrupt policy append fixture, exercised by revocation_recovers_after_abrupt_process_exit"]
fn manual_policy_append_crash() {
    let path = std::env::var("DEVOPTION_POLICY_CRASH_PATH").unwrap();
    let expected = std::env::var("DEVOPTION_POLICY_CRASH_EXPECTED").unwrap();
    let linked = std::env::var("DEVOPTION_POLICY_CRASH_LINKED").unwrap() == "yes";
    let event = Event::decode(&fs::read(expected).unwrap()).unwrap();
    let mut store = RecordStore::<Event>::open(&path).unwrap();
    let _ = store.stage_using(&event, |file, bytes| {
        file.write_all(bytes)?;
        file.sync_all()?;
        if linked {
            fs::hard_link(
                std::path::Path::new(&path).join(Event::PENDING),
                std::path::Path::new(&path).join(event.key()),
            )?;
        }
        std::process::exit(42);
    });
    panic!("fixture did not exit");
}

#[test]
fn revocation_recovers_after_abrupt_process_exit() {
    use std::{
        process::{Command, Stdio},
        time::Duration,
    };
    for linked in [false, true] {
        let mut f = Fixture::new();
        f.change(
            0,
            Change::Publish {
                readers: vec![f.first, f.second],
            },
        );
        let event = f
            .review(
                0,
                Change::Readers {
                    readers: vec![f.second],
                },
            )
            .event;
        let mut expected = tempfile::NamedTempFile::new().unwrap();
        expected.write_all(&event.encode()).unwrap();
        expected.flush().unwrap();
        drop(f.policy);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "library::publication::recovery::tests::manual_policy_append_crash",
                "--ignored",
            ])
            .env_clear()
            .env("DEVOPTION_POLICY_CRASH_PATH", &f.path)
            .env("DEVOPTION_POLICY_CRASH_EXPECTED", expected.path())
            .env(
                "DEVOPTION_POLICY_CRASH_LINKED",
                if linked { "yes" } else { "no" },
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() > deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("policy crash fixture timed out");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(42));
        assert!(Publications::open(&f.path, OWNER).is_err());
        let review =
            Publications::review_recovery(&f.path, OWNER, OWNER, &f.catalog, &f.candidates)
                .unwrap();
        assert_eq!(review.final_link_present(), linked);
        Publications::approve_recovery(&f.path, OWNER, OWNER, &f.catalog, &f.candidates, review)
            .unwrap();
        f.policy = Publications::open(&f.path, OWNER).unwrap();
        assert_eq!(f.archive(f.first, 0), Err(Error::Denied));
        assert!(f.archive(f.second, 0).is_ok());
        assert!(!std::path::Path::new(&f.path).join(Event::PENDING).exists());
    }
}

#[test]
fn completing_the_last_event_preserves_capacity_denial_instead_of_resetting_the_ledger() {
    let mut f = Fixture::new();
    f.change(
        0,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    for n in 1..MAX_POLICY_EVENTS - 1 {
        f.change(0, Change::Yank { yanked: n % 2 == 1 });
    }
    let event = f.review(0, Change::Yank { yanked: true }).event;
    assert_eq!(
        f.policy.store.stage_using(&event, |file, bytes| {
            file.write_all(bytes)?;
            Err(std::io::Error::other("injected final event"))
        }),
        Err(StorageError::Uncertain)
    );
    drop(f.policy);
    let review =
        Publications::review_recovery(&f.path, OWNER, OWNER, &f.catalog, &f.candidates).unwrap();
    assert_eq!(review.decision().remaining_capacity(), 0);
    Publications::approve_recovery(&f.path, OWNER, OWNER, &f.catalog, &f.candidates, review)
        .unwrap();
    f.policy = Publications::open(&f.path, OWNER).unwrap();
    let status = f
        .policy
        .status(
            OWNER,
            &f.catalog,
            &f.candidates,
            f.packages[0].source().coordinates(),
        )
        .unwrap();
    assert_eq!(status.events_used, MAX_POLICY_EVENTS);
    assert!(status.yanked);
    assert_eq!(f.archive(f.first, 0), Err(Error::Capacity));
}
