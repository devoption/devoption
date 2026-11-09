use super::*;
use crate::library::cargo_package::tests::{base, helper, helper_for};
use devoption_registry::{HostedRegistry, RegistryId};
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
};

fn directory() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    temp
}
fn path(temp: &tempfile::TempDir) -> String {
    temp.path().canonicalize().unwrap().to_str().unwrap().into()
}
fn open(temp: &tempfile::TempDir) -> PackageStore {
    PackageStore::open(&path(temp)).unwrap()
}
fn key(package: &CargoPackage) -> String {
    <CargoPackage as Record>::key(package)
}
fn write_private(path: impl AsRef<std::path::Path>, bytes: &[u8]) {
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn exact_archive_index_and_provenance_survive_reopen_without_rewrite() {
    let temp = directory();
    let package = base();
    let mut store = open(&temp);
    assert!(
        store
            .read(package.source().coordinates())
            .unwrap()
            .is_none()
    );
    assert_eq!(store.stage(&package), Ok(StageOutcome::Stored));
    let metadata = fs::metadata(temp.path().join(key(&package))).unwrap();
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    assert_eq!(store.stage(&package), Ok(StageOutcome::AlreadyPresent));
    drop(store);
    let store = open(&temp);
    let retained = store.read(package.source().coordinates()).unwrap().unwrap();
    assert!(retained == package);
    assert_eq!(retained.archive(), package.archive());
    assert_eq!(retained.index_line(), package.index_line());
    assert_eq!(retained.index_path(), package.index_path());
    assert_eq!(
        fs::metadata(temp.path().join(key(&package))).unwrap().ino(),
        metadata.ino()
    );
}

#[test]
fn immutable_versions_refuse_rebinding_and_allow_distinct_versions_and_registries() {
    let temp = directory();
    let first = helper("0.1.0", 0);
    let mut store = open(&temp);
    store.stage(&first).unwrap();
    let saved = fs::read(temp.path().join(key(&first))).unwrap();
    let changed = helper("0.1.0", 42);
    assert_eq!(store.stage(&changed), Err(Error::Conflict));
    let rebound = helper_for(
        HostedRegistry::new(
            RegistryId::new("team").unwrap(),
            "sparse+https://other.example.test/index/",
        )
        .unwrap(),
        "0.1.0",
        0,
    );
    assert_eq!(store.stage(&rebound), Err(Error::Conflict));
    assert_eq!(
        store.read(rebound.source().coordinates()).err(),
        Some(Error::Conflict)
    );
    let alias = Coordinates::new(
        first.source().coordinates().registry().clone(),
        "Shared_Helper",
        "0.1.0+alias",
    )
    .unwrap();
    assert_eq!(store.read(&alias).err(), Some(Error::Conflict));
    let second = helper("0.2.0", 1);
    let other = helper_for(
        HostedRegistry::new(
            RegistryId::new("other").unwrap(),
            "sparse+https://other.example.test/index/",
        )
        .unwrap(),
        "0.1.0",
        0,
    );
    for package in [&second, &other] {
        assert_eq!(store.stage(package), Ok(StageOutcome::Stored));
    }
    drop(store);
    let store = open(&temp);
    for package in [&first, &second, &other] {
        assert!(store.read(package.source().coordinates()).unwrap().as_ref() == Some(package));
    }
    assert_eq!(fs::read(temp.path().join(key(&first))).unwrap(), saved);
}

#[test]
fn candidates_cannot_mix_with_source_store_or_unsafe_inventory() {
    let package = base();
    for mutation in 0..6 {
        let temp = directory();
        let mut store = open(&temp);
        store.stage(&package).unwrap();
        drop(store);
        let target = temp.path().join(key(&package));
        match mutation {
            0 => {
                let mut bytes = fs::read(&target).unwrap();
                bytes[40] ^= 1;
                fs::write(&target, bytes).unwrap();
            }
            1 => fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap(),
            2 => fs::hard_link(&target, temp.path().join("extra-link")).unwrap(),
            3 => {
                fs::remove_file(&target).unwrap();
                symlink("missing", &target).unwrap();
            }
            4 => write_private(temp.path().join("unexpected"), b""),
            _ => fs::rename(
                &target,
                temp.path().join(format!("{}.cargo", "a".repeat(64))),
            )
            .unwrap(),
        }
        assert!(
            PackageStore::open(&path(&temp)).is_err(),
            "mutation {mutation}"
        );
    }
    let temp = directory();
    let mut source = super::super::staging::SourceStore::open(&path(&temp)).unwrap();
    source.stage(package.source()).unwrap();
    drop(source);
    assert!(PackageStore::open(&path(&temp)).is_err());
    let temp = directory();
    let mut candidates = open(&temp);
    candidates.stage(&package).unwrap();
    drop(candidates);
    assert!(super::super::staging::SourceStore::open(&path(&temp)).is_err());
}

#[test]
fn codec_refuses_truncation_oversize_digest_and_mismatched_index() {
    let package = base();
    let bytes = package.encode();
    for end in [0, 8, 40, 100, bytes.len() - 1] {
        assert!(CargoPackage::decode(&bytes[..end]).is_err());
    }
    let mut changed = bytes.clone();
    changed[20] ^= 1;
    assert!(CargoPackage::decode(&changed).is_err());
    let oversized = vec![0; <CargoPackage as Record>::MAX_BYTES + 1];
    assert!(CargoPackage::decode(&oversized).is_err());
    // Recompute the outer corruption checksum to exercise cross-field checks.
    let needle = b"\"yanked\":false";
    let at = bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .unwrap();
    let mut changed = bytes.clone();
    changed[at + needle.len() - 5..at + needle.len()].copy_from_slice(b"true ");
    let n = changed.len() - 32;
    let digest = devoption_registry::Sha256::digest(&changed[..n]);
    changed[n..].copy_from_slice(digest.bytes());
    assert!(CargoPackage::decode(&changed).is_err());
    let at = bytes.windows(7).rposition(|w| w == b"\"cksum\"").unwrap() + 9;
    let mut changed = bytes.clone();
    changed[at] = if changed[at] == b'a' { b'b' } else { b'a' };
    let n = changed.len() - 32;
    let digest = devoption_registry::Sha256::digest(&changed[..n]);
    changed[n..].copy_from_slice(digest.bytes());
    assert!(CargoPackage::decode(&changed).is_err());
}

#[test]
fn complete_interrupted_writes_recover_only_the_exact_candidate() {
    for linked in [false, true] {
        let temp = directory();
        let package = base();
        let mut store = open(&temp);
        assert_eq!(
            store.0.stage_using(&package, |file, bytes| {
                file.write_all(bytes)?;
                Err(std::io::Error::other("injected after complete write"))
            }),
            Err(Error::Uncertain)
        );
        assert_eq!(store.stage(&package), Err(Error::Uncertain));
        drop(store);
        let pending = temp.path().join(<CargoPackage as Record>::PENDING);
        if linked {
            fs::hard_link(&pending, temp.path().join(key(&package))).unwrap();
        }
        assert_eq!(
            PackageStore::open(&path(&temp)).err(),
            Some(Error::Uncertain)
        );
        let wrong = helper("0.1.0", 0);
        assert!(PackageStore::recover(&path(&temp), &wrong).is_err());
        assert!(pending.exists());
        assert_eq!(
            PackageStore::recover(&path(&temp), &package),
            Ok(StageOutcome::Stored)
        );
        assert!(!pending.exists());
        assert_eq!(
            PackageStore::recover(&path(&temp), &package),
            Ok(StageOutcome::AlreadyPresent)
        );
        assert!(
            open(&temp)
                .read(package.source().coordinates())
                .unwrap()
                .as_ref()
                == Some(&package)
        );
    }
}

#[test]
fn recovery_preserves_partial_conflicting_and_unsafe_evidence_and_obeys_lock() {
    let package = base();
    for mutation in 0..7 {
        let temp = directory();
        let pending = temp.path().join(<CargoPackage as Record>::PENDING);
        write_private(&pending, &package.encode());
        let target = temp.path().join(key(&package));
        match mutation {
            0 => write_private(&pending, b"partial"),
            1 => write_private(&target, b"existing destination"),
            2 => write_private(&target, &package.encode()), // separate inode is not a recoverable commit
            3 => fs::hard_link(&pending, temp.path().join("wrong-link")).unwrap(),
            4 => fs::set_permissions(&pending, fs::Permissions::from_mode(0o644)).unwrap(),
            5 => {
                fs::remove_file(&pending).unwrap();
                symlink("missing", &pending).unwrap();
            }
            _ => write_private(temp.path().join("unexpected"), b"keep"),
        }
        let before = fs::read(&pending).ok();
        let final_before = fs::read(&target).ok();
        assert!(
            PackageStore::recover(&path(&temp), &package).is_err(),
            "mutation {mutation}"
        );
        assert_eq!(fs::read(&pending).ok(), before);
        assert_eq!(fs::read(&target).ok(), final_before);
        assert!(fs::symlink_metadata(&pending).is_ok());
    }
    let temp = directory();
    let store = open(&temp);
    assert_eq!(
        PackageStore::recover(&path(&temp), &package),
        Err(Error::Busy)
    );
    drop(store);
    assert_eq!(
        PackageStore::recover(&path(&temp), &package),
        Err(Error::Uncertain)
    );
}

#[test]
fn capacity_retains_existing_versions_and_exact_retry() {
    let temp = directory();
    let mut store = open(&temp);
    let first = helper("0.0.0", 0);
    store.stage(&first).unwrap();
    for n in 1..MAX_STORED_PACKAGES {
        store.stage(&helper(&format!("0.{n}.0"), 0)).unwrap();
    }
    assert_eq!(store.stage(&helper("1.0.0", 1)), Err(Error::Capacity));
    assert_eq!(store.stage(&first), Ok(StageOutcome::AlreadyPresent));
    drop(store);
    assert!(
        open(&temp)
            .read(first.source().coordinates())
            .unwrap()
            .as_ref()
            == Some(&first)
    );
}

#[test]
#[ignore = "child-only abrupt-exit fixture, exercised by recovery_after_abrupt_process_exit"]
fn manual_candidate_crash() {
    let path = std::env::var("DEVOPTION_CANDIDATE_CRASH_PATH").expect("child fixture path");
    let linked = std::env::var("DEVOPTION_CANDIDATE_CRASH_LINKED").unwrap() == "yes";
    let expected = std::env::var("DEVOPTION_CANDIDATE_CRASH_EXPECTED").unwrap();
    let package = CargoPackage::decode(&fs::read(expected).unwrap()).unwrap();
    let mut store = PackageStore::open(&path).unwrap();
    let _ = store.0.stage_using(&package, |file, bytes| {
        file.write_all(bytes)?;
        file.sync_all()?;
        if linked {
            fs::hard_link(
                std::path::Path::new(&path).join(<CargoPackage as Record>::PENDING),
                std::path::Path::new(&path).join(key(&package)),
            )?;
        }
        // No Rust destructors or normal writer cleanup, as at a process crash.
        std::process::exit(42);
    });
    panic!("fixture did not terminate");
}

#[test]
fn recovery_after_abrupt_process_exit() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for linked in [false, true] {
        let temp = directory();
        let package = base();
        let mut expected = tempfile::NamedTempFile::new().unwrap();
        expected.write_all(&package.encode()).unwrap();
        expected.flush().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "library::package_store::tests::manual_candidate_crash",
                "--ignored",
            ])
            .env_clear()
            .env("DEVOPTION_CANDIDATE_CRASH_PATH", path(&temp))
            .env("DEVOPTION_CANDIDATE_CRASH_EXPECTED", expected.path())
            .env(
                "DEVOPTION_CANDIDATE_CRASH_LINKED",
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
                panic!("crash fixture timed out");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(42));
        assert_eq!(
            PackageStore::open(&path(&temp)).err(),
            Some(Error::Uncertain)
        );
        assert_eq!(
            PackageStore::recover(&path(&temp), &package),
            Ok(StageOutcome::Stored)
        );
        assert!(
            open(&temp)
                .read(package.source().coordinates())
                .unwrap()
                .as_ref()
                == Some(&package)
        );
    }
}
