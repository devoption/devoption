use super::*;
use crate::{
    identity::IdentityId,
    owner_store::OwnerStore,
    password::{SALT_BYTES, hash_password},
};
use devoption_projects::CatalogStore;
use std::{fs, os::unix::fs::PermissionsExt};

fn private(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn startup_catalog_uses_the_already_selected_owner_directory() {
    let temporary = tempfile::tempdir().unwrap();
    let base = temporary.path().canonicalize().unwrap();
    let data = base.join("data");
    private(&data);
    let owner = OwnerStore::open_absolute(&data).unwrap();
    let hash = hash_password("startup fixture only", &[0xa5; SALT_BYTES]).unwrap();
    let identity = IdentityId::from_bytes([7; 16]);
    owner.create(identity, &hash).unwrap();
    assert_eq!(owner.load().unwrap().unwrap().identity(), identity);
    let mut original = Catalog::default();
    original.create_project("Original fixture").unwrap();
    CatalogStore::open_absolute(&data)
        .unwrap()
        .save(&original)
        .unwrap();

    // Deterministically exercise the startup interval between selecting owner
    // storage and constructing Projects, without timing hooks in production.
    let retained = base.join("retained");
    fs::rename(&data, &retained).unwrap();
    private(&data);
    let mut replacement = Catalog::default();
    replacement.create_project("Replacement fixture").unwrap();
    CatalogStore::open_absolute(&data)
        .unwrap()
        .save(&replacement)
        .unwrap();
    let selected = Projects::open_with_root(&owner, None).unwrap();
    assert_eq!(selected.directory.load().unwrap().unwrap(), original);
    assert_eq!(owner.load().unwrap().unwrap().identity(), identity);
    assert!(
        OwnerStore::open_absolute(&data)
            .unwrap()
            .load()
            .unwrap()
            .is_none()
    );

    // Overlap is checked against the actual retained directory, too.
    assert!(matches!(
        Projects::open_with_root(&owner, Some(&retained)),
        Err(Error::RootMismatch)
    ));
    fs::write(retained.join("projects.bin"), b"invalid fixture").unwrap();
    assert!(matches!(
        Projects::open_with_root(&owner, None),
        Err(Error::InvalidCatalog)
    ));
    fs::set_permissions(&retained, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        Projects::open_with_root(&owner, None),
        Err(Error::UnsafeDirectory)
    ));
    fs::set_permissions(&retained, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        devoption_projects::read_catalog(&data).unwrap().unwrap(),
        replacement
    );
}
