use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    sync::{Arc, Barrier},
    thread,
};

const PASSWORD: &str = "owner authentication fixture only";

fn fixture() -> (tempfile::TempDir, OwnerStore) {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let store = OwnerStore::open(directory.path()).unwrap();
    (directory, store)
}

fn setup_fixture(store: &OwnerStore, password: &str) -> IdentityId {
    let mut value = 0;
    setup_with_entropy(store, password, |bytes| {
        value += 1;
        bytes.fill(value);
        Ok(())
    })
    .unwrap()
}

#[test]
fn public_setup_and_sign_in_survive_reopening_the_store() {
    let (directory, store) = fixture();
    let identity = setup(&store, PASSWORD).unwrap();
    assert_eq!(store.load().unwrap().unwrap().identity(), identity);
    drop(store);

    let reopened = OwnerStore::open(directory.path()).unwrap();
    assert_eq!(authenticate(&reopened, PASSWORD).unwrap(), Some(identity));
    for wrong in [
        "wrong fixture",
        "",
        &"x".repeat(password::MAX_PASSWORD_BYTES + 1),
    ] {
        assert_eq!(authenticate(&reopened, wrong).unwrap(), None);
    }
    let bytes = fs::read(directory.path().join("owner.bin")).unwrap();
    assert!(
        !bytes
            .windows(PASSWORD.len())
            .any(|part| part == PASSWORD.as_bytes())
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn setup_keeps_identity_salt_and_password_bytes_distinct() {
    let (_directory, store) = fixture();
    let exact = "  owner fixture café 🔑\0\n";
    let identity = setup_fixture(&store, exact);
    assert_eq!(identity, IdentityId::from_bytes([1; 16]));
    let record = store.load().unwrap().unwrap();
    assert_eq!(
        record.password_hash(),
        password::hash_password(exact, &[2; SALT_BYTES]).unwrap()
    );
    assert_eq!(authenticate(&store, exact).unwrap(), Some(identity));
    assert_eq!(authenticate(&store, exact.trim()).unwrap(), None);
    assert_eq!(
        authenticate(&store, "  owner fixture cafe\u{301} 🔑\0\n").unwrap(),
        None
    );
}

#[test]
fn invalid_setup_input_uses_no_entropy_and_writes_nothing() {
    let (directory, store) = fixture();
    for invalid in [
        String::new(),
        "🔑".repeat(password::MAX_PASSWORD_BYTES / 4 + 1),
    ] {
        assert!(matches!(
            setup_with_entropy(&store, &invalid, |_| panic!(
                "invalid input requested entropy"
            )),
            Err(SetupError::Password(HashError::InvalidLength))
        ));
        assert!(store.load().unwrap().is_none());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}

#[test]
fn absent_owner_cannot_sign_in_or_create_storage() {
    let (directory, store) = fixture();
    assert_eq!(authenticate(&store, PASSWORD).unwrap(), None);
    assert_eq!(authenticate(&store, "").unwrap(), None);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn duplicate_setup_preserves_the_existing_identity_and_password() {
    let (directory, store) = fixture();
    let identity = setup_fixture(&store, PASSWORD);
    let original = fs::read(directory.path().join("owner.bin")).unwrap();
    for replacement in ["replacement fixture", ""] {
        assert!(matches!(
            setup_with_entropy(&store, replacement, |_| panic!(
                "duplicate requested entropy"
            )),
            Err(SetupError::Store(StoreError::AlreadyExists))
        ));
        assert_eq!(authenticate(&store, replacement).unwrap(), None);
    }
    assert_eq!(authenticate(&store, PASSWORD).unwrap(), Some(identity));
    assert_eq!(
        fs::read(directory.path().join("owner.bin")).unwrap(),
        original
    );
}

#[test]
fn entropy_failure_never_publishes_partial_or_zero_filled_credentials() {
    for fail_on in [1, 2] {
        let (directory, store) = fixture();
        let mut calls = 0;
        let result = setup_with_entropy(&store, PASSWORD, |bytes| {
            calls += 1;
            bytes[0] = 0x5a; // Model a random source that fails after a partial fill.
            if calls == fail_on {
                Err(SetupError::RandomnessUnavailable)
            } else {
                Ok(())
            }
        });
        assert!(matches!(result, Err(SetupError::RandomnessUnavailable)));
        assert_eq!(calls, fail_on);
        assert!(store.load().unwrap().is_none());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
        let identity = setup_fixture(&store, PASSWORD);
        assert_eq!(authenticate(&store, PASSWORD).unwrap(), Some(identity));
    }
}

#[test]
fn broken_storage_is_an_error_for_setup_and_sign_in_without_replacement() {
    for unsupported_version in [false, true] {
        let (directory, store) = fixture();
        setup_fixture(&store, PASSWORD);
        let path = directory.path().join("owner.bin");
        let mut broken = fs::read(&path).unwrap();
        if unsupported_version {
            broken[8] = 99;
        } else {
            broken.truncate(5);
        }
        fs::write(&path, &broken).unwrap();
        let error = setup_with_entropy(&store, PASSWORD, |_| {
            panic!("broken store requested entropy")
        })
        .unwrap_err();
        assert!(matches!(
            error,
            SetupError::Store(StoreError::InvalidRecord | StoreError::UnsupportedVersion)
        ));
        assert!(matches!(
            authenticate(&store, PASSWORD),
            Err(StoreError::InvalidRecord | StoreError::UnsupportedVersion)
        ));
        assert_eq!(fs::read(path).unwrap(), broken);
        for message in [error.to_string(), format!("{error:?}")] {
            assert!(!message.contains(PASSWORD));
            assert!(!message.contains("$argon2"));
        }
    }
}

#[test]
fn unsafe_permissions_and_record_symlinks_do_not_become_setup_state() {
    let (directory, store) = fixture();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        setup(&store, PASSWORD),
        Err(SetupError::Store(StoreError::UnsafeDirectory))
    ));
    assert!(matches!(
        authenticate(&store, PASSWORD),
        Err(StoreError::UnsafeDirectory)
    ));
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let untouched = directory.path().join("untouched");
    fs::write(&untouched, b"unrelated fixture").unwrap();
    symlink(&untouched, directory.path().join("owner.bin")).unwrap();
    assert!(matches!(setup(&store, PASSWORD), Err(SetupError::Store(_))));
    assert!(authenticate(&store, PASSWORD).is_err());
    assert_eq!(fs::read(untouched).unwrap(), b"unrelated fixture");
}

#[test]
fn failed_publication_returns_no_identity() {
    if rustix::process::geteuid().is_root() {
        return; // Root bypasses the fixture's Unix mode restrictions.
    }
    let (directory, store) = fixture();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o500)).unwrap();
    assert!(matches!(
        setup(&store, PASSWORD),
        Err(SetupError::Store(StoreError::Io(_)))
    ));
    assert!(store.load().unwrap().is_none());
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn racing_setup_attempts_authenticate_only_the_published_owner() {
    let (directory, store) = fixture();
    let barrier = Arc::new(Barrier::new(4));
    let attempts: Vec<_> = (1..=4)
        .map(|index| {
            let path = directory.path().to_owned();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let store = OwnerStore::open(&path).unwrap();
                let password = format!("concurrent owner fixture {index} only");
                let mut calls = 0;
                let result = setup_with_entropy(&store, &password, |bytes| {
                    calls += 1;
                    if calls == 1 {
                        // Every attempt has observed an absent owner before publication.
                        barrier.wait();
                    }
                    bytes.fill(index * 2 + calls);
                    Ok(())
                });
                (password, result)
            })
        })
        .collect();
    let attempts: Vec<_> = attempts
        .into_iter()
        .map(|attempt| attempt.join().unwrap())
        .collect();
    assert_eq!(
        attempts.iter().filter(|(_, result)| result.is_ok()).count(),
        1
    );
    let published = store.load().unwrap().unwrap().identity();
    for (password, result) in attempts {
        match result {
            Ok(identity) => {
                assert_eq!(identity, published);
                assert_eq!(authenticate(&store, &password).unwrap(), Some(published));
            }
            Err(SetupError::Store(StoreError::AlreadyExists)) => {
                assert_eq!(authenticate(&store, &password).unwrap(), None);
            }
            Err(error) => panic!("unexpected fixture setup error: {error}"),
        }
    }
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}
