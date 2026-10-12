use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

const CONFIG: &str = "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n[remote \"origin\"]\n\turl = https://example.invalid/fixture.git\n";

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let root = temp.path().canonicalize().unwrap();
    main_tree(&root, "main");
    (temp, root)
}

fn directory(path: &Path) {
    if path.is_dir() {
        return;
    }
    directory(path.parent().unwrap());
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn write(path: &Path, content: impl AsRef<[u8]>) {
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
}

fn main_tree(root: &Path, name: &str) {
    let tree = root.join(name);
    directory(&tree.join(".git/objects/info"));
    directory(&tree.join(".git/refs/heads"));
    write(&tree.join(".git/config"), CONFIG);
    write(&tree.join(".git/HEAD"), "ref: refs/heads/main\n");
}

fn linked_tree(root: &Path, name: &str) {
    directory(&root.join(name));
    let git = root.join("main/.git/worktrees").join(name);
    directory(&git);
    write(
        &root.join(name).join(".git"),
        format!("gitdir: {}\n", git.display()),
    );
    write(&git.join("commondir"), "../..\n");
    write(
        &git.join("gitdir"),
        format!("{}\n", root.join(name).join(".git").display()),
    );
    write(&git.join("HEAD"), "ref: refs/heads/feature\n");
}

#[test]
fn main_and_linked_worktrees_have_stable_distinct_and_shared_identities() {
    let (_temp, root) = fixture();
    linked_tree(&root, "linked");
    let permitted = PermittedRoot::open_absolute(&root).unwrap();
    let main = permitted.inspect(Path::new("main")).unwrap();
    let linked = permitted.inspect(Path::new("linked")).unwrap();
    assert_eq!(main.kind, WorktreeKind::Main);
    assert_eq!(linked.kind, WorktreeKind::Linked);
    assert_eq!(main.common_directory, linked.common_directory);
    assert_ne!(main.git_directory, linked.git_directory);
    assert_ne!(main.worktree, linked.worktree);
    assert_eq!(main.path, root.join("main"));
    assert_eq!(
        main,
        PermittedRoot::open_absolute(&root)
            .unwrap()
            .inspect(Path::new("main"))
            .unwrap()
    );
    // Git permits relative gitfile pointers; metadata parent steps stay in root.
    write(
        &root.join("linked/.git"),
        "gitdir: ../main/.git/worktrees/linked\n",
    );
    assert_eq!(linked, permitted.inspect(Path::new("linked")).unwrap());
    fs::rename(root.join("main"), root.join("renamed")).unwrap();
    let renamed = permitted.inspect(Path::new("renamed")).unwrap();
    assert_eq!(main.worktree, renamed.worktree);
    assert_eq!(main.common_directory, renamed.common_directory);
    assert!(permitted.inspect(Path::new("linked")).is_err());
}

#[test]
fn explicit_root_and_normalized_candidates_are_required_without_parent_discovery() {
    let (_temp, root) = fixture();
    for path in [
        Path::new("/"),
        Path::new("relative"),
        &root.join(".."),
        &root.join("."),
    ] {
        assert!(PermittedRoot::open_absolute(path).is_err());
    }
    let permitted = PermittedRoot::open_absolute(&root).unwrap();
    for candidate in [
        "",
        ".",
        "..",
        "main/..",
        "main/../main",
        "main//.git",
        "main/.",
        "/main",
        "main/",
        "main\n",
        "missing",
    ] {
        assert!(
            permitted.inspect(Path::new(candidate)).is_err(),
            "{candidate:?}"
        );
    }
    directory(&root.join("main/subdirectory"));
    assert!(permitted.inspect(Path::new("main/subdirectory")).is_err());
    assert!(permitted.inspect(Path::new("main/.git")).is_err());
    assert!(
        permitted
            .inspect(Path::new(&"a".repeat(MAX_PATH + 1)))
            .is_err()
    );
    assert!(
        permitted
            .inspect(Path::new(&vec!["a"; MAX_COMPONENTS + 1].join("/")))
            .is_err()
    );
}

#[test]
fn symlinks_hardlinks_and_writable_entries_fail_closed() {
    let (_temp, root) = fixture();
    symlink(root.join("main"), root.join("alias")).unwrap();
    assert!(PermittedRoot::open_absolute(&root.join("alias")).is_err());
    assert!(PermittedRoot::open_absolute(&root.join("alias/.git")).is_err());
    let permitted = PermittedRoot::open_absolute(&root).unwrap();
    assert!(permitted.inspect(Path::new("alias")).is_err());
    for target in [
        "main",
        "main/.git",
        "main/.git/objects",
        "main/.git/config",
        "main/.git/HEAD",
    ] {
        let path = root.join(target);
        let original = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(permitted.inspect(Path::new("main")).is_err());
        fs::set_permissions(&path, original).unwrap();
        let moved = root.join("preserved");
        fs::rename(&path, &moved).unwrap();
        symlink(&moved, &path).unwrap();
        assert!(permitted.inspect(Path::new("main")).is_err());
        fs::remove_file(&path).unwrap();
        fs::rename(&moved, &path).unwrap();
    }
    fs::hard_link(root.join("main/.git/HEAD"), root.join("hardlink")).unwrap();
    assert!(permitted.inspect(Path::new("main")).is_err());
    fs::remove_file(root.join("hardlink")).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o770)).unwrap();
    assert!(PermittedRoot::open_absolute(&root).is_err());
    assert!(permitted.inspect(Path::new("main")).is_err());
}

#[test]
fn all_gitfile_targets_common_directories_and_backlinks_stay_in_root() {
    let (_temp, root) = fixture();
    linked_tree(&root, "linked");
    let permitted = PermittedRoot::open_absolute(&root).unwrap();
    let marker = root.join("linked/.git");
    let original = fs::read(&marker).unwrap();
    for pointer in [
        "../../outside".to_owned(),
        format!("{}-sibling/repo", root.display()),
        "missing/../main/.git/worktrees/linked".into(),
    ] {
        write(&marker, format!("gitdir: {pointer}\n"));
        assert!(permitted.inspect(Path::new("linked")).is_err());
    }
    write(&marker, original);
    let admin = root.join("main/.git/worktrees/linked");
    for (file, invalid, good) in [
        ("commondir", "../../../../../outside\n", "../..\n"),
        ("commondir", ".\n", "../..\n"),
        (
            "gitdir",
            "../../../../main/.git\n",
            &format!("{}\n", marker.display()),
        ),
    ] {
        write(&admin.join(file), invalid);
        assert!(permitted.inspect(Path::new("linked")).is_err());
        write(&admin.join(file), good);
    }
    write(&root.join("main/.git/commondir"), "../..\n");
    assert!(permitted.inspect(Path::new("linked")).is_err());
}

#[test]
fn metadata_bounds_and_supported_format_fail_closed() {
    let (_temp, root) = fixture();
    linked_tree(&root, "linked");
    let permitted = PermittedRoot::open_absolute(&root).unwrap();
    for (name, limit) in [
        ("main/.git/config", MAX_CONFIG),
        ("main/.git/HEAD", MAX_PATH),
        ("linked/.git", MAX_PATH),
        ("main/.git/worktrees/linked/commondir", MAX_PATH),
        ("main/.git/worktrees/linked/gitdir", MAX_PATH),
    ] {
        let path = root.join(name);
        let original = fs::read(&path).unwrap();
        let candidate = if name.starts_with("main/.git/") && !name.contains("worktrees") {
            "main"
        } else {
            "linked"
        };
        for content in [vec![b'a'; limit + 1], vec![0xff], Vec::new()] {
            write(&path, content);
            assert!(permitted.inspect(Path::new(candidate)).is_err());
        }
        fs::remove_file(&path).unwrap();
        assert!(permitted.inspect(Path::new(candidate)).is_err());
        write(&path, original);
    }
    for config in [
        CONFIG.replace("= 0", "= 1"),
        CONFIG.replace("bare = false", "bare = true"),
        format!("{CONFIG}[include]\npath = /private-fixture\n"),
        format!("{CONFIG}[extensions]\nworktreeConfig = true\n"),
        format!("{CONFIG}[core]\nworktree = ../outside\n"),
        format!("{CONFIG}[core]\nrepositoryformatversion = 0\n"),
        format!("{CONFIG}[remote \"other\"]\nurl = \"unterminated\n"),
    ] {
        write(&root.join("main/.git/config"), config);
        assert!(permitted.inspect(Path::new("main")).is_err());
    }
    write(&root.join("main/.git/config"), CONFIG);
    for head in [
        "ref: refs/heads/../outside\n",
        "ref: refs/tags/tag\n",
        "ref: refs/heads/a.lock\n",
        "ref: refs/heads/a@{b\n",
        "ref: refs/heads/main\nextra\n",
        "not-an-object\n",
    ] {
        write(&root.join("main/.git/HEAD"), head);
        assert!(permitted.inspect(Path::new("main")).is_err());
    }
    write(
        &root.join("main/.git/HEAD"),
        format!("{}\n", "a".repeat(40)),
    );
    assert!(permitted.inspect(Path::new("main")).is_ok());
}

#[test]
fn special_files_and_unsupported_layouts_are_not_followed() {
    let (_temp, root) = fixture();
    linked_tree(&root, "linked");
    let permitted = PermittedRoot::open_absolute(&root).unwrap();
    let head = root.join("main/.git/HEAD");
    fs::remove_file(&head).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&head)
            .status()
            .unwrap()
            .success()
    );
    fs::set_permissions(&head, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(permitted.inspect(Path::new("main")).is_err());
    fs::remove_file(&head).unwrap();
    write(&head, "ref: refs/heads/main\n");
    for file in [
        "main/.git/objects/info/alternates",
        "main/.git/objects/info/http-alternates",
        "main/.git/config.worktree",
    ] {
        write(&root.join(file), "unread external fixture\n");
        assert!(permitted.inspect(Path::new("main")).is_err());
        assert!(permitted.inspect(Path::new("linked")).is_err());
        fs::remove_file(root.join(file)).unwrap();
    }
    directory(&root.join("separate"));
    write(&root.join("separate/.git"), "gitdir: ../main/.git\n");
    assert!(permitted.inspect(Path::new("separate")).is_err());
}

#[test]
fn inspection_does_not_read_hooks_working_files_or_write_metadata() {
    let (_temp, root) = fixture();
    directory(&root.join("main/.git/hooks"));
    let hook = root.join("main/.git/hooks/post-checkout");
    write(&hook, "#!/bin/sh\nexit 99\n");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o000)).unwrap();
    symlink("/unread-fixture", root.join("main/working-file")).unwrap();
    let before: Vec<_> = ["config", "HEAD"]
        .iter()
        .map(|file| fs::read(root.join("main/.git").join(file)).unwrap())
        .collect();
    assert!(
        PermittedRoot::open_absolute(&root)
            .unwrap()
            .inspect(Path::new("main"))
            .is_ok()
    );
    let after: Vec<_> = ["config", "HEAD"]
        .iter()
        .map(|file| fs::read(root.join("main/.git").join(file)).unwrap())
        .collect();
    assert_eq!(before, after);
    assert!(!root.join("main/.git/index").exists());
}

#[test]
fn replacing_the_permitted_root_does_not_redirect_a_held_capability() {
    let (_temp, root) = fixture();
    let selected = root.join("selected");
    directory(&selected);
    main_tree(&selected, "main");
    let permitted = PermittedRoot::open_absolute(&selected).unwrap();
    fs::rename(&selected, root.join("moved")).unwrap();
    directory(&selected);
    main_tree(&selected, "main");
    assert_eq!(
        permitted.inspect(Path::new("main")).unwrap_err(),
        InspectionError::Changed
    );
}
