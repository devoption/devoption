use super::*;
use crate::{Catalog, CatalogStore, worktree::ManagedRoot};
use std::os::unix::fs::{PermissionsExt, symlink};

fn fixture() -> (tempfile::TempDir, PathBuf, PermittedRoot) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    ManagedRoot::open_absolute(&root)
        .unwrap()
        .initialize("app")
        .unwrap();
    let permitted = PermittedRoot::open_absolute(&root).unwrap();
    (temp, root.join("app"), permitted)
}

fn git(app: &Path, args: &[&str]) {
    let output = Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .current_dir(app)
        .args(["-c", "core.hooksPath=/dev/null", "--literal-pathspecs"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn real_index_lists_tracked_paths_only_and_reads_current_text_without_source_writes() {
    let (_temp, app, root) = fixture();
    assert!(
        root.repository_files(Path::new("app"), None)
            .unwrap()
            .entries
            .is_empty()
    );
    fs::create_dir(app.join("src")).unwrap();
    fs::write(app.join("src/main.rs"), "staged\n").unwrap();
    fs::write(app.join("résumé & notes.txt"), "hello\t世界\n").unwrap();
    git(&app, &["add", "src/main.rs", "résumé & notes.txt"]);
    fs::write(app.join("src/main.rs"), "<script>working tree</script>\n").unwrap();
    fs::write(app.join("untracked-secret"), "must not display").unwrap();
    let index = fs::read(app.join(".git/index")).unwrap();
    let config = fs::read(app.join(".git/config")).unwrap();
    let result = root
        .repository_files(Path::new("app"), Some("src/main.rs"))
        .unwrap();
    assert_eq!(
        result
            .entries
            .iter()
            .map(|e| e.path.as_str())
            .collect::<Vec<_>>(),
        ["résumé & notes.txt", "src/main.rs"]
    );
    assert_eq!(
        result.selected.unwrap().1,
        FileContent::Text("<script>working tree</script>\n".into())
    );
    assert_eq!(
        root.repository_files(Path::new("app"), Some("untracked-secret")),
        Err(Error::InvalidPath)
    );
    assert_eq!(fs::read(app.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(app.join(".git/config")).unwrap(), config);
    assert_eq!(
        fs::read_to_string(app.join("untracked-secret")).unwrap(),
        "must not display"
    );
    // No source object is needed to browse the private index copy.
    fs::rename(app.join(".git/objects"), app.join(".git/objects-preserved")).unwrap();
    fs::create_dir(app.join(".git/objects")).unwrap();
    assert!(
        root.repository_files(Path::new("app"), Some("src/main.rs"))
            .is_ok()
    );
}

#[test]
fn text_binary_size_and_missing_states_are_explicit() {
    let (_temp, app, root) = fixture();
    for (name, bytes) in [
        ("empty", vec![]),
        ("binary", vec![0, 255]),
        ("control", b"one\x1btwo".to_vec()),
        ("large", vec![b'x'; MAX_FILE_BYTES + 1]),
        ("maximum", vec![b'x'; MAX_FILE_BYTES]),
        ("missing", b"gone".to_vec()),
    ] {
        fs::write(app.join(name), bytes).unwrap();
    }
    git(&app, &["add", "."]);
    fs::remove_file(app.join("missing")).unwrap();
    let read = |path| {
        root.repository_files(Path::new("app"), Some(path))
            .unwrap()
            .selected
            .unwrap()
            .1
    };
    assert_eq!(read("empty"), FileContent::Text(String::new()));
    assert_eq!(read("binary"), FileContent::Binary);
    assert_eq!(read("control"), FileContent::Binary);
    assert_eq!(read("large"), FileContent::TooLarge);
    assert_eq!(
        read("maximum"),
        FileContent::Text("x".repeat(MAX_FILE_BYTES))
    );
    assert_eq!(read("missing"), FileContent::Unavailable);
}

#[test]
fn tracked_symlinks_and_replaced_paths_never_read_their_targets() {
    let (_temp, app, root) = fixture();
    fs::write(app.parent().unwrap().join("outside"), "outside secret").unwrap();
    fs::write(app.join("regular"), "original").unwrap();
    fs::create_dir(app.join("dir")).unwrap();
    fs::write(app.join("dir/file"), "original").unwrap();
    symlink("../outside", app.join("link")).unwrap();
    git(&app, &["add", "."]);
    assert_eq!(
        root.repository_files(Path::new("app"), Some("link"))
            .unwrap()
            .selected
            .unwrap()
            .1,
        FileContent::Unsupported
    );
    fs::remove_file(app.join("regular")).unwrap();
    symlink("../outside", app.join("regular")).unwrap();
    assert_eq!(
        root.repository_files(Path::new("app"), Some("regular"))
            .unwrap()
            .selected
            .unwrap()
            .1,
        FileContent::Unavailable
    );
    fs::remove_file(app.join("regular")).unwrap();
    fs::hard_link(app.parent().unwrap().join("outside"), app.join("regular")).unwrap();
    assert_eq!(
        root.repository_files(Path::new("app"), Some("regular"))
            .unwrap()
            .selected
            .unwrap()
            .1,
        FileContent::Unavailable
    );
    fs::rename(app.join("dir"), app.join("saved-dir")).unwrap();
    symlink("..", app.join("dir")).unwrap();
    assert_eq!(
        root.repository_files(Path::new("app"), Some("dir/file"))
            .unwrap()
            .selected
            .unwrap()
            .1,
        FileContent::Unavailable
    );
    for path in [
        "../outside",
        "/outside",
        ".git/config",
        "dir/../regular",
        "dir//file",
        "dir\\file",
    ] {
        assert!(root.repository_files(Path::new("app"), Some(path)).is_err());
    }
}

#[test]
fn malformed_conflicted_and_oversized_indexes_never_return_partial_lists() {
    let line = |mode: &str, stage: &str, path: &str| {
        format!("{mode} {} {stage}\t{path}\0", "1".repeat(40))
    };
    for bytes in [
        line("100644", "1", "file"),
        line("100644", "0", "../outside"),
        line("100644", "0", ".git/config"),
        line("unknown", "0", "file"),
        "truncated".into(),
        format!(
            "{}{}",
            line("100644", "0", "file"),
            line("100644", "0", "file")
        ),
    ] {
        assert!(parse(bytes.as_bytes()).is_err());
    }
    let mut bytes = String::new();
    for i in 0..MAX_FILES {
        bytes.push_str(&line("100644", "0", &format!("file{i}")));
    }
    assert_eq!(parse(bytes.as_bytes()).unwrap().len(), MAX_FILES);
    bytes.push_str(&line("100644", "0", "overflow"));
    assert_eq!(parse(bytes.as_bytes()), Err(Error::LimitExceeded));
    let (_temp, app, root) = fixture();
    fs::write(app.join(".git/index"), vec![0; super::super::MAX_INDEX + 1]).unwrap();
    assert_eq!(
        root.repository_files(Path::new("app"), None),
        Err(Error::LimitExceeded)
    );
}

#[test]
fn catalog_scope_and_changed_checkout_are_revalidated_on_every_read() {
    let (_temp, app, root) = fixture();
    let data = tempfile::tempdir().unwrap();
    let data = data.path().canonicalize().unwrap();
    fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
    let mut catalog = Catalog::default();
    let project = catalog.create_project("Fixture").unwrap();
    let other = catalog.create_project("Other").unwrap();
    let mut store = CatalogStore::open_absolute(&data).unwrap();
    store.save(&catalog).unwrap();
    let repository = store
        .attach_repository(project, &root, Path::new("app"))
        .unwrap();
    let catalog = store.load().unwrap().unwrap();
    assert!(matches!(
        catalog.repository_files(other, repository, &root, None),
        Err(crate::Error::UnknownRepository)
    ));
    assert!(
        catalog
            .repository_files(project, repository, &root, None)
            .unwrap()
            .entries
            .is_empty()
    );
    fs::write(app.join("new"), "new").unwrap();
    git(&app, &["add", "new"]);
    assert_eq!(
        catalog
            .repository_files(project, repository, &root, None)
            .unwrap()
            .entries
            .len(),
        1
    );
    fs::rename(&app, app.with_file_name("preserved")).unwrap();
    ManagedRoot::open_absolute(app.parent().unwrap())
        .unwrap()
        .initialize("app")
        .unwrap();
    assert!(matches!(
        catalog.repository_files(project, repository, &root, None),
        Err(crate::Error::StaleCheckout)
    ));
}
