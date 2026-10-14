use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    sync::Arc,
    thread,
};
const ID: IdentityId = IdentityId::from_bytes([1; 16]);
fn fixture() -> (tempfile::TempDir, OwnerStore, ProfileStore) {
    let directory = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let owner = OwnerStore::open(directory.path()).unwrap();
    let store = ProfileStore::new(&owner, ID).unwrap();
    (directory, owner, store)
}
#[test]
fn unicode_name_and_clear_survive_reopen_with_private_permissions() {
    let (directory, owner, store) = fixture();
    assert_eq!(store.load(ID).unwrap().as_str(), "");
    assert!(!directory.path().join(RECORD).exists());
    let name = DisplayName::new("Élodie 林 <&>").unwrap();
    store.save(ID, &name).unwrap();
    assert_eq!(
        ProfileStore::new(&owner, ID).unwrap().load(ID).unwrap(),
        name
    );
    assert_eq!(
        fs::metadata(directory.path().join(RECORD)).unwrap().mode() & 0o777,
        0o600
    );
    store.save(ID, &DisplayName::default()).unwrap();
    assert_eq!(
        ProfileStore::new(&owner, ID)
            .unwrap()
            .load(ID)
            .unwrap()
            .as_str(),
        ""
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}
#[test]
fn name_validation_is_bounded_and_rejects_control_or_ambiguous_edges() {
    for name in [
        " ",
        " Alice",
        "\u{feff}Alice",
        "Alice\u{feff}",
        "Alice ",
        "a\nb",
        "a\0b",
        "a\u{7f}",
        "a\u{85}",
        "a\u{202e}b",
        "a\u{2066}b",
        &"a".repeat(129),
        &"é".repeat(65),
    ] {
        assert!(DisplayName::new(name).is_err(), "{name:?}");
    }
    for name in ["", "Alice Smith", "林", "👩‍💻", &"é".repeat(64)] {
        assert!(DisplayName::new(name).is_ok());
    }
}
#[test]
fn unsafe_or_corrupt_records_fail_closed_and_are_never_replaced() {
    let (directory, owner, store) = fixture();
    let name = DisplayName::new("Fixture").unwrap();
    store.save(ID, &name).unwrap();
    let path = directory.path().join(RECORD);
    let valid = fs::read(&path).unwrap();
    let mut wrong_id = valid.clone();
    wrong_id[9] = 2;
    let mut wrong_version = valid.clone();
    wrong_version[8] = 2;
    let mut invalid_utf8 = valid[..HEADER].to_vec();
    invalid_utf8.push(255);
    for bytes in [vec![], vec![0; 200], wrong_id, wrong_version, invalid_utf8] {
        fs::write(&path, &bytes).unwrap();
        assert!(ProfileStore::new(&owner, ID).is_err());
        assert!(store.save(ID, &name).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    fs::write(&path, &valid).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.save(ID, &name).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let alias = directory.path().join("alias");
    fs::hard_link(&path, &alias).unwrap();
    assert!(store.save(ID, &name).is_err());
    fs::remove_file(&path).unwrap();
    symlink(&alias, &path).unwrap();
    assert!(store.load(ID).is_err());
    assert!(store.save(ID, &name).is_err());
    assert_eq!(fs::read(&alias).unwrap(), valid);
}
#[test]
fn identity_and_directory_changes_cannot_redirect_writes() {
    let (directory, _owner, store) = fixture();
    let name = DisplayName::new("Fixture").unwrap();
    assert!(store.save(IdentityId::from_bytes([2; 16]), &name).is_err());
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(store.save(ID, &name).is_err());
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let moved = directory.path().with_extension("moved-profile-fixture");
    fs::rename(directory.path(), &moved).unwrap();
    fs::create_dir(directory.path()).unwrap();
    store.save(ID, &name).unwrap();
    assert!(!directory.path().join(RECORD).exists());
    assert!(moved.join(RECORD).is_file());
    fs::remove_dir_all(moved).unwrap();
}
#[test]
fn concurrent_saves_publish_only_complete_records() {
    let (directory, _owner, store) = fixture();
    let store = Arc::new(store);
    let handles: Vec<_> = (0..8)
        .map(|number| {
            let store = Arc::clone(&store);
            thread::spawn(move || {
                store
                    .save(ID, &DisplayName::new(&format!("Fixture {number}")).unwrap())
                    .unwrap()
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    assert!(store.load(ID).unwrap().as_str().starts_with("Fixture "));
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn replacement_after_open_reads_the_current_complete_record() {
    let (directory, owner, store) = fixture();
    let old = DisplayName::new("Previous").unwrap();
    let current = DisplayName::new("Current 林").unwrap();
    store.save(ID, &old).unwrap();
    let writer = ProfileStore::new(&owner, ID).unwrap();
    let opened = File::open(directory.path().join(RECORD)).unwrap();
    let mut calls = 0;
    let result = store.load_after_open(ID, || {
        calls += 1;
        if calls == 1 {
            writer.save(ID, &current).unwrap();
            assert_eq!(opened.metadata().unwrap().nlink(), 0);
        }
    });
    assert_eq!(result.unwrap(), current);
    assert_eq!(calls, 2);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn replaced_records_still_require_valid_private_single_linked_owner_data() {
    for invalid in [
        "corrupt",
        "identity",
        "oversized",
        "permissions",
        "hardlink",
        "symlink",
        "directory",
    ] {
        let (directory, owner, store) = fixture();
        let name = DisplayName::new("Valid").unwrap();
        store.save(ID, &name).unwrap();
        let writer = ProfileStore::new(&owner, ID).unwrap();
        let path = directory.path().join(RECORD);
        let alias = directory.path().join("alias");
        let mut calls = 0;
        let result = store.load_after_open(ID, || {
            calls += 1;
            if calls != 1 {
                return;
            }
            writer.save(ID, &name).unwrap();
            match invalid {
                "corrupt" => fs::write(&path, b"invalid").unwrap(),
                "identity" => {
                    let mut bytes = fs::read(&path).unwrap();
                    bytes[9] = 2;
                    fs::write(&path, bytes).unwrap();
                }
                "oversized" => fs::write(&path, [0; HEADER + MAX_NAME_BYTES + 1]).unwrap(),
                "permissions" => {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap()
                }
                "hardlink" => fs::hard_link(&path, &alias).unwrap(),
                "symlink" => {
                    fs::rename(&path, &alias).unwrap();
                    symlink(&alias, &path).unwrap();
                }
                "directory" => {
                    fs::remove_file(&path).unwrap();
                    fs::create_dir(&path).unwrap();
                }
                _ => unreachable!(),
            }
        });
        assert!(result.is_err(), "{invalid}");
        let before = fs::symlink_metadata(&path).unwrap();
        let bytes = if before.is_dir() {
            None
        } else {
            Some(fs::read(&path).unwrap())
        };
        assert!(
            store
                .save(ID, &DisplayName::new("Must not replace").unwrap())
                .is_err(),
            "{invalid}"
        );
        let after = fs::symlink_metadata(&path).unwrap();
        assert_eq!(
            (before.ino(), before.mode(), before.nlink()),
            (after.ino(), after.mode(), after.nlink()),
            "{invalid}"
        );
        if let Some(bytes) = bytes {
            assert_eq!(fs::read(&path).unwrap(), bytes, "{invalid}");
        }
    }
}

#[test]
fn disappearing_or_continuously_replaced_records_fail_with_bounded_reads() {
    let (directory, owner, store) = fixture();
    let name = DisplayName::new("Valid").unwrap();
    store.save(ID, &name).unwrap();
    let mut calls = 0;
    assert!(
        store
            .load_after_open(ID, || {
                calls += 1;
                fs::remove_file(directory.path().join(RECORD)).unwrap();
            })
            .is_err()
    );
    assert_eq!(calls, 1);
    // A separate ordinary read still treats an initially absent profile as unset.
    assert_eq!(store.load(ID).unwrap(), DisplayName::default());
    store.save(ID, &name).unwrap();
    let writer = ProfileStore::new(&owner, ID).unwrap();
    calls = 0;
    let result = store.load_after_open(ID, || {
        calls += 1;
        writer.save(ID, &name).unwrap();
    });
    assert!(
        matches!(result, Err(Error::Io(ref error)) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    assert_eq!(calls, MAX_READ_ATTEMPTS);
    assert_eq!(store.load(ID).unwrap(), name);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}
