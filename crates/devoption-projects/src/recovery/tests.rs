use super::*;
use crate::{CatalogDirectory, CatalogStore};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};

fn directory(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
fn repository(path: &Path) {
    directory(path);
    directory(&path.join(".git"));
    directory(&path.join(".git/objects"));
    directory(&path.join(".git/refs"));
    fs::write(path.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(
        path.join(".git/config"),
        "[core]\nrepositoryformatversion = 0\nbare = false\n",
    )
    .unwrap();
}

struct Fixture {
    _temp: tempfile::TempDir,
    data: PathBuf,
    path: PathBuf,
    original: Catalog,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let data = base.join("data");
        let path = base.join("root");
        directory(&data);
        directory(&path);
        repository(&path.join("app"));
        directory(&path.join("linked"));
        directory(&path.join("app/.git/worktrees"));
        directory(&path.join("app/.git/worktrees/linked"));
        fs::write(
            path.join("linked/.git"),
            "gitdir: ../app/.git/worktrees/linked\n",
        )
        .unwrap();
        fs::write(path.join("app/.git/worktrees/linked/commondir"), "../..\n").unwrap();
        fs::write(
            path.join("app/.git/worktrees/linked/gitdir"),
            "../../../../linked/.git\n",
        )
        .unwrap();
        fs::write(
            path.join("app/.git/worktrees/linked/HEAD"),
            "ref: refs/heads/feature\n",
        )
        .unwrap();
        let root = PermittedRoot::open_absolute(&path).unwrap();
        let mut store = CatalogStore::open_absolute(&data).unwrap();
        let one = store.create_project("工具 <first>").unwrap();
        let two = store.create_project("Second").unwrap();
        let repo = store
            .attach_repository(one, &root, Path::new("app"))
            .unwrap();
        assert_eq!(
            store
                .attach_repository(two, &root, Path::new("linked"))
                .unwrap(),
            repo
        );
        let original = store.load().unwrap().unwrap();
        let mut retired = original.clone();
        retired.root.as_mut().unwrap().identity.device ^= 1u64 << 32;
        for binding in &mut retired.bindings {
            binding.common_directory.device ^= 1u64 << 32;
        }
        for c in &mut retired.checkouts {
            c.worktree.device ^= 1u64 << 32;
            c.git_directory.device ^= 1u64 << 32;
        }
        store.save(&retired).unwrap();
        fs::write(data.join("owner.bin"), b"private unrelated fixture bytes").unwrap();
        Self {
            _temp: temp,
            data,
            path,
            original,
        }
    }
    fn root(&self) -> PermittedRoot {
        PermittedRoot::open_absolute(&self.path).unwrap()
    }
    fn reader(&self) -> CatalogDirectory {
        CatalogDirectory::open_absolute(&self.data).unwrap()
    }
    fn bytes(&self) -> Vec<u8> {
        fs::read(self.data.join("projects.bin")).unwrap()
    }
}

#[test]
fn device_recovery_preserves_catalog_source_and_private_backup_after_explicit_approval() {
    let f = Fixture::new();
    let root = f.root();
    let reader = f.reader();
    let before = f.bytes();
    assert!(matches!(
        reader.load().unwrap().unwrap().check_repository_root(&root),
        Err(Error::RootMismatch)
    ));
    let entries = fs::read_dir(&f.data).unwrap().count();
    let review = reader.review_device_rebind(&root).unwrap();
    assert_eq!(review.changes().len(), 7);
    assert_eq!(f.bytes(), before);
    assert_eq!(fs::read_dir(&f.data).unwrap().count(), entries);
    assert_eq!(
        review.fingerprint(),
        reader.review_device_rebind(&root).unwrap().fingerprint()
    );
    let mut store = reader.writer().unwrap();
    assert!(store.approve_device_rebind(&root, [0; 32]).is_err());
    assert!(!f.data.join(review.backup_name()).exists());
    assert_eq!(f.bytes(), before);
    let backup = store
        .approve_device_rebind(&root, review.fingerprint())
        .unwrap();
    assert_eq!(store.load().unwrap().unwrap(), f.original);
    assert_eq!(fs::read(f.data.join(&backup)).unwrap(), before);
    assert_eq!(
        fs::metadata(f.data.join(backup))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::read(f.data.join("owner.bin")).unwrap(),
        b"private unrelated fixture bytes"
    );
    assert_eq!(
        fs::read(f.path.join("app/.git/HEAD")).unwrap(),
        b"ref: refs/heads/main\n"
    );
    assert!(reader.review_device_rebind(&root).is_err());
    assert!(
        store
            .approve_device_rebind(&root, review.fingerprint())
            .is_err()
    );
}

#[test]
fn catalog_changes_and_other_data_directories_invalidate_review() {
    let f = Fixture::new();
    let root = f.root();
    let reader = f.reader();
    let token = reader.review_device_rebind(&root).unwrap().fingerprint();
    let other = f.data.with_file_name("other-data");
    directory(&other);
    fs::write(other.join("projects.bin"), f.bytes()).unwrap();
    fs::set_permissions(
        other.join("projects.bin"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(
        CatalogStore::open_absolute(&other)
            .unwrap()
            .approve_device_rebind(&root, token)
            .is_err()
    );
    let mut store = reader.writer().unwrap();
    let id = store.load().unwrap().unwrap().projects()[0].id();
    store.rename_project(id, "Changed after review").unwrap();
    let changed = f.bytes();
    assert!(store.approve_device_rebind(&root, token).is_err());
    assert_eq!(f.bytes(), changed);
    assert!(matches!(reader.writer(), Err(Error::Busy)));
}

#[test]
fn different_paths_inodes_devices_and_unsafe_layouts_cannot_be_rebound() {
    for case in [
        "root-path",
        "root-inode",
        "worktree-inode",
        "inconsistent-device",
        "missing",
        "symlink",
        "hardlink",
        "permissions",
        "invalid-git",
    ] {
        let f = Fixture::new();
        let root = f.root();
        let mut catalog = f.reader().load().unwrap().unwrap();
        match case {
            "root-path" => catalog.root.as_mut().unwrap().path.push_str("-changed"),
            "root-inode" => catalog.root.as_mut().unwrap().identity.inode += 1,
            "worktree-inode" => {
                for c in &mut catalog.checkouts {
                    c.worktree.inode += 1;
                }
            }
            "inconsistent-device" => {
                for c in &mut catalog.checkouts {
                    c.worktree.device ^= 1u64 << 33;
                }
            }
            "missing" => fs::rename(f.path.join("app"), f.path.join("missing")).unwrap(),
            "symlink" => {
                fs::rename(f.path.join("app"), f.path.join("actual")).unwrap();
                symlink("actual", f.path.join("app")).unwrap();
            }
            "hardlink" => {
                fs::hard_link(f.path.join("app/.git/HEAD"), f.path.join("head-alias")).unwrap()
            }
            "permissions" => {
                fs::set_permissions(f.path.join("app"), fs::Permissions::from_mode(0o777)).unwrap()
            }
            "invalid-git" => {
                fs::write(f.path.join("app/.git/config"), "[include]\npath = unsafe\n").unwrap()
            }
            _ => unreachable!(),
        }
        CatalogStore::open_absolute(&f.data)
            .unwrap()
            .save(&catalog)
            .unwrap();
        let bytes = f.bytes();
        assert!(f.reader().review_device_rebind(&root).is_err(), "{case}");
        assert_eq!(f.bytes(), bytes, "{case}");
    }
}

#[test]
fn overlap_and_replaced_root_handles_are_rejected() {
    let f = Fixture::new();
    let root = f.root();
    fs::write(f.path.join("projects.bin"), f.bytes()).unwrap();
    fs::set_permissions(
        f.path.join("projects.bin"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(
        CatalogDirectory::open_absolute(&f.path)
            .unwrap()
            .review_device_rebind(&root)
            .is_err()
    );
    fs::rename(&f.path, f.path.with_file_name("retired-root")).unwrap();
    directory(&f.path);
    assert!(f.reader().review_device_rebind(&root).is_err());
    assert!(f.reader().review_device_rebind(&f.root()).is_err());
}

#[test]
fn unsafe_or_different_backups_never_get_overwritten() {
    for case in ["symlink", "hardlink", "permissions", "directory", "corrupt"] {
        let f = Fixture::new();
        let root = f.root();
        let reader = f.reader();
        let review = reader.review_device_rebind(&root).unwrap();
        let backup = f.data.join(review.backup_name());
        let target = f.data.join("other");
        fs::write(&target, b"preserve me").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        match case {
            "symlink" => symlink(&target, &backup).unwrap(),
            "hardlink" => fs::hard_link(&target, &backup).unwrap(),
            "permissions" => {
                fs::write(&backup, f.bytes()).unwrap();
                fs::set_permissions(&backup, fs::Permissions::from_mode(0o644)).unwrap();
            }
            "directory" => directory(&backup),
            "corrupt" => {
                fs::write(&backup, b"partial backup").unwrap();
                fs::set_permissions(&backup, fs::Permissions::from_mode(0o600)).unwrap();
            }
            _ => unreachable!(),
        }
        let before = f.bytes();
        assert!(
            reader
                .writer()
                .unwrap()
                .approve_device_rebind(&root, review.fingerprint())
                .is_err(),
            "{case}"
        );
        assert_eq!(f.bytes(), before);
        assert_eq!(fs::read(&target).unwrap(), b"preserve me");
    }
}

#[test]
fn backup_is_retained_on_failure_and_reused_only_if_identical() {
    let f = Fixture::new();
    let root = f.root();
    let reader = f.reader();
    let review = reader.review_device_rebind(&root).unwrap();
    let before = f.bytes();
    let mut store = reader.writer().unwrap();
    assert!(
        store
            .approve_device_rebind_using(&root, review.fingerprint(), || Err(
                std::io::Error::other("injected failure").into()
            ))
            .is_err()
    );
    assert_eq!(f.bytes(), before);
    assert_eq!(fs::read(f.data.join(review.backup_name())).unwrap(), before);
    store
        .approve_device_rebind(&root, review.fingerprint())
        .unwrap();
    assert_eq!(store.load().unwrap().unwrap(), f.original);
}

#[test]
fn replacement_between_backup_and_publication_preserves_original_catalog() {
    let f = Fixture::new();
    let root = f.root();
    let reader = f.reader();
    let review = reader.review_device_rebind(&root).unwrap();
    let before = f.bytes();
    let result =
        reader
            .writer()
            .unwrap()
            .approve_device_rebind_using(&root, review.fingerprint(), || {
                fs::rename(f.path.join("app"), f.path.join("retired-app"))?;
                repository(&f.path.join("app"));
                Ok(())
            });
    assert!(result.is_err());
    assert_eq!(f.bytes(), before);
    assert_eq!(fs::read(f.data.join(review.backup_name())).unwrap(), before);
}

#[test]
fn changed_backup_or_catalog_after_review_is_never_published_over() {
    for replace_backup in [true, false] {
        let f = Fixture::new();
        let root = f.root();
        let reader = f.reader();
        let review = reader.review_device_rebind(&root).unwrap();
        let before = f.bytes();
        let mut external = review.original.clone();
        external
            .rename_project(external.projects()[0].id(), "Concurrent update")
            .unwrap();
        let result = reader.writer().unwrap().approve_device_rebind_using(
            &root,
            review.fingerprint(),
            || {
                if replace_backup {
                    let backup = f.data.join(review.backup_name());
                    fs::rename(&backup, f.data.join("retained-backup"))?;
                    fs::write(&backup, b"replacement")?;
                    fs::set_permissions(&backup, fs::Permissions::from_mode(0o600))?;
                } else {
                    fs::write(f.data.join("projects.bin"), codec::encode(&external)?)?;
                }
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(
            f.bytes(),
            if replace_backup {
                before
            } else {
                codec::encode(&external).unwrap()
            }
        );
    }
}

#[test]
fn in_place_backup_corruption_before_publication_is_rejected() {
    for reused in [false, true] {
        let f = Fixture::new();
        let root = f.root();
        let reader = f.reader();
        let review = reader.review_device_rebind(&root).unwrap();
        let before = f.bytes();
        let backup = f.data.join(review.backup_name());
        if reused {
            fs::write(&backup, &before).unwrap();
            fs::set_permissions(&backup, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let result = reader.writer().unwrap().approve_device_rebind_using(
            &root,
            review.fingerprint(),
            || {
                fs::write(&backup, b"partial")?;
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(f.bytes(), before);
    }
}
