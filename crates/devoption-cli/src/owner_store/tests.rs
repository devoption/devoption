use super::*;
use crate::password::{SALT_BYTES, hash_password, verify_password};
use std::{
    fs::{self, DirBuilder, OpenOptions},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink},
    sync::{Arc, Barrier, OnceLock},
    thread,
};

const IDENTITY: IdentityId = IdentityId::from_bytes([1; 16]);
const PASSWORD: &str = "owner storage test fixture only";

fn private_directory() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

fn fixture_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| hash_password(PASSWORD, &[0x5a; SALT_BYTES]).unwrap())
}

fn write_fixture(path: &Path, bytes: &[u8]) {
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

#[test]
fn record_survives_reopen_without_plaintext_or_session_data() {
    let directory = private_directory();
    let store = OwnerStore::open(directory.path()).unwrap();
    assert!(store.load().unwrap().is_none());
    store.create(IDENTITY, fixture_hash()).unwrap();
    drop(store);

    let record = OwnerStore::open(directory.path())
        .unwrap()
        .load()
        .unwrap()
        .unwrap();
    assert_eq!(record.identity(), IDENTITY);
    assert_eq!(record.password_hash(), fixture_hash());
    assert!(verify_password(PASSWORD, record.password_hash()));
    let bytes = fs::read(directory.path().join(RECORD_NAME)).unwrap();
    assert!(
        !bytes
            .windows(PASSWORD.len())
            .any(|part| part == PASSWORD.as_bytes())
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    assert_eq!(
        fs::metadata(directory.path().join(RECORD_NAME))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn duplicate_setup_preserves_the_original_owner_and_cleans_temporary_files() {
    let directory = private_directory();
    let store = OwnerStore::open(directory.path()).unwrap();
    store.create(IDENTITY, fixture_hash()).unwrap();
    let before = fs::read(directory.path().join(RECORD_NAME)).unwrap();
    assert!(matches!(
        store.create(IdentityId::from_bytes([2; 16]), fixture_hash()),
        Err(StoreError::AlreadyExists)
    ));
    assert_eq!(
        fs::read(directory.path().join(RECORD_NAME)).unwrap(),
        before
    );
    assert_eq!(store.load().unwrap().unwrap().identity(), IDENTITY);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn concurrent_setup_has_one_winner_and_readers_never_see_partial_records() {
    let directory = private_directory();
    let hash = fixture_hash();
    let barrier = Arc::new(Barrier::new(9));
    let mut workers = Vec::new();
    for number in 1..=8 {
        let path = directory.path().to_path_buf();
        let barrier = Arc::clone(&barrier);
        workers.push(thread::spawn(move || {
            let identity = IdentityId::from_bytes([number; 16]);
            let store = OwnerStore::open(&path).unwrap();
            barrier.wait();
            store.create(identity, hash).map(|()| identity)
        }));
    }
    let reader = OwnerStore::open(directory.path()).unwrap();
    barrier.wait();
    for _ in 0..32 {
        if let Some(record) = reader.load().unwrap() {
            assert_eq!(record.password_hash(), hash);
        }
    }
    let mut winners = Vec::new();
    for worker in workers {
        match worker.join().unwrap() {
            Ok(identity) => winners.push(identity),
            Err(StoreError::AlreadyExists) => {}
            Err(error) => panic!("unexpected setup failure: {error}"),
        }
    }
    assert_eq!(winners.len(), 1);
    assert_eq!(reader.load().unwrap().unwrap().identity(), winners[0]);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn corrupt_records_never_mean_unconfigured_and_cannot_be_overwritten() {
    let directory = private_directory();
    let store = OwnerStore::open(directory.path()).unwrap();
    store.create(IDENTITY, fixture_hash()).unwrap();
    let path = directory.path().join(RECORD_NAME);
    let valid = fs::read(&path).unwrap();
    let mut invalid_utf8 = valid.clone();
    invalid_utf8[HEADER_BYTES] = 0xff;
    let mut unsupported = valid.clone();
    unsupported[8] = 99;
    for bytes in [
        Vec::new(),
        valid[..20].to_vec(),
        vec![0; MAX_RECORD_BYTES + 1],
        invalid_utf8,
        unsupported,
    ] {
        write_fixture(&path, &bytes);
        assert!(store.load().is_err());
        assert!(matches!(
            store.create(IDENTITY, fixture_hash()),
            Err(StoreError::AlreadyExists)
        ));
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    let mut unsupported = valid;
    unsupported[8] = 99;
    write_fixture(&path, &unsupported);
    assert!(matches!(store.load(), Err(StoreError::UnsupportedVersion)));
}

#[test]
fn invalid_password_records_are_never_persisted() {
    let directory = private_directory();
    let store = OwnerStore::open(directory.path()).unwrap();
    for hash in [
        "".to_owned(),
        PASSWORD.to_owned(),
        fixture_hash().replace("m=19456", "m=4294967295"),
    ] {
        assert!(matches!(
            store.create(IDENTITY, &hash),
            Err(StoreError::InvalidRecord)
        ));
        assert!(store.load().unwrap().is_none());
    }
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn nonprivate_permissions_are_rejected_without_repairing_them() {
    let directory = private_directory();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        OwnerStore::open(directory.path()),
        Err(StoreError::UnsafeDirectory)
    ));
    assert_eq!(
        fs::metadata(directory.path()).unwrap().mode() & 0o777,
        0o755
    );
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let store = OwnerStore::open(directory.path()).unwrap();
    store.create(IDENTITY, fixture_hash()).unwrap();
    let record = directory.path().join(RECORD_NAME);
    fs::set_permissions(&record, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(store.load(), Err(StoreError::InvalidRecord)));
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(store.load(), Err(StoreError::UnsafeDirectory)));
    assert!(matches!(
        store.create(IDENTITY, fixture_hash()),
        Err(StoreError::UnsafeDirectory)
    ));
}

#[test]
fn symlinks_and_nonregular_entries_do_not_bypass_the_store() {
    let root = private_directory();
    let private = root.path().join("private");
    DirBuilder::new().mode(0o700).create(&private).unwrap();
    let alias = root.path().join("alias");
    symlink(&private, &alias).unwrap();
    for suffix in ["alias", "alias/", "alias/.", "alias//./", "alias/././"] {
        assert!(
            OwnerStore::open(&root.path().join(suffix)).is_err(),
            "accepted symlink path: {suffix}"
        );
    }
    for suffix in [
        "private",
        "private/",
        "private/.",
        "private//./",
        "private/././",
    ] {
        assert!(
            OwnerStore::open(&root.path().join(suffix))
                .unwrap()
                .load()
                .unwrap()
                .is_none(),
            "unexpected record at real directory path: {suffix}"
        );
    }
    let store = OwnerStore::open(&private).unwrap();
    let target = root.path().join("untouched");
    write_fixture(&target, b"unrelated fixture");
    let record = private.join(RECORD_NAME);
    symlink(&target, &record).unwrap();
    assert!(store.load().is_err());
    assert!(matches!(
        store.create(IDENTITY, fixture_hash()),
        Err(StoreError::AlreadyExists)
    ));
    assert_eq!(fs::read(&target).unwrap(), b"unrelated fixture");
    fs::remove_file(&record).unwrap();
    fs::create_dir(&record).unwrap();
    assert!(matches!(store.load(), Err(StoreError::InvalidRecord)));
    assert!(matches!(
        store.create(IDENTITY, fixture_hash()),
        Err(StoreError::AlreadyExists)
    ));
}

#[test]
fn parent_components_keep_filesystem_resolution() {
    let root = private_directory();
    let private = root.path().join("private");
    let child = private.join("child");
    DirBuilder::new().mode(0o700).create(&private).unwrap();
    DirBuilder::new().mode(0o700).create(&child).unwrap();
    OwnerStore::open(&private)
        .unwrap()
        .create(IDENTITY, fixture_hash())
        .unwrap();
    symlink(&child, root.path().join("alias")).unwrap();

    // `alias/..` names the target's parent, not the alias's lexical parent.
    for suffix in [
        "alias/..",
        "alias/../",
        "alias/../.",
        "alias/../child/..//./",
    ] {
        let record = OwnerStore::open(&root.path().join(suffix))
            .unwrap()
            .load()
            .unwrap()
            .unwrap();
        assert_eq!(record.identity(), IDENTITY, "wrong directory: {suffix}");
    }
    assert!(
        OwnerStore::open(root.path())
            .unwrap()
            .load()
            .unwrap()
            .is_none()
    );
}

#[test]
fn operations_stay_anchored_when_the_directory_path_is_replaced() {
    let root = private_directory();
    let original = root.path().join("state");
    let moved = root.path().join("moved");
    DirBuilder::new().mode(0o700).create(&original).unwrap();
    let store = OwnerStore::open(&original).unwrap();
    fs::rename(&original, &moved).unwrap();
    DirBuilder::new().mode(0o700).create(&original).unwrap();
    store.create(IDENTITY, fixture_hash()).unwrap();
    assert_eq!(
        OwnerStore::open(&moved)
            .unwrap()
            .load()
            .unwrap()
            .unwrap()
            .identity(),
        IDENTITY
    );
    assert!(
        OwnerStore::open(&original)
            .unwrap()
            .load()
            .unwrap()
            .is_none()
    );
}

#[test]
fn missing_directories_are_not_created_and_removed_ones_cannot_publish() {
    let root = private_directory();
    let path = root.path().join("missing");
    assert!(OwnerStore::open(&path).is_err());
    assert!(!path.exists());
    DirBuilder::new().mode(0o700).create(&path).unwrap();
    let store = OwnerStore::open(&path).unwrap();
    fs::remove_dir(&path).unwrap();
    assert!(store.create(IDENTITY, fixture_hash()).is_err());
    assert!(!path.exists());
}

#[test]
fn failed_writes_do_not_publish_an_owner() {
    // Root bypasses ordinary Unix mode checks; the supported service uses an
    // unprivileged OS account. Other tests still run when invoked as root.
    if geteuid().is_root() {
        return;
    }
    let directory = private_directory();
    let store = OwnerStore::open(directory.path()).unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o500)).unwrap();
    assert!(matches!(
        store.create(IDENTITY, fixture_hash()),
        Err(StoreError::Io(_))
    ));
    assert!(store.load().unwrap().is_none());
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}
