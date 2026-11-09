use super::super::tests::{Fixture, OTHER, OWNER};
use super::*;
use std::fs;

fn entry(f: &Fixture, project: ProjectId) -> Result<Vec<u8>, Error> {
    let p = &f.packages[1];
    f.policy.index_entry(
        OWNER,
        project,
        &f.catalog,
        &f.candidates,
        p.source().coordinates().registry(),
        p.index_path(),
    )
}
fn publish(f: &mut Fixture, at: usize, readers: Vec<ProjectId>) {
    f.change(at, Change::Publish { readers });
}
#[test]
fn only_approved_project_versions_aggregate_and_yanks_revocations_are_current() {
    let mut f = Fixture::new();
    assert_eq!(entry(&f, f.first), Err(Error::Denied));
    let first = f.first;
    let second = f.second;
    // Publish in reverse order to prove output order is independent of events.
    publish(&mut f, 2, vec![second]);
    publish(&mut f, 1, vec![first]);
    assert_eq!(entry(&f, first).unwrap(), f.line(first, 1).unwrap());
    assert_eq!(entry(&f, second).unwrap(), f.line(second, 2).unwrap());
    let mut expected = f.line(f.source, 1).unwrap();
    expected.extend(f.line(f.source, 2).unwrap());
    assert_eq!(entry(&f, f.source).unwrap(), expected);
    f.change(
        2,
        Change::Readers {
            readers: vec![first, second],
        },
    );
    assert_eq!(entry(&f, first).unwrap(), expected);
    f.change(1, Change::Yank { yanked: true });
    let rows: Vec<Value> = std::str::from_utf8(&entry(&f, first).unwrap())
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["vers"], "0.1.0");
    assert_eq!(rows[0]["yanked"], true);
    assert_eq!(rows[1]["vers"], "0.2.0");
    assert_eq!(rows[1]["yanked"], false);
    assert_eq!(f.archive(first, 1).unwrap(), f.packages[1].archive());
    f.change(
        1,
        Change::Readers {
            readers: vec![second],
        },
    );
    assert_eq!(entry(&f, first).unwrap(), f.line(first, 2).unwrap());
    f.change(
        2,
        Change::Readers {
            readers: vec![second],
        },
    );
    assert_eq!(entry(&f, first), Err(Error::Denied));
    let path = f.path.clone();
    drop(f.policy);
    f.policy = Publications::open(&path, OWNER).unwrap();
    assert_eq!(entry(&f, first), Err(Error::Denied));
    assert_eq!(entry(&f, second).unwrap().split(|b| *b == b'\n').count(), 3);
}
#[test]
fn canonical_path_registry_and_actor_are_required_without_namespace_aliases() {
    let mut f = Fixture::new();
    let first = f.first;
    publish(&mut f, 1, vec![first]);
    let p = &f.packages[1];
    let registry = p.source().coordinates().registry();
    for path in [
        "",
        "config.json",
        "shared-helper",
        "SH/AR/SHARED-HELPER",
        "sh/ar/Shared-Helper",
        "sh/ar/shared_helper",
        "wrong/path/shared-helper",
        "sh/ar/shared-helper/",
        "sh/ar/../shared-helper",
        "/sh/ar/shared-helper",
        "sh/ar/shared-helper\0",
        "sh/ar/unknown",
    ] {
        assert_eq!(
            f.policy
                .index_entry(OWNER, first, &f.catalog, &f.candidates, registry, path),
            Err(Error::Denied),
            "{path:?}"
        );
    }
    for path in ["a".repeat(1000), "sh/ar/é".into()] {
        assert_eq!(
            f.policy
                .index_entry(OWNER, first, &f.catalog, &f.candidates, registry, &path),
            Err(Error::Denied)
        );
    }
    for registry in [
        HostedRegistry::new(RegistryId::new("other").unwrap(), registry.index()).unwrap(),
        HostedRegistry::new(
            registry.id().clone(),
            "sparse+https://other.example.test/index/",
        )
        .unwrap(),
    ] {
        assert_eq!(
            f.policy.index_entry(
                OWNER,
                first,
                &f.catalog,
                &f.candidates,
                &registry,
                p.index_path()
            ),
            Err(Error::Denied)
        );
    }
    assert_eq!(
        f.policy.index_entry(
            OTHER,
            first,
            &f.catalog,
            &f.candidates,
            registry,
            p.index_path()
        ),
        Err(Error::Denied)
    );
    assert_eq!(
        entry(&f, ProjectId::from_bytes([0; 16])),
        Err(Error::Denied)
    );
    assert_eq!(entry(&f, f.second), Err(Error::Denied));
}
#[test]
fn unavailable_or_corrupt_included_version_refuses_the_whole_entry() {
    let mut f = Fixture::new();
    let first = f.first;
    let second = f.second;
    publish(&mut f, 1, vec![first]);
    publish(&mut f, 2, vec![second]);
    let file = f._root.path().join("candidates").join(f.packages[2].key());
    let bytes = fs::read(&file).unwrap();
    fs::write(&file, b"corrupt private version").unwrap();
    // The existing store-wide integrity boundary remains in force even for
    // hidden candidates: no corrupt inventory is silently skipped.
    assert!(entry(&f, first).is_err());
    assert!(entry(&f, second).is_err());
    assert!(entry(&f, f.source).is_err());
    fs::write(&file, &bytes).unwrap();
    f.change(
        2,
        Change::Readers {
            readers: vec![first, second],
        },
    );
    fs::remove_file(&file).unwrap();
    assert_eq!(entry(&f, first), Err(Error::Unavailable));
    fs::write(&file, &bytes).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(entry(&f, first).is_ok());
}

#[test]
fn complete_response_budget_and_late_failure_do_not_return_a_prefix() {
    let mut f = Fixture::new();
    let first = f.first;
    publish(&mut f, 1, vec![first]);
    let row = f.line(first, 1).unwrap();
    let count = MAX_INDEX_ENTRY_BYTES / row.len();
    let bytes = assemble((0..count).map(|_| Ok(row.clone()))).unwrap();
    assert_eq!(bytes.len(), count * row.len());
    assert!(bytes.len() <= MAX_INDEX_ENTRY_BYTES);
    assert_eq!(
        assemble((0..=count).map(|_| Ok(row.clone()))),
        Err(Error::Capacity)
    );
    assert_eq!(
        assemble([Ok(row), Err(Error::Integrity)]),
        Err(Error::Integrity)
    );
}
