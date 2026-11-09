use super::super::tests::{Fixture, OTHER, OWNER};
use super::*;
use crate::source_snapshot::tests::ProofFixture;
use devoption_registry::{HostedRegistries, RegistryId};
use std::fs;
const ARCHIVE: &[u8] =
    include_bytes!("../../../../../devoption-registry/tests/fixtures/registry/itoa-1.0.18.crate");
const INDEX: &str = "sparse+https://packages.example.test/index/";
fn checksum(bytes: &[u8]) -> String {
    super::super::hex(Sha256::digest(bytes).bytes())
}
fn source(f: &Fixture, helper: usize, bad_edges: bool) -> SourceSnapshot {
    let version = f.packages[helper].source().coordinates().version();
    let direct = if bad_edges {
        "'shared-helper','private-base','itoa'"
    } else {
        "'shared-helper'"
    };
    let extra = if bad_edges {
        "private-base={version='=0.1.0',registry='team'}\nitoa='=1.0.18'\n"
    } else {
        ""
    };
    let manifest = format!(
        "[package]\nname='consumer'\nversion='0.1.0'\nedition='2024'\nbuild=false\n[dependencies]\nshared-helper={{version='={version}',registry='team'}}\n{extra}"
    );
    let lock = format!(
        "version=4\n[[package]]\nname='consumer'\nversion='0.1.0'\ndependencies=[{direct}]\n[[package]]\nname='shared-helper'\nversion='{version}'\nsource='{INDEX}'\nchecksum='{}'\ndependencies=[{}]\n[[package]]\nname='private-base'\nversion='0.1.0'\nsource='{INDEX}'\nchecksum='{}'\n[[package]]\nname='itoa'\nversion='1.0.18'\nsource='registry+{PUBLIC_INDEX}'\nchecksum='{}'\n",
        checksum(f.packages[helper].archive()),
        if bad_edges {
            ""
        } else {
            "'private-base','itoa'"
        },
        checksum(f.packages[0].archive()),
        checksum(ARCHIVE)
    );
    ProofFixture::with_files(&[
        ("Cargo.toml", manifest.as_bytes()),
        ("Cargo.lock", lock.as_bytes()),
        ("src/main.rs", b"fn main() {}\n"),
    ])
    .verify_with_registries(
        &HostedRegistries::new(vec![
            HostedRegistry::new(RegistryId::new("team").unwrap(), INDEX).unwrap(),
        ])
        .unwrap(),
    )
    .unwrap()
}
fn public(source: &SourceSnapshot) -> PublicArchive<'_> {
    PublicArchive {
        package: source
            .request()
            .packages()
            .iter()
            .find(|p| p.origin == CargoOrigin::CratesIo)
            .unwrap(),
        bytes: ARCHIVE,
    }
}
fn grant(f: &mut Fixture, at: usize) {
    f.change(
        at,
        Change::Publish {
            readers: vec![f.first, f.second],
        },
    );
}
fn acquire(f: &Fixture, source: &SourceSnapshot) -> Result<LockedArchives, Error> {
    f.policy.locked_archives(
        OWNER,
        f.first,
        &f.catalog,
        &f.candidates,
        source,
        &[public(source)],
    )
}
#[test]
fn complete_archives_are_immutable_exact_and_revalidated_against_live_policy() {
    let mut f = Fixture::new();
    let source = source(&f, 1, false);
    grant(&mut f, 0);
    grant(&mut f, 1);
    let captured = acquire(&f, &source).unwrap();
    assert_eq!(captured.archives().len(), source.request().packages().len());
    assert!(captured.request() == source.request());
    assert_eq!(captured.project(), f.first);
    assert!(captured.source_sha256() == source.sha256());
    for archive in captured.archives() {
        assert!(Sha256::digest(archive.bytes()) == archive.package().archive_sha256);
        assert_eq!(
            archive.index_line().is_some(),
            archive.package().origin != CargoOrigin::CratesIo
        );
    }
    let repeated = acquire(&f, &source).unwrap();
    assert!(captured.fingerprint() == repeated.fingerprint());
    f.policy
        .revalidate_archives(OWNER, &f.catalog, &f.candidates, &source, &captured)
        .unwrap();
    let mut changed = f.catalog.clone();
    changed.create_project("Changed context").unwrap();
    assert_eq!(
        f.policy
            .revalidate_archives(OWNER, &changed, &f.candidates, &source, &captured),
        Err(Error::Changed)
    );
    assert_eq!(
        f.policy
            .revalidate_archives(OTHER, &f.catalog, &f.candidates, &source, &captured),
        Err(Error::Denied)
    );
    let path = f.path.clone();
    drop(f.policy);
    f.policy = Publications::open(&path, OWNER).unwrap();
    f.policy
        .revalidate_archives(OWNER, &f.catalog, &f.candidates, &source, &captured)
        .unwrap();
    f.change(1, Change::Yank { yanked: true });
    assert_eq!(
        f.policy
            .revalidate_archives(OWNER, &f.catalog, &f.candidates, &source, &captured),
        Err(Error::Changed)
    );
    let yanked = acquire(&f, &source).unwrap();
    let row: Value = serde_json::from_slice(
        yanked
            .archives()
            .iter()
            .find(|a| a.package().name == "shared-helper")
            .unwrap()
            .index_line()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(row["yanked"], true);
    assert!(captured.fingerprint() != yanked.fingerprint());
    f.change(
        0,
        Change::Readers {
            readers: vec![f.second],
        },
    );
    assert_eq!(acquire(&f, &source).err(), Some(Error::Denied));
    assert_eq!(
        f.policy
            .revalidate_archives(OWNER, &f.catalog, &f.candidates, &source, &yanked),
        Err(Error::Denied)
    );
    // Revocation cannot rewrite or retract already owned observations.
    assert_eq!(captured.archives().len(), 3);
}
#[test]
fn every_hosted_transitive_grant_is_required_and_public_bytes_cannot_substitute() {
    let mut f = Fixture::new();
    let source = source(&f, 1, false);
    assert_eq!(acquire(&f, &source).err(), Some(Error::Denied));
    grant(&mut f, 1); // The helper's grant does not grant its private base.
    assert_eq!(acquire(&f, &source).err(), Some(Error::Denied));
    let hosted = source
        .request()
        .packages()
        .iter()
        .find(|p| p.origin != CargoOrigin::CratesIo)
        .unwrap();
    assert_eq!(
        f.policy
            .locked_archives(
                OWNER,
                f.first,
                &f.catalog,
                &f.candidates,
                &source,
                &[
                    public(&source),
                    PublicArchive {
                        package: hosted,
                        bytes: f.packages[0].archive()
                    }
                ]
            )
            .err(),
        Some(Error::Invalid)
    );
    grant(&mut f, 0);
    assert_eq!(
        f.policy
            .locked_archives(OWNER, f.first, &f.catalog, &f.candidates, &source, &[])
            .err(),
        Some(Error::Unavailable)
    );
    assert_eq!(
        f.policy
            .locked_archives(
                OWNER,
                f.first,
                &f.catalog,
                &f.candidates,
                &source,
                &[public(&source), public(&source)]
            )
            .err(),
        Some(Error::Invalid)
    );
    for (bytes, expected) in [
        (b"".as_slice(), Error::Unavailable),
        (b"corrupt public archive".as_slice(), Error::Integrity),
    ] {
        assert_eq!(
            f.policy
                .locked_archives(
                    OWNER,
                    f.first,
                    &f.catalog,
                    &f.candidates,
                    &source,
                    &[PublicArchive {
                        package: public(&source).package,
                        bytes
                    }]
                )
                .err(),
            Some(expected)
        );
    }
    let too_large = vec![0; MAX_DEPENDENCY_ARCHIVE_BYTES + 1];
    assert_eq!(
        f.policy
            .locked_archives(
                OWNER,
                f.first,
                &f.catalog,
                &f.candidates,
                &source,
                &[PublicArchive {
                    package: public(&source).package,
                    bytes: &too_large
                }]
            )
            .err(),
        Some(Error::Capacity)
    );
    let file = f._root.path().join("candidates").join(f.packages[0].key());
    let saved = fs::read(&file).unwrap();
    fs::write(&file, b"corrupt hosted archive").unwrap();
    assert!(acquire(&f, &source).is_err());
    fs::write(&file, saved).unwrap();
    fs::remove_file(&file).unwrap();
    assert_eq!(acquire(&f, &source).err(), Some(Error::Unavailable));
}
#[test]
fn hosted_index_edges_must_equal_the_consumer_lock_not_just_appear_in_the_flat_set() {
    let mut f = Fixture::new();
    grant(&mut f, 0);
    grant(&mut f, 1);
    // All three nodes are reachable as direct root dependencies; only the
    // helper's outgoing edges were dishonestly omitted by this consumer lock.
    let wrong = source(&f, 1, true);
    assert_eq!(wrong.request().packages().len(), 3);
    assert_eq!(acquire(&f, &wrong).err(), Some(Error::Integrity));
    let original = source(&f, 1, false);
    let captured = acquire(&f, &original).unwrap();
    grant(&mut f, 2);
    assert_eq!(
        f.policy
            .revalidate_archives(OWNER, &f.catalog, &f.candidates, &original, &captured),
        Err(Error::Changed)
    );
    let old = acquire(&f, &original).unwrap();
    let update = source(&f, 2, false);
    let newer = acquire(&f, &update).unwrap();
    assert!(old.fingerprint() != newer.fingerprint());
    assert_eq!(
        old.archives()
            .iter()
            .find(|a| a.package().name == "shared-helper")
            .unwrap()
            .bytes(),
        f.packages[1].archive()
    );
    assert_eq!(
        f.policy
            .revalidate_archives(OWNER, &f.catalog, &f.candidates, &update, &old),
        Err(Error::Changed)
    );
}
#[test]
fn aggregate_payload_budget_is_inclusive_and_error_does_not_advance_it() {
    let mut bytes = MAX_LOCKED_ARCHIVE_BYTES - ARCHIVE.len();
    verify_archive(ARCHIVE, Sha256::digest(ARCHIVE), &mut bytes).unwrap();
    assert_eq!(bytes, MAX_LOCKED_ARCHIVE_BYTES);
    assert_eq!(budget(1, &mut bytes), Err(Error::Capacity));
    assert_eq!(bytes, MAX_LOCKED_ARCHIVE_BYTES);
}

#[test]
fn identical_public_and_hosted_coordinates_require_separate_exact_archives_and_grants() {
    use crate::library::cargo_package::{CargoPackage, tests::plan};
    let mut f = Fixture::new();
    let make = |plan: crate::BuildPlan| {
        let mut input = plan.details().clone();
        input.project = f.source;
        input.repository = RepositoryId::from_bytes(f.packages[0].source().repository());
        CargoPackage::from_plan(
            HostedRegistry::new(RegistryId::new("team").unwrap(), INDEX).unwrap(),
            &crate::BuildPlan::validate(input, &f.catalog).unwrap(),
        )
        .unwrap()
    };
    let private = make(plan(
        "itoa",
        "1.0.18",
        "",
        "",
        "",
        "pub fn private_value() -> u32 { 7 }",
        None,
    ));
    let row = format!(
        "[[package]]\nname='itoa'\nversion='1.0.18'\nsource='{INDEX}'\nchecksum='{}'\n",
        checksum(private.archive())
    );
    let wrapper = make(plan(
        "private-wrapper",
        "0.1.0",
        "[dependencies]\nitoa={version='=1.0.18',registry='team'}\n",
        "'itoa'",
        &row,
        "pub fn value() -> u32 { itoa::private_value() }",
        None,
    ));
    for p in [&private, &wrapper] {
        f.candidates.stage(p).unwrap();
    }
    let publish = |f: &mut Fixture, p: &CargoPackage| {
        let review = f
            .policy
            .review(
                OWNER,
                &f.catalog,
                &f.candidates,
                p.source().coordinates(),
                Change::Publish {
                    readers: vec![f.first],
                },
            )
            .unwrap();
        f.policy
            .approve(OWNER, &f.catalog, &f.candidates, review)
            .unwrap();
    };
    publish(&mut f, &wrapper);
    let manifest = "[package]\nname='consumer'\nversion='0.1.0'\nedition='2024'\nbuild=false\n[dependencies]\nitoa='=1.0.18'\nprivate-wrapper={version='=0.1.0',registry='team'}\n";
    let lock = format!(
        "version=4\n[[package]]\nname='consumer'\nversion='0.1.0'\ndependencies=['private-wrapper','itoa 1.0.18 (registry+{PUBLIC_INDEX})']\n[[package]]\nname='private-wrapper'\nversion='0.1.0'\nsource='{INDEX}'\nchecksum='{}'\ndependencies=['itoa 1.0.18 ({INDEX})']\n{row}[[package]]\nname='itoa'\nversion='1.0.18'\nsource='registry+{PUBLIC_INDEX}'\nchecksum='{}'\n",
        checksum(wrapper.archive()),
        checksum(ARCHIVE)
    );
    let source = ProofFixture::with_files(&[
        ("Cargo.toml", manifest.as_bytes()),
        ("Cargo.lock", lock.as_bytes()),
        ("src/main.rs", b"fn main(){}"),
    ])
    .verify_with_registries(
        &HostedRegistries::new(vec![
            HostedRegistry::new(RegistryId::new("team").unwrap(), INDEX).unwrap(),
        ])
        .unwrap(),
    )
    .unwrap();
    assert_eq!(acquire(&f, &source).err(), Some(Error::Denied));
    publish(&mut f, &private);
    let result = acquire(&f, &source).unwrap();
    let same: Vec<_> = result
        .archives()
        .iter()
        .filter(|a| a.package().name == "itoa")
        .collect();
    assert_eq!(same.len(), 2);
    assert!(same[0].package().origin != same[1].package().origin);
    assert!(same[0].package().archive_sha256 != same[1].package().archive_sha256);
    assert!(same[0].bytes() != same[1].bytes());
}

#[test]
fn consumer_archive_checksums_and_registry_urls_cannot_be_rebound() {
    let mut f = Fixture::new();
    grant(&mut f, 0);
    grant(&mut f, 1);
    let original = source(&f, 1, false);
    for (from, to, registry, expected) in [
        (
            checksum(f.packages[1].archive()),
            "0".repeat(64),
            INDEX,
            Error::Integrity,
        ),
        (
            INDEX.into(),
            "sparse+https://other.example.test/index/".into(),
            "sparse+https://other.example.test/index/",
            Error::Denied,
        ),
    ] {
        let files: Vec<_> = original
            .files()
            .iter()
            .map(|f| {
                (
                    f.path().to_owned(),
                    if f.path() == "Cargo.lock" {
                        std::str::from_utf8(f.bytes())
                            .unwrap()
                            .replace(&from, &to)
                            .into_bytes()
                    } else {
                        f.bytes().to_vec()
                    },
                )
            })
            .collect();
        let borrowed: Vec<_> = files
            .iter()
            .map(|(p, b)| (p.as_str(), b.as_slice()))
            .collect();
        let changed = ProofFixture::with_files(&borrowed)
            .verify_with_registries(
                &HostedRegistries::new(vec![
                    HostedRegistry::new(RegistryId::new("team").unwrap(), registry).unwrap(),
                ])
                .unwrap(),
            )
            .unwrap();
        assert_eq!(acquire(&f, &changed).err(), Some(expected));
    }
}
#[test]
fn empty_lock_closure_is_exact_and_does_not_accept_an_unrequested_archive() {
    let f = Fixture::new();
    let empty = ProofFixture::with_files(&[
        (
            "Cargo.toml",
            b"[package]\nname='empty'\nversion='0.1.0'\nedition='2024'\nbuild=false\n",
        ),
        (
            "Cargo.lock",
            b"version=4\n[[package]]\nname='empty'\nversion='0.1.0'\n",
        ),
        ("src/main.rs", b"fn main() {}"),
    ])
    .verify()
    .unwrap();
    let result = f
        .policy
        .locked_archives(OWNER, f.first, &f.catalog, &f.candidates, &empty, &[])
        .unwrap();
    assert!(result.archives().is_empty());
    f.policy
        .revalidate_archives(OWNER, &f.catalog, &f.candidates, &empty, &result)
        .unwrap();
    let other = source(&f, 1, false);
    assert_eq!(
        f.policy
            .locked_archives(
                OWNER,
                f.first,
                &f.catalog,
                &f.candidates,
                &empty,
                &[public(&other)]
            )
            .err(),
        Some(Error::Invalid)
    );
}
