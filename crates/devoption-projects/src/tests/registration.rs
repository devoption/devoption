use crate::{
    Catalog, CatalogStore, Error, ProjectId, codec, read_catalog, worktree::PermittedRoot,
};
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

fn directory(path: &Path) {
    if path.is_dir() {
        return;
    }
    directory(path.parent().unwrap());
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn main_tree(root: &Path, name: &str) {
    let tree = root.join(name);
    directory(&tree.join(".git/objects"));
    directory(&tree.join(".git/refs"));
    fs::write(
        tree.join(".git/config"),
        "[core]\nrepositoryformatversion = 0\nbare = false\n",
    )
    .unwrap();
    fs::write(tree.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
}

fn linked_tree(root: &Path) {
    directory(&root.join("linked"));
    let git = root.join("main/.git/worktrees/linked");
    directory(&git);
    fs::write(
        root.join("linked/.git"),
        "gitdir: ../main/.git/worktrees/linked\n",
    )
    .unwrap();
    fs::write(git.join("commondir"), "../..\n").unwrap();
    fs::write(git.join("gitdir"), "../../../../linked/.git\n").unwrap();
    fs::write(git.join("HEAD"), "ref: refs/heads/feature\n").unwrap();
}

struct Fixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    data: PathBuf,
    first: ProjectId,
    second: ProjectId,
}
impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let base = temporary.path().canonicalize().unwrap();
        let root = base.join("repositories");
        let data = base.join("data");
        directory(&root);
        directory(&data);
        main_tree(&root, "main");
        linked_tree(&root);
        main_tree(&root, "other");
        let mut catalog = Catalog::default();
        let first = catalog.create_project("First").unwrap();
        let second = catalog.create_project("Second").unwrap();
        let metadata = catalog.create_repository("Legacy metadata").unwrap();
        catalog.associate(first, metadata).unwrap();
        CatalogStore::open_absolute(&data)
            .unwrap()
            .save(&catalog)
            .unwrap();
        Self {
            _temporary: temporary,
            root,
            data,
            first,
            second,
        }
    }
    fn root(&self) -> PermittedRoot {
        PermittedRoot::open_absolute(&self.root).unwrap()
    }
    fn bytes(&self) -> Vec<u8> {
        fs::read(self.data.join("projects.bin")).unwrap()
    }
}

#[test]
fn legacy_bytes_survive_reads_and_metadata_writes_then_migrate_without_identity_loss() {
    let f = Fixture::new();
    let original = read_catalog(&f.data).unwrap().unwrap();
    let bytes = f.bytes();
    assert_eq!(bytes[8], 1);
    let mut store = CatalogStore::open_absolute(&f.data).unwrap();
    store.save(&original).unwrap();
    assert_eq!(f.bytes(), bytes);
    let repository = store
        .attach_repository(f.first, &f.root(), Path::new("main"))
        .unwrap();
    let saved = store.load().unwrap().unwrap();
    assert_eq!(f.bytes()[8], 2);
    assert_eq!(&saved.repositories()[..1], original.repositories());
    assert_eq!(saved.projects()[0].id(), original.projects()[0].id());
    assert_eq!(
        saved.projects()[0].repositories(),
        &[original.repositories()[0].id(), repository]
    );
    assert_eq!(
        saved.default_checkouts()[0].relative_path(),
        Path::new("main")
    );
    assert_eq!(saved.repository_root().unwrap().path(), f.root);
    drop(store);
    assert_eq!(read_catalog(&f.data).unwrap().unwrap(), saved);
    let reopened = CatalogStore::open_absolute(&f.data).unwrap();
    assert_eq!(reopened.load().unwrap().unwrap(), saved);
}

#[test]
fn linked_worktrees_share_repository_identity_with_per_project_defaults() {
    let f = Fixture::new();
    let root = f.root();
    let mut store = CatalogStore::open_absolute(&f.data).unwrap();
    let first = store
        .attach_repository(f.first, &root, Path::new("main"))
        .unwrap();
    assert_eq!(
        store
            .attach_repository(f.second, &root, Path::new("linked"))
            .unwrap(),
        first
    );
    let before = f.bytes();
    assert!(matches!(
        store.attach_repository(f.first, &root, Path::new("linked")),
        Err(Error::AlreadyAssociated)
    ));
    assert_eq!(f.bytes(), before);
    let other = store
        .attach_repository(f.first, &root, Path::new("other"))
        .unwrap();
    assert_ne!(first, other);
    let mut saved = store.load().unwrap().unwrap();
    assert_eq!(saved.repositories().len(), 3);
    assert_eq!(saved.repository_bindings().len(), 2);
    assert_eq!(saved.default_checkouts().len(), 3);
    assert_eq!(
        saved.default_checkouts()[1].relative_path(),
        Path::new("linked")
    );
    assert_eq!(saved.default_checkouts()[1].project(), f.second);
    assert_eq!(saved.default_checkouts()[1].repository(), first);
    assert!(matches!(
        saved.associate(f.second, other),
        Err(Error::CheckoutRequired)
    ));
}

#[test]
fn unknown_projects_invalid_paths_and_stale_checkouts_preserve_previous_bytes() {
    let f = Fixture::new();
    let root = f.root();
    let mut store = CatalogStore::open_absolute(&f.data).unwrap();
    store
        .attach_repository(f.first, &root, Path::new("main"))
        .unwrap();
    let before = f.bytes();
    assert!(matches!(
        store.attach_repository(ProjectId::from_bytes([0; 16]), &root, Path::new("main")),
        Err(Error::UnknownProject)
    ));
    for path in ["", "../data", "/main", "main/..", "main//nested", "missing"] {
        assert!(
            store
                .attach_repository(f.second, &root, Path::new(path))
                .is_err()
        );
        assert_eq!(f.bytes(), before);
    }
    // Retain the old objects to avoid relying on inode allocation/reuse behavior.
    fs::rename(f.root.join("main"), f.root.join("old-main")).unwrap();
    main_tree(&f.root, "main");
    assert!(matches!(
        store.attach_repository(f.second, &root, Path::new("main")),
        Err(Error::StaleCheckout)
    ));
    assert_eq!(f.bytes(), before);
}

#[test]
fn saved_roots_cannot_be_replaced_or_overlap_data_in_either_direction() {
    let f = Fixture::new();
    let root = f.root();
    let mut store = CatalogStore::open_absolute(&f.data).unwrap();
    store
        .attach_repository(f.first, &root, Path::new("main"))
        .unwrap();
    let before = f.bytes();
    for candidate in [&f.data, f.data.parent().unwrap()] {
        let overlap = PermittedRoot::open_absolute(candidate).unwrap();
        assert!(matches!(
            store.attach_repository(f.second, &overlap, Path::new("main")),
            Err(Error::RootMismatch)
        ));
    }
    directory(&f.data.join("nested"));
    let nested = PermittedRoot::open_absolute(&f.data.join("nested")).unwrap();
    assert!(matches!(
        store.attach_repository(f.second, &nested, Path::new("main")),
        Err(Error::RootMismatch)
    ));
    let renamed = f.root.with_file_name("old-root");
    fs::rename(&f.root, renamed).unwrap();
    directory(&f.root);
    main_tree(&f.root, "main");
    assert!(
        store
            .attach_repository(f.second, &root, Path::new("main"))
            .is_err()
    );
    assert!(matches!(
        store.attach_repository(f.second, &f.root(), Path::new("main")),
        Err(Error::RootMismatch)
    ));
    assert_eq!(f.bytes(), before);
}

#[test]
fn version_two_corruption_and_inconsistent_bindings_fail_closed() {
    let f = Fixture::new();
    let mut store = CatalogStore::open_absolute(&f.data).unwrap();
    store
        .attach_repository(f.first, &f.root(), Path::new("main"))
        .unwrap();
    let saved = store.load().unwrap().unwrap();
    let bytes = f.bytes();
    for end in 0..bytes.len() {
        assert!(codec::decode(&bytes[..end]).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(codec::decode(&trailing).is_err());
    let mut invalid = saved.clone();
    invalid.checkouts.clear();
    assert!(codec::encode(&invalid).is_err());
    let mut invalid = saved.clone();
    invalid.bindings.push(invalid.bindings[0].clone());
    assert!(codec::encode(&invalid).is_err());
    let mut invalid = saved.clone();
    invalid.checkouts.push(invalid.checkouts[0].clone());
    assert!(codec::encode(&invalid).is_err());
    let mut invalid = saved.clone();
    invalid.checkouts[0].path = "../escape".into();
    assert!(codec::encode(&invalid).is_err());
    fs::write(f.data.join("projects.bin"), &trailing).unwrap();
    assert!(
        store
            .attach_repository(f.second, &f.root(), Path::new("linked"))
            .is_err()
    );
    assert_eq!(f.bytes(), trailing);
}

#[test]
fn interrupted_migration_and_conflicting_writers_never_publish_partial_state() {
    let f = Fixture::new();
    let before = f.bytes();
    let mut store = CatalogStore::open_absolute(&f.data).unwrap();
    assert!(matches!(
        CatalogStore::open_absolute(&f.data),
        Err(Error::Busy)
    ));
    let mut changed = store.load().unwrap().unwrap();
    changed
        .attach(f.first, &f.root(), Path::new("main"))
        .unwrap();
    assert!(
        store
            .save_using(&changed, |file, bytes| {
                file.write_all(&bytes[..bytes.len() / 2])?;
                Err(io::Error::other("fixture write failure"))
            })
            .is_err()
    );
    assert_eq!(f.bytes(), before);
    assert_eq!(
        read_catalog(&f.data)
            .unwrap()
            .unwrap()
            .repository_bindings()
            .len(),
        0
    );
    assert!(fs::read_dir(&f.data).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
}

#[test]
fn encoded_binding_capacity_and_metadata_updates_preserve_saved_registrations() {
    let f = Fixture::new();
    main_tree(&f.root, "third");
    let mut store = CatalogStore::open_absolute(&f.data).unwrap();
    store
        .attach_repository(f.first, &f.root(), Path::new("main"))
        .unwrap();
    store
        .attach_repository(f.first, &f.root(), Path::new("other"))
        .unwrap();
    store
        .attach_repository(f.first, &f.root(), Path::new("third"))
        .unwrap();
    let id = store.create_project("After migration").unwrap();
    let mut saved = store.load().unwrap().unwrap();
    assert_eq!(saved.repository_bindings().len(), 3);
    assert_eq!(saved.projects()[2].id(), id);
    assert_eq!(f.bytes()[8], 2);
    let before = f.bytes();
    // Construct a structurally valid snapshot above the byte budget. The store
    // must reject it without truncating any project or checkout to make it fit.
    for index in saved.projects.len()..crate::MAX_PROJECTS {
        saved.create_project(&format!("Capacity {index}")).unwrap();
    }
    let templates = saved.checkouts.clone();
    saved.checkouts.clear();
    for project in &mut saved.projects {
        for (index, template) in templates.iter().enumerate() {
            if !project.repositories.contains(&template.repository) {
                project.repositories.push(template.repository);
            }
            let mut checkout = template.clone();
            checkout.project = project.id;
            checkout.path = format!("{index}/{}", vec!["a".repeat(240); 16].join("/"));
            saved.checkouts.push(checkout);
        }
    }
    assert!(saved.validate().is_ok());
    assert!(matches!(store.save(&saved), Err(Error::Capacity)));
    assert_eq!(f.bytes(), before);
}
