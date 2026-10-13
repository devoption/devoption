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
