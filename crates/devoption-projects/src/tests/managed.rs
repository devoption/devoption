use crate::{
    Catalog, CatalogStore, Error, ProjectId, read_catalog,
    worktree::{ManagedRoot, PermittedRoot},
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    data: PathBuf,
    project: ProjectId,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let root = base.join("repositories");
        let data = base.join("data");
        for path in [&root, &data] {
            fs::create_dir(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut catalog = Catalog::default();
        let project = catalog.create_project("Fixture project").unwrap();
        CatalogStore::open_absolute(&data)
            .unwrap()
            .save(&catalog)
            .unwrap();
        Self {
            _temp: temp,
            root,
            data,
            project,
        }
    }
    fn managed(&self) -> ManagedRoot {
        ManagedRoot::open_absolute(&self.root).unwrap()
    }
    fn bytes(&self) -> Vec<u8> {
        fs::read(self.data.join("projects.bin")).unwrap()
    }
}
#[test]
fn created_repository_has_private_valid_git_metadata_and_persisted_independent_identity() {
    let f = Fixture::new();
    let root = f.managed();
    let before = f.bytes();
    let id = CatalogStore::open_absolute(&f.data)
        .unwrap()
        .create_managed_repository(f.project, &root, "example-app")
        .unwrap();
    assert_ne!(f.bytes(), before);
    let saved = read_catalog(&f.data).unwrap().unwrap();
    assert_eq!(saved.projects()[0].repositories(), &[id]);
    assert_eq!(saved.repositories()[0].name(), "example-app");
    assert_eq!(
        saved.default_checkouts()[0].relative_path(),
        Path::new("example-app")
    );
    let status = saved
        .repository_status(f.project, id, root.permitted())
        .unwrap();
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.commit, None);
    assert_eq!(status.changes.untracked, 0);
    for path in [
        "example-app",
        "example-app/.git",
        "example-app/.git/objects",
        "example-app/.git/refs",
    ] {
        assert_eq!(
            fs::metadata(f.root.join(path))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    for path in ["example-app/.git/HEAD", "example-app/.git/config"] {
        assert_eq!(
            fs::metadata(f.root.join(path))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    assert!(!f.root.join("example-app/.git/hooks").exists());
    assert_eq!(
        CatalogStore::open_absolute(&f.data)
            .unwrap()
            .load()
            .unwrap()
            .unwrap(),
        saved
    );
    assert_eq!(
        root.permitted().checkout_options().unwrap().paths,
        ["example-app"]
    );
}
#[test]
fn duplicates_and_existing_files_directories_symlinks_never_change_saved_or_existing_data() {
    let f = Fixture::new();
    let root = f.managed();
    fs::write(f.root.join("file"), "preserved").unwrap();
    fs::create_dir(f.root.join("directory")).unwrap();
    symlink("file", f.root.join("link")).unwrap();
    let mut store = CatalogStore::open_absolute(&f.data).unwrap();
    store
        .create_managed_repository(f.project, &root, "created")
        .unwrap();
    let before = f.bytes();
    let original = fs::read(f.root.join("created/.git/config")).unwrap();
    for name in ["file", "directory", "link", "created"] {
        assert!(matches!(
            store.create_managed_repository(f.project, &root, name),
            Err(Error::RepositoryExists)
        ));
        assert_eq!(f.bytes(), before);
    }
    assert_eq!(fs::read(f.root.join("file")).unwrap(), b"preserved");
    assert_eq!(fs::read_dir(f.root.join("directory")).unwrap().count(), 0);
    assert!(f.root.join("link").is_symlink());
    assert_eq!(
        fs::read(f.root.join("created/.git/config")).unwrap(),
        original
    );
}
#[test]
fn invalid_names_missing_project_and_capacity_fail_before_creating_paths() {
    let f = Fixture::new();
    let root = f.managed();
    let mut store = CatalogStore::open_absolute(&f.data).unwrap();
    let before = f.bytes();
    for name in [
        "",
        "../escape",
        "/outside",
        "two/parts",
        "two\\parts",
        ".git",
        "-name",
        "a b",
        "é",
        "a\n",
        &"x".repeat(65),
    ] {
        assert!(matches!(
            store.create_managed_repository(f.project, &root, name),
            Err(Error::InvalidName)
        ));
    }
    assert!(matches!(
        store.create_managed_repository(ProjectId::from_bytes([0; 16]), &root, "missing"),
        Err(Error::UnknownProject)
    ));
    assert_eq!(f.bytes(), before);
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 0);
    let mut catalog = store.load().unwrap().unwrap();
    for i in 0..crate::MAX_REPOSITORIES {
        catalog.create_repository(&format!("Metadata {i}")).unwrap();
    }
    store.save(&catalog).unwrap();
    assert!(matches!(
        store.create_managed_repository(f.project, &root, "capacity"),
        Err(Error::Capacity)
    ));
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 0);
}
#[test]
fn unsafe_changed_or_overlapping_root_cannot_gain_write_authority() {
    let f = Fixture::new();
    let root = f.managed();
    let mut store = CatalogStore::open_absolute(&f.data).unwrap();
    fs::set_permissions(&f.root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(ManagedRoot::open_absolute(&f.root).is_err());
    assert!(
        store
            .create_managed_repository(f.project, &root, "unsafe")
            .is_err()
    );
    fs::set_permissions(&f.root, fs::Permissions::from_mode(0o700)).unwrap();
    let moved = f.root.with_extension("moved");
    fs::rename(&f.root, &moved).unwrap();
    fs::create_dir(&f.root).unwrap();
    fs::set_permissions(&f.root, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        store
            .create_managed_repository(f.project, &root, "changed")
            .is_err()
    );
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&moved).unwrap().count(), 0);
    let overlapping = ManagedRoot::open_absolute(&f.data).unwrap();
    assert!(matches!(
        store.create_managed_repository(f.project, &overlapping, "overlap"),
        Err(Error::RootMismatch)
    ));
    assert!(!f.data.join("overlap").exists());
    let alias = f.root.with_extension("alias");
    symlink(&f.root, &alias).unwrap();
    assert!(ManagedRoot::open_absolute(&alias).is_err());
}
#[test]
fn failed_catalog_publication_preserves_unregistered_checkout_for_explicit_recovery() {
    let f = Fixture::new();
    let root = f.managed();
    let mut store = CatalogStore::open_absolute(&f.data).unwrap();
    let before = f.bytes();
    fs::set_permissions(&f.data, fs::Permissions::from_mode(0o500)).unwrap();
    assert!(matches!(
        store.create_managed_repository(f.project, &root, "interrupted"),
        Err(Error::CreationUncertain)
    ));
    fs::set_permissions(&f.data, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(f.bytes(), before);
    assert!(root.permitted().inspect(Path::new("interrupted")).is_ok());
    assert!(matches!(
        store.create_managed_repository(f.project, &root, "interrupted"),
        Err(Error::RepositoryExists)
    ));
    let id = store
        .attach_repository(
            f.project,
            &PermittedRoot::open_absolute(&f.root).unwrap(),
            Path::new("interrupted"),
        )
        .unwrap();
    assert_eq!(
        store.load().unwrap().unwrap().projects()[0].repositories(),
        &[id]
    );
}

#[test]
fn encoded_capacity_after_initialization_requires_inspection_before_retry() {
    let f = Fixture::new();
    let root = f.managed();
    let mut store = CatalogStore::open_absolute(&f.data).unwrap();
    let mut catalog = store.load().unwrap().unwrap();
    let mut projects = vec![f.project];
    for i in 1..crate::MAX_PROJECTS {
        projects.push(catalog.create_project(&format!("Project {i}")).unwrap());
    }
    // Shared checkouts can exhaust encoded bytes well before repository count.
    // Build real, valid long paths; no synthetic filesystem identities are used.
    let parent = PathBuf::from("a".repeat(180))
        .join("b".repeat(180))
        .join("c".repeat(180));
    fs::create_dir_all(f.root.join(&parent)).unwrap();
    let target_bytes = crate::codec::MAX_BYTES - 32;
    'repositories: for i in 0..16 {
        let name = format!("existing-{i}");
        root.initialize(&name).unwrap();
        let path = parent.join(&name);
        fs::rename(f.root.join(&name), f.root.join(&path)).unwrap();
        for &project in &projects {
            let mut next = catalog.clone();
            next.attach(project, root.permitted(), &path).unwrap();
            match crate::codec::encode(&next) {
                Ok(bytes) if bytes.len() <= target_bytes => catalog = next,
                Ok(_) | Err(Error::Capacity) => break 'repositories,
                Err(error) => panic!("invalid capacity fixture: {error}"),
            }
        }
    }
    for &project in &projects {
        let length = crate::codec::encode(&catalog).unwrap().len();
        assert!(length <= target_bytes);
        let name = catalog
            .projects()
            .iter()
            .find(|p| p.id() == project)
            .unwrap()
            .name();
        let padding = (target_bytes - length).min(crate::MAX_NAME_BYTES - name.len());
        catalog
            .rename_project(project, &format!("{name}{}", "x".repeat(padding)))
            .unwrap();
        if padding == 0 {
            break;
        }
    }
    assert_eq!(crate::codec::encode(&catalog).unwrap().len(), target_bytes);
    assert!(catalog.repositories().len() < crate::MAX_REPOSITORIES);
    store.save(&catalog).unwrap();
    let before = f.bytes();
    assert!(matches!(
        store.create_managed_repository(f.project, &root, "new-service"),
        Err(Error::CreationUncertain)
    ));
    assert_eq!(f.bytes(), before);
    assert!(root.permitted().inspect(Path::new("new-service")).is_ok());
    assert!(matches!(
        store.create_managed_repository(f.project, &root, "new-service"),
        Err(Error::RepositoryExists)
    ));
    assert_eq!(f.bytes(), before);
}
#[test]
fn existing_bindings_and_cross_project_associations_survive_new_creation() {
    let f = Fixture::new();
    let root = f.managed();
    let mut store = CatalogStore::open_absolute(&f.data).unwrap();
    let first = store
        .create_managed_repository(f.project, &root, "first")
        .unwrap();
    let other = store.create_project("Other").unwrap();
    assert_eq!(
        store
            .attach_repository(other, root.permitted(), Path::new("first"))
            .unwrap(),
        first
    );
    let second = store
        .create_managed_repository(f.project, &root, "second")
        .unwrap();
    assert_ne!(first, second);
    let saved = store.load().unwrap().unwrap();
    assert_eq!(saved.projects()[0].repositories(), &[first, second]);
    assert_eq!(saved.projects()[1].repositories(), &[first]);
}

#[test]
fn metadata_depth_limit_is_checked_before_creating_any_checkout() {
    let f = Fixture::new();
    let mut path = f.root.clone();
    while path.to_str().unwrap().split('/').count() < 62 {
        path.push("d");
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let root = ManagedRoot::open_absolute(&path).unwrap();
    assert!(
        CatalogStore::open_absolute(&f.data)
            .unwrap()
            .create_managed_repository(f.project, &root, "too-deep")
            .is_err()
    );
    assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
}
