use super::*;
use crate::library::cargo_package::tests::publication_fixture;
use std::{fs, os::unix::fs::PermissionsExt};
pub(super) const OWNER: OwnerId = OwnerId::from_bytes([1; 16]);
pub(super) const OTHER: OwnerId = OwnerId::from_bytes([2; 16]);
pub(super) struct Fixture {
    pub(super) _root: tempfile::TempDir,
    pub(super) path: String,
    pub(super) candidates: PackageStore,
    pub(super) policy: Publications,
    pub(super) catalog: Catalog,
    pub(super) packages: [CargoPackage; 3],
    pub(super) source: ProjectId,
    pub(super) first: ProjectId,
    pub(super) second: ProjectId,
}
impl Fixture {
    pub(super) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let rootpath = root.path().canonicalize().unwrap();
        for dir in ["candidates", "policy"] {
            fs::create_dir(rootpath.join(dir)).unwrap();
            fs::set_permissions(rootpath.join(dir), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut candidates =
            PackageStore::open(rootpath.join("candidates").to_str().unwrap()).unwrap();
        let (catalog, packages) = publication_fixture();
        for package in &packages {
            candidates.stage(package).unwrap();
        }
        let source = ProjectId::from_bytes(packages[0].source().project());
        let projects: Vec<_> = catalog
            .projects()
            .iter()
            .filter(|p| p.id() != source)
            .map(|p| p.id())
            .collect();
        let path = rootpath.join("policy").to_str().unwrap().to_owned();
        Self {
            _root: root,
            policy: Publications::open(&path, OWNER).unwrap(),
            path,
            candidates,
            catalog,
            packages,
            source,
            first: projects[0],
            second: projects[1],
        }
    }
    pub(super) fn review(&self, at: usize, change: Change) -> Review {
        self.policy
            .review(
                OWNER,
                &self.catalog,
                &self.candidates,
                self.packages[at].source().coordinates(),
                change,
            )
            .unwrap()
    }
    pub(super) fn change(&mut self, at: usize, change: Change) {
        let review = self.review(at, change);
        self.policy
            .approve(OWNER, &self.catalog, &self.candidates, review)
            .unwrap();
    }
    pub(super) fn line(&self, project: ProjectId, at: usize) -> Result<Vec<u8>, Error> {
        self.policy.index_line(
            OWNER,
            project,
            &self.catalog,
            &self.candidates,
            self.packages[at].source().coordinates(),
        )
    }
    pub(super) fn archive(&self, project: ProjectId, at: usize) -> Result<Vec<u8>, Error> {
        self.policy.archive(
            OWNER,
            project,
            &self.catalog,
            &self.candidates,
            self.packages[at].source().coordinates(),
            self.packages[at].archive_sha256(),
        )
    }
}

#[test]
fn unpublished_and_unapproved_versions_deny_reads_and_owner_identity_is_required() {
    let mut f = Fixture::new();
    assert_eq!(f.line(f.first, 0), Err(Error::Denied));
    let review = f.review(
        0,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    assert_eq!(review.coordinates().name(), "private-base");
    assert_eq!(review.candidate().archive(), f.packages[0].archive());
    assert_eq!(f.archive(f.first, 0), Err(Error::Denied));
    assert_eq!(
        f.policy.approve(OTHER, &f.catalog, &f.candidates, review),
        Err(Error::Denied)
    );
    assert_eq!(
        f.policy
            .review(
                OTHER,
                &f.catalog,
                &f.candidates,
                f.packages[0].source().coordinates(),
                Change::Publish { readers: vec![] }
            )
            .err(),
        Some(Error::Denied)
    );
    f.change(
        0,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    assert_eq!(
        f.policy.index_line(
            OTHER,
            f.first,
            &f.catalog,
            &f.candidates,
            f.packages[0].source().coordinates()
        ),
        Err(Error::Denied)
    );
}

#[test]
fn exact_grants_default_private_and_revocation_are_rechecked_at_archive_acquisition() {
    let mut f = Fixture::new();
    f.change(0, Change::Publish { readers: vec![] });
    assert!(f.line(f.source, 0).is_ok());
    assert_eq!(f.line(f.first, 0), Err(Error::Denied));
    f.change(
        0,
        Change::Readers {
            readers: vec![f.first, f.second],
        },
    );
    assert_eq!(f.archive(f.first, 0).unwrap(), f.packages[0].archive());
    assert_eq!(f.archive(f.second, 0).unwrap(), f.packages[0].archive());
    f.line(f.first, 0).unwrap();
    f.change(
        0,
        Change::Readers {
            readers: vec![f.second],
        },
    );
    assert_eq!(f.archive(f.first, 0), Err(Error::Denied));
    assert!(f.archive(f.second, 0).is_ok());
    assert!(f.archive(f.source, 0).is_ok());
    let unknown = ProjectId::from_bytes([255; 16]);
    assert_eq!(f.line(unknown, 0), Err(Error::Denied));
    assert_eq!(
        f.policy
            .review(
                OWNER,
                &f.catalog,
                &f.candidates,
                f.packages[0].source().coordinates(),
                Change::Readers {
                    readers: vec![unknown]
                }
            )
            .err(),
        Some(Error::Denied)
    );
}

#[test]
fn yanks_preserve_checksum_locked_bytes_and_older_version_independence() {
    let mut f = Fixture::new();
    f.change(
        1,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    f.change(
        2,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    let archive = f.archive(f.first, 1).unwrap();
    f.change(1, Change::Yank { yanked: true });
    assert_eq!(
        serde_json::from_slice::<Value>(&f.line(f.first, 1).unwrap()).unwrap()["yanked"],
        true
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&f.line(f.first, 2).unwrap()).unwrap()["yanked"],
        false
    );
    assert_eq!(f.archive(f.first, 1).unwrap(), archive);
    assert_eq!(
        f.policy.archive(
            OWNER,
            f.first,
            &f.catalog,
            &f.candidates,
            f.packages[1].source().coordinates(),
            Sha256::digest(b"wrong lock")
        ),
        Err(Error::Integrity)
    );
    f.change(1, Change::Yank { yanked: false });
    assert_eq!(
        serde_json::from_slice::<Value>(&f.line(f.first, 1).unwrap()).unwrap()["yanked"],
        false
    );
}

#[test]
fn review_binds_ledger_catalog_candidate_and_expiry_without_cross_store_transfer() {
    let mut f = Fixture::new();
    let review = f.review(
        0,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    let expired = review.issued + REVIEW_LIFETIME;
    assert_eq!(
        f.policy
            .approve_at(OWNER, &f.catalog, &f.candidates, review, expired),
        Err(Error::Changed)
    );
    let review = f.review(
        0,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    let mut changed = f.catalog.clone();
    changed.rename_project(f.first, "Renamed").unwrap();
    assert_eq!(
        f.policy.approve(OWNER, &changed, &f.candidates, review),
        Err(Error::Changed)
    );
    let review = f.review(
        0,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    let other = Fixture::new();
    assert_ne!(
        f.policy.store.binding().unwrap(),
        other.policy.store.binding().unwrap()
    );
    let path = other.path.clone();
    drop(other.policy);
    let mut second = Publications::open(&path, OWNER).unwrap();
    assert_eq!(
        second.approve(OWNER, &f.catalog, &f.candidates, review),
        Err(Error::Changed)
    );
    let review = f.review(
        0,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    f.change(
        1,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    assert_eq!(
        f.policy.approve(OWNER, &f.catalog, &f.candidates, review),
        Err(Error::Changed)
    );
    assert_eq!(f.line(f.first, 0), Err(Error::Denied));
}

#[test]
fn ledger_and_yanks_survive_reopen_and_wrong_owner_cannot_adopt_them() {
    let mut f = Fixture::new();
    f.change(
        0,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    f.change(0, Change::Yank { yanked: true });
    drop(f.policy);
    assert_eq!(
        Publications::open(&f.path, OTHER).err(),
        Some(Error::Integrity)
    );
    f.policy = Publications::open(&f.path, OWNER).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&f.line(f.first, 0).unwrap()).unwrap()["yanked"],
        true
    );
    assert!(f.archive(f.first, 0).is_ok());
    assert_eq!(f.line(f.second, 0), Err(Error::Denied));
    assert_eq!(
        f.policy
            .review(
                OWNER,
                &f.catalog,
                &f.candidates,
                f.packages[0].source().coordinates(),
                Change::Publish {
                    readers: vec![f.first]
                }
            )
            .err(),
        Some(Error::Conflict)
    );
}

#[test]
fn aliases_rebinding_and_duplicate_reader_sets_are_refused() {
    let mut f = Fixture::new();
    f.change(
        1,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    let c = f.packages[1].source().coordinates();
    let alias = Coordinates::new(c.registry().clone(), "SHARED_helper", "0.1.0+different").unwrap();
    let rebound = Coordinates::new(
        HostedRegistry::new(
            c.registry().id().clone(),
            "sparse+https://other.example.test/index/",
        )
        .unwrap(),
        c.name(),
        c.version(),
    )
    .unwrap();
    for changed in [&alias, &rebound] {
        assert_eq!(
            f.policy
                .index_line(OWNER, f.first, &f.catalog, &f.candidates, changed),
            Err(Error::Denied)
        );
    }
    assert_eq!(
        f.policy
            .review(
                OWNER,
                &f.catalog,
                &f.candidates,
                c,
                Change::Readers {
                    readers: vec![f.first, f.first]
                }
            )
            .err(),
        Some(Error::Denied)
    );
}

#[test]
fn corrupt_missing_and_pending_policy_evidence_never_becomes_an_empty_registry() {
    for mutation in 0..4 {
        let mut f = Fixture::new();
        f.change(
            0,
            Change::Publish {
                readers: vec![f.first],
            },
        );
        f.change(
            1,
            Change::Publish {
                readers: vec![f.first],
            },
        );
        let first = std::path::Path::new(&f.path).join(Event::key_for(&0));
        match mutation {
            0 => {
                let mut bytes = fs::read(&first).unwrap();
                bytes[30] ^= 1;
                fs::write(&first, bytes).unwrap();
            }
            1 => fs::remove_file(&first).unwrap(),
            2 => {
                fs::write(
                    std::path::Path::new(&f.path).join(Event::PENDING),
                    b"partial",
                )
                .unwrap();
            }
            _ => fs::set_permissions(&first, fs::Permissions::from_mode(0o644)).unwrap(),
        }
        assert!(f.line(f.first, 0).is_err());
        drop(f.policy);
        assert!(Publications::open(&f.path, OWNER).is_err());
    }
}

#[test]
fn partial_append_preserves_evidence_and_blocks_every_read() {
    let mut f = Fixture::new();
    let review = f.review(
        0,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    use std::io::Write;
    assert_eq!(
        f.policy.store.stage_using(&review.event, |file, bytes| {
            file.write_all(&bytes[..100])?;
            Err(std::io::Error::other("injected write failure"))
        }),
        Err(StorageError::Uncertain)
    );
    assert!(f.line(f.first, 0).is_err());
    let pending = std::path::Path::new(&f.path).join(Event::PENDING);
    assert_eq!(fs::read(&pending).unwrap().len(), 100);
    drop(f.policy);
    assert_eq!(
        Publications::open(&f.path, OWNER).err(),
        Some(Error::Storage(StorageError::Uncertain))
    );
    assert_eq!(fs::read(&pending).unwrap().len(), 100);
}

#[test]
fn event_codec_refuses_truncation_tampering_and_noncanonical_fields() {
    let f = Fixture::new();
    let review = f.review(
        0,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    let bytes = review.event.encode();
    for n in [0, 31, bytes.len() - 1] {
        assert!(Event::decode(&bytes[..n]).is_err());
    }
    let mut changed = bytes.clone();
    changed[40] ^= 1;
    assert!(Event::decode(&changed).is_err());
    let mut value: Value = serde_json::from_slice(&bytes[..bytes.len() - 32]).unwrap();
    value["readers"] = json!([hex(&f.first.to_bytes()), hex(&f.first.to_bytes())]);
    let mut changed = serde_json::to_vec(&value).unwrap();
    changed.extend(Sha256::digest(&changed).bytes());
    assert!(Event::decode(&changed).is_err());
}

#[test]
fn missing_or_corrupt_candidates_do_not_fall_back_to_published_metadata() {
    for corrupt in [false, true] {
        let mut f = Fixture::new();
        f.change(
            0,
            Change::Publish {
                readers: vec![f.first],
            },
        );
        let path = f
            ._root
            .path()
            .join("candidates")
            .join(<CargoPackage as Record>::key_for(
                f.packages[0].source().coordinates(),
            ));
        if corrupt {
            fs::write(&path, b"corrupt").unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
        assert!(f.line(f.first, 0).is_err());
        assert!(f.archive(f.first, 0).is_err());
    }
}

#[test]
fn logical_registry_and_package_namespace_cannot_switch_source_between_versions() {
    let mut f = Fixture::new();
    f.change(
        1,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    let review = f.review(
        2,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    for mutation in 0..3 {
        let mut event = review.event.clone();
        match mutation {
            0 => event.source = f.first,
            1 => {
                event.coordinates = Coordinates::new(
                    HostedRegistry::new(
                        event.coordinates.registry().id().clone(),
                        "sparse+https://other.example.test/index/",
                    )
                    .unwrap(),
                    event.coordinates.name(),
                    event.coordinates.version(),
                )
                .unwrap()
            }
            _ => {
                event.coordinates = Coordinates::new(
                    event.coordinates.registry().clone(),
                    "Shared_Helper",
                    event.coordinates.version(),
                )
                .unwrap()
            }
        }
        assert_eq!(
            apply(&mut f.policy.state().unwrap().versions, &event),
            Err(Error::Conflict)
        );
    }
    f.policy
        .approve(OWNER, &f.catalog, &f.candidates, review)
        .unwrap();
}

#[test]
fn append_capacity_blocks_acquisition_without_freezing_an_irrevocable_reader_set() {
    let mut f = Fixture::new();
    f.change(
        0,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    for n in 1..super::super::record_store::MAX_RECORDS {
        f.change(0, Change::Yank { yanked: n % 2 == 1 });
    }
    assert_eq!(
        f.policy
            .review(
                OWNER,
                &f.catalog,
                &f.candidates,
                f.packages[0].source().coordinates(),
                Change::Yank { yanked: false }
            )
            .err(),
        Some(Error::Capacity)
    );
    assert_eq!(f.archive(f.first, 0), Err(Error::Capacity));
    assert_eq!(f.archive(f.second, 0), Err(Error::Capacity));
    let status = f
        .policy
        .status(
            OWNER,
            &f.catalog,
            &f.candidates,
            f.packages[0].source().coordinates(),
        )
        .unwrap();
    assert_eq!(status.events_used, status.event_capacity);
    assert!(status.readers.contains(&f.first));
}

#[test]
fn owner_status_reports_exact_policy_and_provenance_without_granting_a_consumer_access() {
    let mut f = Fixture::new();
    f.change(
        0,
        Change::Publish {
            readers: vec![f.first],
        },
    );
    let coordinates = f.packages[0].source().coordinates();
    assert_eq!(
        f.policy
            .status(OTHER, &f.catalog, &f.candidates, coordinates)
            .err(),
        Some(Error::Denied)
    );
    let status = f
        .policy
        .status(OWNER, &f.catalog, &f.candidates, coordinates)
        .unwrap();
    assert!(status.coordinates == *coordinates);
    assert_eq!(status.source, f.source);
    assert_eq!(status.commit, f.packages[0].source().commit());
    assert!(status.candidate_sha256 == Sha256::digest(&f.packages[0].encode()));
    assert!(status.archive_sha256 == f.packages[0].archive_sha256());
    assert_eq!(status.archive_bytes as usize, f.packages[0].archive().len());
    assert_eq!(status.events_used, 1);
    assert_eq!(status.readers.len(), 2);
    assert_eq!(f.archive(f.second, 0), Err(Error::Denied));
    let review = f.review(0, Change::Yank { yanked: true });
    assert_eq!(review.remaining_capacity(), MAX_POLICY_EVENTS - 2);
}
