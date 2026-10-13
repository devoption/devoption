use super::*;
use std::{collections::BTreeMap, os::unix::fs::symlink};

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Disposable fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Disposable fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .current_dir(path)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn fixture() -> (tempfile::TempDir, PathBuf, PermittedRoot) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::create_dir(root.join("empty-template")).unwrap();
    fs::create_dir(root.join("app")).unwrap();
    git(
        &root.join("app"),
        &[
            "init",
            "--initial-branch=main",
            "--template=../empty-template",
        ],
    );
    let permitted = PermittedRoot::open_absolute(&root).unwrap();
    (temp, root.join("app"), permitted)
}

fn commit(path: &Path, content: &str) -> String {
    fs::write(path.join("tracked.txt"), content).unwrap();
    git(path, &["add", "tracked.txt"]);
    git(path, &["commit", "-m", "Disposable fixture"]);
    git(path, &["rev-parse", "HEAD"])
}

fn files(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            result.extend(files(&entry.path()));
        } else {
            result.insert(entry.path(), fs::read(entry.path()).unwrap());
        }
    }
    result
}

#[test]
fn actual_git_unborn_clean_dirty_and_refresh_preserve_source_metadata() {
    let (_temp, app, root) = fixture();
    let read = || root.git_status(Path::new("app")).unwrap();
    assert_eq!(
        read(),
        GitStatus {
            branch: Some("main".into()),
            commit: None,
            changes: ChangeCounts::default()
        }
    );
    let hash = commit(&app, "first\n");
    assert_eq!(read().commit.as_deref(), Some(hash.as_str()));
    assert_eq!(read().changes, ChangeCounts::default());
    fs::write(app.join("tracked.txt"), "staged\n").unwrap();
    git(&app, &["add", "tracked.txt"]);
    fs::write(app.join("tracked.txt"), "staged then modified\n").unwrap();
    fs::create_dir(app.join("untracked-directory")).unwrap();
    fs::write(app.join("untracked-directory/one"), "fixture").unwrap();
    fs::write(app.join("untracked-directory/two"), "fixture").unwrap();
    fs::create_dir(app.join(".git/info")).unwrap();
    fs::write(app.join(".git/info/exclude"), "ignored.txt\n").unwrap();
    fs::write(app.join("ignored.txt"), "fixture").unwrap();
    let before = files(&app);
    let index_time = fs::metadata(app.join(".git/index"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        read().changes,
        ChangeCounts {
            staged: 1,
            modified: 1,
            untracked: 1,
            conflicted: 0
        }
    );
    assert_eq!(files(&app), before);
    assert_eq!(
        fs::metadata(app.join(".git/index"))
            .unwrap()
            .modified()
            .unwrap(),
        index_time
    );
    fs::write(app.join("tracked.txt"), "staged\n").unwrap();
    assert_eq!(read().changes.modified, 0);
}

#[test]
fn packed_refs_detached_head_and_linked_checkout_resolve_actual_commits() {
    let (_temp, app, root) = fixture();
    let hash = commit(&app, "committed\n");
    git(&app, &["pack-refs", "--all"]);
    assert!(!app.join(".git/refs/heads/main").exists());
    assert_eq!(
        root.git_status(Path::new("app")).unwrap().commit,
        Some(hash.clone())
    );
    git(&app, &["worktree", "add", "-b", "feature", "../linked"]);
    let linked = root.git_status(Path::new("linked")).unwrap();
    assert_eq!(linked.branch.as_deref(), Some("feature"));
    assert_eq!(linked.commit.as_deref(), Some(hash.as_str()));
    assert_eq!(linked.changes, ChangeCounts::default());
    git(&app, &["checkout", "--detach"]);
    let detached = root.git_status(Path::new("app")).unwrap();
    assert_eq!(detached.branch, None);
    assert_eq!(detached.commit, Some(hash));
}

#[test]
fn conflict_index_is_counted_without_double_counting_staged_or_modified() {
    let (_temp, app, root) = fixture();
    let base = commit(&app, "base\n");
    git(&app, &["checkout", "-b", "feature"]);
    commit(&app, "feature\n");
    git(&app, &["checkout", "main"]);
    commit(&app, "main\n");
    // Install three conflict stages from real blobs without running merge drivers.
    let stages = [
        base,
        git(&app, &["rev-parse", "main"]),
        git(&app, &["rev-parse", "feature"]),
    ];
    git(&app, &["update-index", "--force-remove", "tracked.txt"]);
    let mut command = Command::new("git");
    let mut child = command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .current_dir(&app)
        .args(["update-index", "--index-info"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    for (index, hash) in stages.iter().enumerate() {
        let blob = git(&app, &["rev-parse", &format!("{hash}:tracked.txt")]);
        writeln!(
            child.stdin.as_mut().unwrap(),
            "100644 {blob} {}\ttracked.txt",
            index + 1
        )
        .unwrap();
    }
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    assert_eq!(
        root.git_status(Path::new("app")).unwrap().changes,
        ChangeCounts {
            conflicted: 1,
            ..ChangeCounts::default()
        }
    );
}

#[test]
fn source_executable_configuration_is_never_run_and_unsupported_rules_fail_closed() {
    let (_temp, app, root) = fixture();
    commit(&app, "fixture\n");
    git(&app, &["config", "core.fsmonitor", "touch MUST-NOT-EXIST"]);
    assert_eq!(
        root.git_status(Path::new("app")).unwrap().changes,
        ChangeCounts::default()
    );
    assert!(!app.join("MUST-NOT-EXIST").exists());
    let original = fs::read_to_string(app.join(".git/config")).unwrap();
    for extra in [
        "[filter \"fixture\"]\nclean = touch MUST-NOT-EXIST\n",
        "[core]\nsparseCheckout = true\n",
        "[core]\nattributesFile = outside\n",
        "[core]\nexcludesFile = outside\n",
    ] {
        fs::write(app.join(".git/config"), format!("{original}\n{extra}")).unwrap();
        assert_eq!(
            root.git_status(Path::new("app")),
            Err(Error::UnsupportedLayout)
        );
        assert!(!app.join("MUST-NOT-EXIST").exists());
    }
    fs::write(app.join(".git/config"), original).unwrap();
    for path in [".gitmodules", ".git/shallow"] {
        fs::write(app.join(path), "fixture").unwrap();
        assert_eq!(
            root.git_status(Path::new("app")),
            Err(Error::UnsupportedLayout)
        );
        fs::remove_file(app.join(path)).unwrap();
    }
    fs::create_dir(app.join(".git/info")).unwrap();
    fs::write(app.join(".git/info/attributes"), "* -text\n").unwrap();
    assert_eq!(
        root.git_status(Path::new("app")),
        Err(Error::UnsupportedLayout)
    );
}

#[test]
fn missing_objects_unsafe_index_and_metadata_limits_never_return_clean() {
    let (_temp, app, root) = fixture();
    commit(&app, "fixture\n");
    let index = fs::read(app.join(".git/index")).unwrap();
    fs::write(app.join(".git/index"), vec![0; MAX_INDEX + 1]).unwrap();
    assert_eq!(root.git_status(Path::new("app")), Err(Error::LimitExceeded));
    fs::remove_file(app.join(".git/index")).unwrap();
    fs::write(app.join("saved-index"), index).unwrap();
    symlink(app.join("saved-index"), app.join(".git/index")).unwrap();
    assert!(root.git_status(Path::new("app")).is_err());
    fs::remove_file(app.join(".git/index")).unwrap();
    fs::rename(app.join("saved-index"), app.join(".git/index")).unwrap();
    fs::write(
        app.join(".git/refs/heads/main"),
        format!("{}\n", "a".repeat(40)),
    )
    .unwrap();
    assert_eq!(root.git_status(Path::new("app")), Err(Error::Unavailable));
}

#[test]
fn process_and_porcelain_failures_are_bounded() {
    assert_eq!(
        run(Command::new("/bin/sh").args(["-c", "exit 7"]), DEADLINE),
        Err(Error::Unavailable)
    );
    assert_eq!(
        run(&mut Command::new("/devoption-missing-git"), DEADLINE),
        Err(Error::Unavailable)
    );
    assert_eq!(
        run(
            Command::new("/bin/sh").args(["-c", "while :; do :; done"]),
            Duration::from_millis(30)
        ),
        Err(Error::LimitExceeded)
    );
    assert_eq!(
        run(
            Command::new("/bin/sh").args(["-c", "while :; do printf '0123456789'; done"]),
            DEADLINE
        ),
        Err(Error::LimitExceeded)
    );
    for invalid in [&b" M path"[..], b"R  old\0new\0", b"   empty\0", b"?? \0"] {
        assert_eq!(parse(invalid), Err(Error::InvalidLayout));
    }
    assert_eq!(
        parse(&b"?? path\0".repeat(MAX_RECORDS + 1)),
        Err(Error::LimitExceeded)
    );
    assert_eq!(parse(b"?? \xff\nname\0").unwrap().untracked, 1);
}

#[test]
fn split_index_is_unavailable_without_copying_shared_index_dependencies() {
    let (_temp, app, root) = fixture();
    commit(&app, "fixture\n");
    git(&app, &["update-index", "--split-index"]);
    let before = files(&app);
    assert_eq!(root.git_status(Path::new("app")), Err(Error::Unavailable));
    assert_eq!(files(&app), before);
}

#[test]
fn symlink_emulation_preserves_clean_and_modified_status() {
    let (_temp, app, root) = fixture();
    commit(&app, "fixture\n");
    symlink("tracked.txt", app.join("link")).unwrap();
    git(&app, &["add", "link"]);
    git(&app, &["commit", "-m", "Disposable symlink fixture"]);
    git(&app, &["config", "core.symlinks", "false"]);
    fs::remove_file(app.join("link")).unwrap();
    fs::write(app.join("link"), "tracked.txt").unwrap();
    assert!(git(&app, &["status", "--porcelain"]).is_empty());
    let before = files(&app);
    assert_eq!(
        root.git_status(Path::new("app")).unwrap().changes,
        ChangeCounts::default()
    );
    assert_eq!(files(&app), before);
    fs::write(app.join("link"), "another-target.txt").unwrap();
    assert_eq!(
        root.git_status(Path::new("app")).unwrap().changes.modified,
        1
    );
}
