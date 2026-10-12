use super::fixture;
use crate::{Catalog, CatalogStore, Error, codec, read_catalog};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
    path::Path,
    process::Command,
};

fn directory() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

fn write(path: &Path, bytes: &[u8]) {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

fn assert_relationships(catalog: &Catalog) {
    assert_eq!(catalog.projects().len(), 2);
    assert_eq!(catalog.repositories().len(), 2);
    assert_eq!(catalog.projects()[0].name(), "Application");
    let ids: Vec<_> = catalog.repositories().iter().map(|r| r.id()).collect();
    assert_eq!(catalog.projects()[0].repositories(), ids);
    assert_eq!(catalog.projects()[1].repositories(), &ids[1..]);
}

#[test]
fn snapshot_reads_create_no_files_and_can_coexist_with_a_writer() {
    let temporary = directory();
    let root = temporary.path().canonicalize().unwrap();
    assert!(read_catalog(&root).unwrap().is_none());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    let mut writer = CatalogStore::open_absolute(&root).unwrap();
    let catalog = fixture();
    writer.save(&catalog).unwrap();
    let before = fs::read(root.join("projects.bin")).unwrap();
    assert_eq!(read_catalog(&root).unwrap().unwrap(), catalog);
    drop(writer);
    fs::remove_file(root.join(".projects.lock")).unwrap();
    assert_eq!(read_catalog(&root).unwrap().unwrap(), catalog);
    assert_eq!(fs::read(root.join("projects.bin")).unwrap(), before);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    write(&root.join("projects.bin"), &vec![0; codec::MAX_BYTES + 1]);
    assert!(matches!(read_catalog(&root), Err(Error::InvalidCatalog)));
    assert_eq!(
        fs::metadata(root.join("projects.bin")).unwrap().len(),
        (codec::MAX_BYTES + 1) as u64
    );
}

#[test]
fn multiple_repositories_and_stable_ids_survive_reopening_and_atomic_updates() {
    let temporary = directory();
    let root = temporary.path().canonicalize().unwrap();
    write(&root.join("owner.bin"), b"unrelated owner fixture");
    let mut store = CatalogStore::open_absolute(&root).unwrap();
    assert!(store.load().unwrap().is_none());
    let mut catalog = fixture();
    store.save(&catalog).unwrap();
    assert_relationships(&store.load().unwrap().unwrap());
    drop(store);
    let mut store = CatalogStore::open_absolute(&root).unwrap();
    assert_eq!(store.load().unwrap().unwrap(), catalog);
    catalog.create_project("Additional project").unwrap();
    store.save(&catalog).unwrap();
    assert_eq!(store.load().unwrap().unwrap(), catalog);
    assert_eq!(
        fs::read(root.join("owner.bin")).unwrap(),
        b"unrelated owner fixture"
    );
    for name in ["projects.bin", ".projects.lock"] {
        assert_eq!(
            fs::metadata(root.join(name)).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
}

#[test]
fn process_restart_and_conflicting_writers_preserve_identity_and_state() {
    const MODE: &str = "DEVOPTION_CATALOG_TEST_MODE";
    const ROOT: &str = "DEVOPTION_CATALOG_TEST_ROOT";
    if let Ok(mode) = std::env::var(MODE) {
        let root = std::env::var_os(ROOT).unwrap();
        if mode == "busy" {
            assert!(matches!(
                CatalogStore::open_absolute(Path::new(&root)),
                Err(Error::Busy)
            ));
        } else {
            let mut store = CatalogStore::open_absolute(Path::new(&root)).unwrap();
            if mode == "write" {
                store.save(&fixture()).unwrap();
            }
            assert_relationships(&store.load().unwrap().unwrap());
        }
        return;
    }
    let temporary = directory();
    let root = temporary.path().canonicalize().unwrap();
    let child = |mode: &str| {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::storage::process_restart_and_conflicting_writers_preserve_identity_and_state"])
            .env(MODE, mode).env(ROOT, &root).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
    };
    child("write");
    let bytes = fs::read(root.join("projects.bin")).unwrap();
    let store = CatalogStore::open_absolute(&root).unwrap();
    let original = store.load().unwrap().unwrap();
    assert!(matches!(
        CatalogStore::open_absolute(&root),
        Err(Error::Busy)
    ));
    child("busy");
    drop(store);
    child("read");
    assert_eq!(fs::read(root.join("projects.bin")).unwrap(), bytes);
    assert_eq!(
        CatalogStore::open_absolute(&root)
            .unwrap()
            .load()
            .unwrap()
            .unwrap(),
        original
    );
}

#[test]
fn partial_write_failure_and_abandoned_temporaries_do_not_replace_previous_state() {
    let temporary = directory();
    let root = temporary.path().canonicalize().unwrap();
    let mut store = CatalogStore::open_absolute(&root).unwrap();
    let original = fixture();
    store.save(&original).unwrap();
    let bytes = fs::read(root.join("projects.bin")).unwrap();
    let mut replacement = original.clone();
    replacement.create_project("Pending").unwrap();
    for count in [0, 10, bytes.len() / 2] {
        let result = store.save_using(&replacement, |file, bytes| {
            file.write_all(&bytes[..count])?;
            Err(io::Error::other("injected write interruption"))
        });
        assert!(matches!(result, Err(Error::Io(_))));
        assert_eq!(fs::read(root.join("projects.bin")).unwrap(), bytes);
        assert_eq!(store.load().unwrap().unwrap(), original);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
    }
    // A process can die before cleanup. Its unpublished bytes must not be read.
    write(
        &root.join(".projects-interrupted.tmp"),
        b"incomplete catalog",
    );
    drop(store);
    let mut store = CatalogStore::open_absolute(&root).unwrap();
    assert_eq!(store.load().unwrap().unwrap(), original);
    store.save(&replacement).unwrap();
    assert_eq!(store.load().unwrap().unwrap(), replacement);
    assert_eq!(
        fs::read(root.join(".projects-interrupted.tmp")).unwrap(),
        b"incomplete catalog"
    );
}

#[test]
fn readers_observe_complete_snapshots_during_atomic_replacement() {
    let temporary = directory();
    let root = temporary.path().canonicalize().unwrap();
    let mut store = CatalogStore::open_absolute(&root).unwrap();
    let mut catalog = fixture();
    store.save(&catalog).unwrap();
    let path = root.join("projects.bin");
    let reader = std::thread::spawn(move || {
        for _ in 0..128 {
            let snapshot = codec::decode(&fs::read(&path).unwrap()).unwrap();
            assert_eq!(snapshot.repositories().len(), 2);
            assert_eq!(snapshot.projects()[0].repositories().len(), 2);
            std::thread::yield_now();
        }
    });
    for _ in 0..32 {
        catalog.create_project("Additional").unwrap();
        store.save(&catalog).unwrap();
    }
    reader.join().unwrap();
    assert_eq!(store.load().unwrap().unwrap(), catalog);
}

#[test]
fn malformed_or_unsupported_state_cannot_be_reset_or_overwritten() {
    let temporary = directory();
    let root = temporary.path().canonicalize().unwrap();
    let mut store = CatalogStore::open_absolute(&root).unwrap();
    let catalog = fixture();
    store.save(&catalog).unwrap();
    let good = codec::encode(&catalog).unwrap();
    let mut unsupported = good.clone();
    unsupported[8] = 255;
    for bad in [
        vec![],
        b"broken".to_vec(),
        unsupported,
        vec![0; codec::MAX_BYTES + 1],
    ] {
        write(&root.join("projects.bin"), &bad);
        assert!(store.load().is_err());
        assert!(store.save(&Catalog::default()).is_err());
        assert_eq!(fs::read(root.join("projects.bin")).unwrap(), bad);
    }
    drop(store);
    assert!(CatalogStore::open_absolute(&root).is_err());
    assert_eq!(
        fs::metadata(root.join("projects.bin")).unwrap().len(),
        (codec::MAX_BYTES + 1) as u64
    );
}

#[test]
fn codec_rejects_truncation_overflow_duplicate_ids_and_dangling_associations() {
    let catalog = fixture();
    let good = codec::encode(&catalog).unwrap();
    for end in 0..good.len() {
        assert!(codec::decode(&good[..end]).is_err());
    }
    let mut unsupported = good.clone();
    unsupported[8] = 255;
    assert!(matches!(
        codec::decode(&unsupported),
        Err(Error::UnsupportedVersion)
    ));
    let mut trailing = good.clone();
    trailing.push(0);
    let mut overflow = good.clone();
    overflow[9..11].copy_from_slice(&u16::MAX.to_le_bytes());
    let mut utf8 = good.clone();
    utf8[29] = 0xff;
    let mut duplicate = good.clone();
    let second_id = 11 + 16 + 2 + catalog.repositories()[0].name().len();
    duplicate[second_id..second_id + 16]
        .copy_from_slice(&catalog.repositories()[0].id().to_bytes());
    let mut dangling = good.clone();
    let last_id = dangling.len() - 16;
    dangling[last_id..].fill(0xff);
    for bad in [trailing, overflow, utf8, duplicate, dangling] {
        assert!(matches!(codec::decode(&bad), Err(Error::InvalidCatalog)));
    }
    let mut invalid = catalog;
    let first = invalid.projects[0].repositories[0];
    invalid.projects[0].repositories.push(first);
    assert!(codec::encode(&invalid).is_err());
}

#[test]
fn paths_permissions_and_special_files_fail_closed() {
    let temporary = directory();
    let root = temporary.path().canonicalize().unwrap();
    let data = root.join("data");
    fs::create_dir(&data).unwrap();
    fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&data, root.join("alias")).unwrap();
    symlink(&root, root.join("ancestor")).unwrap();
    for path in [
        Path::new("relative").to_owned(),
        data.join(".."),
        root.join("alias"),
        root.join("ancestor/data"),
        root.join("missing"),
    ] {
        assert!(CatalogStore::open_absolute(&path).is_err());
        assert!(read_catalog(&path).is_err());
    }
    fs::set_permissions(&data, fs::Permissions::from_mode(0o750)).unwrap();
    assert!(matches!(
        CatalogStore::open_absolute(&data),
        Err(Error::UnsafeDirectory)
    ));
    fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
    let mut store = CatalogStore::open_absolute(&data).unwrap();
    store.save(&fixture()).unwrap();
    for name in ["projects.bin", ".projects.lock"] {
        fs::set_permissions(data.join(name), fs::Permissions::from_mode(0o640)).unwrap();
        assert!(store.load().is_err());
        assert!(store.save(&Catalog::default()).is_err());
        fs::set_permissions(data.join(name), fs::Permissions::from_mode(0o600)).unwrap();
    }
    fs::hard_link(data.join("projects.bin"), data.join("linked")).unwrap();
    assert!(matches!(store.load(), Err(Error::UnsafeFile)));
    fs::remove_file(data.join("linked")).unwrap();
    fs::rename(data.join("projects.bin"), root.join("outside")).unwrap();
    let outside = fs::read(root.join("outside")).unwrap();
    symlink(root.join("outside"), data.join("projects.bin")).unwrap();
    assert!(store.save(&fixture()).is_err());
    assert!(read_catalog(&data).is_err());
    assert_eq!(fs::read(root.join("outside")).unwrap(), outside);
    fs::remove_file(data.join("projects.bin")).unwrap();
    fs::create_dir(data.join("projects.bin")).unwrap();
    assert!(store.load().is_err());
    drop(store);
    fs::remove_file(data.join(".projects.lock")).unwrap();
    symlink(root.join("outside"), data.join(".projects.lock")).unwrap();
    assert!(CatalogStore::open_absolute(&data).is_err());
    assert_eq!(fs::read(root.join("outside")).unwrap(), outside);
}

#[test]
fn opened_directory_anchor_survives_rename_without_following_replacement_symlinks() {
    let temporary = directory();
    let root = temporary.path().canonicalize().unwrap();
    let data = root.join("data");
    fs::create_dir(&data).unwrap();
    fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
    let mut store = CatalogStore::open_absolute(&data).unwrap();
    let moved = root.join("moved");
    fs::rename(&data, &moved).unwrap();
    symlink(&moved, &data).unwrap();
    assert!(CatalogStore::open_absolute(&data).is_err());
    let catalog = fixture();
    store.save(&catalog).unwrap();
    assert_eq!(
        codec::decode(&fs::read(moved.join("projects.bin")).unwrap()).unwrap(),
        catalog
    );
}
