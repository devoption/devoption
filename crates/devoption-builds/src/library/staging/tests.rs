use super::*;
use crate::{BuildPlan, source_snapshot::tests::ProofFixture};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

fn registry(index: &str) -> HostedRegistry {
    HostedRegistry::new(RegistryId::new("team").unwrap(), index).unwrap()
}
fn source() -> StagedSource {
    source_version("0.1.0")
}
fn source_version(version: &str) -> StagedSource {
    let (catalog, mut input) = crate::tests::fixture();
    let original = ProofFixture::library("rlib").verify().unwrap();
    let files: Vec<_> = original
        .files()
        .iter()
        .map(|file| {
            let bytes = if matches!(file.path(), "Cargo.toml" | "Cargo.lock") {
                String::from_utf8(file.bytes().to_vec())
                    .unwrap()
                    .replace("0.1.0", version)
                    .into_bytes()
            } else {
                file.bytes().to_vec()
            };
            (file.path(), bytes)
        })
        .collect();
    input.source = ProofFixture::with_files(
        &files
            .iter()
            .map(|(path, bytes)| (*path, bytes.as_slice()))
            .collect::<Vec<_>>(),
    )
    .verify()
    .unwrap();
    input.commit = input.source.commit().into();
    input.dependencies = devoption_registry::CargoMaterialization::new(
        input.source.request().clone(),
        Sha256::digest(b"vendor"),
        1,
    );
    input.package = input.source.package().into();
    input.binary = input.package.clone();
    let plan = BuildPlan::validate(input, &catalog).unwrap();
    StagedSource::from_plan(
        registry("sparse+https://packages.example.test/index/"),
        &plan,
    )
    .unwrap()
}
fn directory() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    temp
}
fn open(temp: &tempfile::TempDir) -> SourceStore {
    SourceStore::open(temp.path().canonicalize().unwrap().to_str().unwrap()).unwrap()
}
fn record_path(temp: &tempfile::TempDir, source: &StagedSource) -> std::path::PathBuf {
    temp.path().join(source.coordinates.slot())
}

#[test]
fn durable_exact_source_provenance_and_idempotent_retry() {
    let source = source();
    let temp = directory();
    let mut store = open(&temp);
    assert!(store.read(source.coordinates()).unwrap().is_none());
    assert_eq!(store.stage(&source), Ok(StageOutcome::Stored));
    let path = record_path(&temp, &source);
    let metadata = fs::metadata(&path).unwrap();
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    assert_eq!(metadata.nlink(), 1);
    let saved = fs::read(&path).unwrap();
    assert_eq!(store.stage(&source), Ok(StageOutcome::AlreadyPresent));
    assert_eq!(metadata.ino(), fs::metadata(&path).unwrap().ino());
    drop(store);
    let store = open(&temp);
    let retained = store.read(source.coordinates()).unwrap().unwrap();
    assert!(retained == source);
    assert_eq!(retained.project(), source.project());
    assert_eq!(retained.repository(), source.repository());
    assert_eq!(retained.commit(), source.commit());
    assert!(retained.source_sha256() == source.source_sha256());
    assert!(retained.archive_sha256() == Sha256::digest(retained.archive()));
    assert_eq!(retained.archive_bytes(), retained.archive().len() as u64);
    assert_eq!(saved, fs::read(path).unwrap());
    assert!(!temp.path().join(PENDING).exists());
}

#[test]
fn staging_requires_a_library_plan_and_valid_destination_coordinates() {
    let (catalog, input) = crate::tests::fixture();
    let plan = BuildPlan::validate(input, &catalog).unwrap();
    let binding = registry("sparse+https://packages.example.test/index/");
    assert_eq!(
        StagedSource::from_plan(binding.clone(), &plan).err(),
        Some(Error::Invalid)
    );
    for (name, version) in [
        ("../escape", "1.0.0"),
        ("valid", "^1.0.0"),
        ("valid", "1.0"),
        ("valid", "1.0.0/path"),
    ] {
        assert_eq!(
            Coordinates::new(binding.clone(), name, version).err(),
            Some(Error::Invalid)
        );
    }
}

#[test]
fn occupied_versions_refuse_changed_bytes_provenance_aliases_and_index_rebinding() {
    let source = source();
    let temp = directory();
    let mut store = open(&temp);
    store.stage(&source).unwrap();
    let saved = fs::read(record_path(&temp, &source)).unwrap();
    for mutation in 0..8 {
        let mut changed = source.clone();
        match mutation {
            0 => changed.archive[512] ^= 1,
            1 => changed.project[0] ^= 1,
            2 => changed.repository[0] ^= 1,
            3 => changed.commit = "a".repeat(40),
            4 => changed.source = Sha256::digest(b"other source"),
            5 => changed.coordinates.name = changed.coordinates.name.to_lowercase(),
            6 => changed.coordinates.version += "+another-build",
            _ => {
                changed.coordinates.registry = registry("sparse+https://other.example.test/index/")
            }
        }
        assert_eq!(store.stage(&changed), Err(Error::Conflict));
        assert_eq!(fs::read(record_path(&temp, &source)).unwrap(), saved);
    }
    let mut rebound = source.coordinates.clone();
    rebound.registry = registry("sparse+https://other.example.test/index/");
    assert_eq!(store.read(&rebound).err(), Some(Error::Conflict));
    assert!(!temp.path().join(PENDING).exists());
    let a = Coordinates::new(
        source.coordinates.registry.clone(),
        "some_name",
        "1.0.0+one",
    )
    .unwrap();
    let b = Coordinates::new(
        source.coordinates.registry.clone(),
        "Some-Name",
        "1.0.0+two",
    )
    .unwrap();
    assert_eq!(a.slot(), b.slot());
    let c = Coordinates::new(source.coordinates.registry.clone(), "some_name", "1.0.1").unwrap();
    assert_ne!(a.slot(), c.slot());
}

#[test]
fn separate_versions_and_registries_survive_reopen() {
    let first = source();
    let second = source_version("0.2.0");
    let mut other_registry = first.clone();
    other_registry.coordinates.registry = HostedRegistry::new(
        RegistryId::new("other").unwrap(),
        "https://other.example.test/index",
    )
    .unwrap();
    let temp = directory();
    let mut store = open(&temp);
    for source in [&first, &second, &other_registry] {
        assert_eq!(store.stage(source), Ok(StageOutcome::Stored));
    }
    drop(store);
    let store = open(&temp);
    for source in [&first, &second, &other_registry] {
        assert!(store.read(&source.coordinates).unwrap().as_ref() == Some(source));
    }
}

#[test]
fn corrupt_oversized_relocated_and_unsafe_records_fail_closed() {
    let source = source();
    for mutation in 0..8 {
        let temp = directory();
        let outside = directory();
        let mut store = open(&temp);
        store.stage(&source).unwrap();
        let path = record_path(&temp, &source);
        match mutation {
            0 => {
                let mut bytes = fs::read(&path).unwrap();
                bytes[12] ^= 1;
                fs::write(&path, bytes).unwrap();
            }
            1 => {
                let mut bytes = fs::read(&path).unwrap();
                bytes[512] ^= 1;
                fs::write(&path, bytes).unwrap();
            }
            2 => {
                fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_len(MAX_RECORD_BYTES as u64 + 1)
                    .unwrap();
            }
            3 => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
            4 => {
                fs::rename(&path, outside.path().join("saved")).unwrap();
                symlink(outside.path().join("saved"), &path).unwrap();
            }
            5 => fs::hard_link(&path, outside.path().join("alias")).unwrap(),
            6 => fs::rename(
                &path,
                temp.path().join(format!("{}.source", "f".repeat(64))),
            )
            .unwrap(),
            _ => {
                fs::remove_file(&path).unwrap();
                fs::create_dir(&path).unwrap();
            }
        }
        assert!(
            store.read(&source.coordinates).is_err(),
            "mutation {mutation}"
        );
        assert!(store.stage(&source).is_err(), "mutation {mutation}");
        drop(store);
        assert!(SourceStore::open(temp.path().canonicalize().unwrap().to_str().unwrap()).is_err());
    }
}

#[test]
fn partial_write_and_late_destination_conflict_preserve_evidence_without_overwrite() {
    let source = source();
    for mutation in 0..3 {
        let temp = directory();
        let mut store = open(&temp);
        let path = record_path(&temp, &source);
        let result = store.stage_using(&source, |file, bytes| {
            if mutation == 0 {
                file.write_all(&bytes[..100])?;
                return Err(std::io::Error::other("injected partial write"));
            }
            file.write_all(bytes)?;
            if mutation == 1 {
                fs::write(&path, b"concurrent destination")?;
            }
            if mutation == 2 {
                fs::rename(temp.path().join(LOCK), temp.path().join("saved-lock"))?;
                fs::write(temp.path().join(LOCK), b"")?;
                fs::set_permissions(temp.path().join(LOCK), fs::Permissions::from_mode(0o600))?;
            }
            Ok(())
        });
        assert_eq!(result, Err(Error::Uncertain));
        assert!(temp.path().join(PENDING).exists());
        assert_eq!(
            store.read(&source.coordinates).err(),
            Some(Error::Uncertain)
        );
        assert_eq!(store.stage(&source), Err(Error::Uncertain));
        if mutation == 1 {
            assert_eq!(fs::read(path).unwrap(), b"concurrent destination");
        } else {
            assert!(!path.exists());
        }
        drop(store);
        assert!(SourceStore::open(temp.path().canonicalize().unwrap().to_str().unwrap()).is_err());
        assert!(temp.path().join(PENDING).exists());
    }
}

#[test]
fn lock_pending_and_directory_safety_are_enforced() {
    let temp = directory();
    let store = open(&temp);
    assert_eq!(
        SourceStore::open(temp.path().canonicalize().unwrap().to_str().unwrap()).err(),
        Some(Error::Busy)
    );
    drop(store);
    fs::write(temp.path().join(PENDING), b"interrupted").unwrap();
    assert_eq!(
        SourceStore::open(temp.path().canonicalize().unwrap().to_str().unwrap()).err(),
        Some(Error::Uncertain)
    );
    assert_eq!(fs::read(temp.path().join(PENDING)).unwrap(), b"interrupted");
    fs::remove_file(temp.path().join(PENDING)).unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        SourceStore::open(temp.path().canonicalize().unwrap().to_str().unwrap()).err(),
        Some(Error::Unsafe)
    );
    assert!(SourceStore::open("relative").is_err());
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let parent = directory();
    symlink(temp.path(), parent.path().join("alias")).unwrap();
    assert!(SourceStore::open(parent.path().join("alias").to_str().unwrap()).is_err());
}

#[test]
fn bounded_codec_rejects_truncation_tamper_and_trailing_data() {
    let source = source();
    let encoded = source.encode();
    assert!(StagedSource::decode(&encoded).unwrap() == source);
    for end in 0..encoded.len() {
        assert!(StagedSource::decode(&encoded[..end]).is_err());
    }
    let mut extended = encoded.clone();
    extended.push(0);
    assert!(StagedSource::decode(&extended).is_err());
    for field in 0..4 {
        let mut invalid = source.clone();
        match field {
            0 => invalid.commit = "bad".into(),
            1 => invalid.coordinates.version = "*".into(),
            2 => invalid.archive.clear(),
            _ => invalid.archive.resize(MAX_PACKAGE_BYTES + 1, 0),
        }
        assert!(StagedSource::decode(&invalid.encode()).is_err());
    }
}

#[test]
fn capacity_refuses_new_versions_but_allows_exact_retry_without_mutation() {
    let source = source();
    let temp = directory();
    let mut store = open(&temp);
    for n in 0..MAX_STAGED_PACKAGES {
        let mut version = source.clone();
        version.coordinates.version = format!("1.0.{n}");
        assert_eq!(store.stage(&version), Ok(StageOutcome::Stored));
    }
    assert_eq!(store.stage(&source), Err(Error::Capacity));
    let mut first = source.clone();
    first.coordinates.version = "1.0.0".into();
    assert_eq!(store.stage(&first), Ok(StageOutcome::AlreadyPresent));
    assert!(!temp.path().join(PENDING).exists());
    drop(store);
    assert_eq!(open(&temp).inventory().unwrap().len(), MAX_STAGED_PACKAGES);
}

#[test]
fn falsely_successful_short_write_and_unicode_inventory_fail_before_commit() {
    let source = source();
    let temp = directory();
    let mut store = open(&temp);
    assert_eq!(
        store.stage_using(&source, |file, bytes| file.write_all(&bytes[..100])),
        Err(Error::Uncertain)
    );
    assert!(!record_path(&temp, &source).exists());
    assert_eq!(fs::read(temp.path().join(PENDING)).unwrap().len(), 100);
    drop(store);
    let temp = directory();
    let store = open(&temp);
    // A non-ASCII filename with the expected byte length is still unsafe.
    fs::write(
        temp.path().join(format!("{}é.source", "a".repeat(62))),
        b"unexpected",
    )
    .unwrap();
    assert_eq!(store.inventory().err(), Some(Error::Unsafe));
}

#[test]
fn retained_directory_descriptor_does_not_follow_a_replacement_path() {
    let source = source();
    let parent = directory();
    let path = parent.path().canonicalize().unwrap().join("stage");
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let mut store = SourceStore::open(path.to_str().unwrap()).unwrap();
    let moved = path.with_file_name("retained");
    fs::rename(&path, &moved).unwrap();
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(store.stage(&source), Ok(StageOutcome::Stored));
    assert!(moved.join(source.coordinates.slot()).is_file());
    assert!(!path.join(source.coordinates.slot()).exists());
    let replacement = SourceStore::open(path.to_str().unwrap()).unwrap();
    assert!(replacement.read(source.coordinates()).unwrap().is_none());
    assert!(store.read(source.coordinates()).unwrap().as_ref() == Some(&source));
}
