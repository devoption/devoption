use super::*;
use std::{fs, path::Path, process::Command};

const MANIFEST: &[u8] =
    include_bytes!("../../../devoption-registry/src/cargo_inputs/fixtures/standalone.toml");
const LOCK: &[u8] =
    include_bytes!("../../../devoption-registry/src/cargo_inputs/fixtures/standalone.lock");
const MAIN: &[u8] = b"compile_error!(\"snapshot is never compiled\");\n";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn id(kind: ObjectKind, bytes: &[u8]) -> [u8; 20] {
    cargo_source::object_id(
        match kind {
            ObjectKind::Tree => "tree",
            ObjectKind::Blob => "blob",
        },
        bytes,
    )
    .unwrap()
}
fn entry(mode: &str, name: &str, id: [u8; 20]) -> Vec<u8> {
    [mode.as_bytes(), b" ", name.as_bytes(), b"\0", &id].concat()
}

pub(crate) struct ProofFixture {
    pub commit: Vec<u8>,
    objects: Vec<(ObjectKind, Vec<u8>)>,
}

impl ProofFixture {
    pub fn new() -> Self {
        Self::with_files(&[
            ("Cargo.lock", LOCK),
            ("Cargo.toml", MANIFEST),
            ("src/main.rs", MAIN),
        ])
    }
    fn with_files(files: &[(&str, &[u8])]) -> Self {
        let mut objects = Vec::new();
        let tree = make_tree(files, &mut objects);
        Self {
            commit: commit(tree),
            objects,
        }
    }
    pub fn changed_source() -> Self {
        Self::with_files(&[
            ("Cargo.lock", LOCK),
            ("Cargo.toml", MANIFEST),
            ("src/main.rs", b"different source bytes\n"),
        ])
    }
    pub fn verify(&self) -> Result<SourceSnapshot, Error> {
        verify(
            RegistryId::new("fixture-registry").unwrap(),
            &hex(&cargo_source::object_id("commit", &self.commit).unwrap()),
            &self.commit,
            &self
                .objects
                .iter()
                .map(|(kind, bytes)| Object { kind: *kind, bytes })
                .collect::<Vec<_>>(),
        )
    }
}

fn commit(tree: [u8; 20]) -> Vec<u8> {
    format!("tree {}\nauthor Fixture <fixture@example.invalid> 1 +0000\ncommitter Fixture <fixture@example.invalid> 1 +0000\n\nPublic fixture\n", hex(&tree)).into_bytes()
}

fn add(objects: &mut Vec<(ObjectKind, Vec<u8>)>, kind: ObjectKind, bytes: Vec<u8>) -> [u8; 20] {
    let identity = id(kind, &bytes);
    if !objects.iter().any(|(k, b)| *k == kind && *b == bytes) {
        objects.push((kind, bytes));
    }
    identity
}

fn make_tree(files: &[(&str, &[u8])], objects: &mut Vec<(ObjectKind, Vec<u8>)>) -> [u8; 20] {
    let mut children: BTreeMap<&str, Vec<(&str, &[u8])>> = BTreeMap::new();
    let mut entries = Vec::new();
    for &(path, bytes) in files {
        if let Some((name, rest)) = path.split_once('/') {
            children.entry(name).or_default().push((rest, bytes));
        } else {
            entries.push((
                path.to_owned(),
                entry(
                    "100644",
                    path,
                    add(objects, ObjectKind::Blob, bytes.to_vec()),
                ),
            ));
        }
    }
    for (name, children) in children {
        let tree = make_tree(&children, objects);
        entries.push((format!("{name}/"), entry("40000", name, tree)));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    add(
        objects,
        ObjectKind::Tree,
        entries.into_iter().flat_map(|(_, entry)| entry).collect(),
    )
}

#[test]
fn retains_all_owned_bytes_and_exact_conventional_target() {
    let mut f = ProofFixture::with_files(&[
        ("Cargo.lock", LOCK),
        ("Cargo.toml", MANIFEST),
        ("src/main.rs", MAIN),
        ("src/helper.rs", b"module"),
        ("assets/payload.bin", &[0, 255]),
        ("README.md", b"Public fixture"),
    ]);
    let source = f.verify().unwrap();
    assert_eq!(source.files().len(), 6);
    assert_eq!(
        (
            source.package(),
            source.version(),
            source.binary(),
            source.target_path()
        ),
        ("ExampleService", "0.1.0", "ExampleService", "src/main.rs")
    );
    assert!(source.files().windows(2).all(|f| f[0].path() < f[1].path()));
    for file in source.files() {
        assert_eq!(file.object_id(), id(ObjectKind::Blob, file.bytes()));
        assert!(file.sha256() == Sha256::digest(file.bytes()));
    }
    let original = source.clone();
    f.objects.reverse();
    assert!(f.verify().unwrap() == original);
    f.objects.clear();
    f.commit.fill(0);
    assert!(source == original);
    assert_eq!(
        source
            .files()
            .iter()
            .find(|f| f.path() == "assets/payload.bin")
            .unwrap()
            .bytes(),
        &[0, 255]
    );
}

#[test]
fn changed_source_changes_snapshot_identity_even_with_same_cargo_inputs() {
    let first = ProofFixture::new().verify().unwrap();
    let second = ProofFixture::changed_source().verify().unwrap();
    assert!(first.request() == second.request());
    assert!(first.sha256() != second.sha256());
    assert_ne!(first.commit(), second.commit());
    assert!(first != second);
}

#[test]
fn missing_mutated_wrong_type_duplicate_and_extra_objects_fail_closed() {
    for index in 0..ProofFixture::new().objects.len() {
        let mut f = ProofFixture::new();
        f.objects.remove(index);
        assert_eq!(f.verify().err(), Some(Error::MissingObject));
        let mut f = ProofFixture::new();
        f.objects[index].1[0] ^= 1;
        assert!(f.verify().is_err());
        let mut f = ProofFixture::new();
        f.objects[index].0 = match f.objects[index].0 {
            ObjectKind::Tree => ObjectKind::Blob,
            ObjectKind::Blob => ObjectKind::Tree,
        };
        assert!(f.verify().is_err());
    }
    let mut f = ProofFixture::new();
    f.objects.push(f.objects[0].clone());
    assert_eq!(f.verify().err(), Some(Error::DuplicateObject));
    let mut f = ProofFixture::new();
    f.objects.push((ObjectKind::Blob, b"unreferenced".to_vec()));
    assert_eq!(f.verify().err(), Some(Error::ExtraObject));
    let f = ProofFixture::new();
    assert_eq!(
        verify(
            RegistryId::new("fixture-registry").unwrap(),
            &"f".repeat(40),
            &f.commit,
            &[]
        )
        .err(),
        Some(Error::Verification(cargo_source::Error::ObjectMismatch))
    );
}

#[test]
fn shared_blobs_and_subtrees_expand_without_dropping_paths() {
    let f = ProofFixture::with_files(&[
        ("Cargo.lock", LOCK),
        ("Cargo.toml", MANIFEST),
        ("src/main.rs", MAIN),
        ("one/file", b"same"),
        ("two/file", b"same"),
    ]);
    let source = f.verify().unwrap();
    let first = source
        .files()
        .iter()
        .find(|f| f.path() == "one/file")
        .unwrap();
    let second = source
        .files()
        .iter()
        .find(|f| f.path() == "two/file")
        .unwrap();
    assert_eq!(first.object_id(), second.object_id());
    assert_eq!(source.files().len(), 5);
}

#[test]
fn rejects_additional_targets_context_and_ambiguous_paths_at_any_depth() {
    for path in [
        "build.rs",
        "src/lib.rs",
        "src/bin/other.rs",
        "tests/check.rs",
        "examples/sample.rs",
        "benches/bench.rs",
        "nested/Cargo.toml",
        "nested/.cargo/config.toml",
        "nested/.git/config",
        "nested/.gitattributes",
        "rust-toolchain.toml",
        "Src/other.rs",
        "src/../evil.rs",
        "src/back\\slash",
        "src/name.",
        "src/é",
    ] {
        let f = ProofFixture::with_files(&[
            ("Cargo.lock", LOCK),
            ("Cargo.toml", MANIFEST),
            ("src/main.rs", MAIN),
            (path, b"content"),
        ]);
        assert!(f.verify().is_err(), "unexpectedly accepted {path}");
    }
    for main in [b"".as_slice(), b" \n", &[255]] {
        let f = ProofFixture::with_files(&[
            ("Cargo.lock", LOCK),
            ("Cargo.toml", MANIFEST),
            ("src/main.rs", main),
        ]);
        assert_eq!(f.verify().err(), Some(Error::UnsupportedTarget));
    }
    for absent in ["Cargo.lock", "Cargo.toml", "src/main.rs"] {
        let files: Vec<_> = [
            ("Cargo.lock", LOCK),
            ("Cargo.toml", MANIFEST),
            ("src/main.rs", MAIN),
        ]
        .into_iter()
        .filter(|(path, _)| *path != absent)
        .collect();
        assert!(ProofFixture::with_files(&files).verify().is_err());
    }
}

#[test]
fn rejects_nonregular_executable_modes_empty_trees_and_tampered_cargo() {
    for mode in ["100755", "120000", "160000"] {
        let mut f = ProofFixture::new();
        let root = f.objects.last_mut().unwrap();
        let offset = root.1.windows(6).position(|w| w == b"100644").unwrap();
        root.1[offset..offset + 6].copy_from_slice(mode.as_bytes());
        f.commit = commit(id(ObjectKind::Tree, &root.1));
        assert!(f.verify().is_err());
    }
    let mut f = ProofFixture::new();
    let empty = add(&mut f.objects, ObjectKind::Tree, vec![]);
    let mut root = f.objects[f.objects.len() - 2].1.clone();
    root.extend(entry("40000", "zz-empty", empty));
    f.objects
        .retain(|(kind, bytes)| *kind != ObjectKind::Tree || *bytes != f_root());
    f.commit = commit(add(&mut f.objects, ObjectKind::Tree, root));
    assert_eq!(f.verify().err(), Some(Error::EmptyTree));
    let bad = ProofFixture::with_files(&[
        ("Cargo.lock", LOCK),
        ("Cargo.toml", b"[workspace]\n"),
        ("src/main.rs", MAIN),
    ]);
    assert!(bad.verify().is_err());
}

fn f_root() -> Vec<u8> {
    ProofFixture::new().objects.last().unwrap().1.clone()
}

#[test]
fn byte_file_object_path_and_depth_bounds_stop_expansion() {
    let large = vec![b'x'; MAX_FILE_BYTES];
    assert!(
        ProofFixture::with_files(&[
            ("Cargo.lock", LOCK),
            ("Cargo.toml", MANIFEST),
            ("src/main.rs", MAIN),
            ("asset", &large)
        ])
        .verify()
        .is_ok()
    );
    let too_large = vec![b'x'; MAX_FILE_BYTES + 1];
    assert_eq!(
        ProofFixture::with_files(&[
            ("Cargo.lock", LOCK),
            ("Cargo.toml", MANIFEST),
            ("src/main.rs", MAIN),
            ("asset", &too_large)
        ])
        .verify()
        .err(),
        Some(Error::Capacity)
    );
    let names: Vec<_> = (0..MAX_FILES).map(|n| format!("file{n:03}")).collect();
    let mut files = vec![
        ("Cargo.lock", LOCK),
        ("Cargo.toml", MANIFEST),
        ("src/main.rs", MAIN),
    ];
    for name in &names[..MAX_FILES - 3] {
        files.push((name, b"shared"));
    }
    assert!(ProofFixture::with_files(&files).verify().is_ok());
    files.push((&names[MAX_FILES - 3], b"shared"));
    assert_eq!(
        ProofFixture::with_files(&files).verify().err(),
        Some(Error::Capacity)
    );
    let mut f = ProofFixture::new();
    f.objects
        .resize(MAX_OBJECTS + 1, (ObjectKind::Blob, vec![]));
    assert_eq!(f.verify().err(), Some(Error::Capacity));
    for path in [
        format!("{}file", "d/".repeat(MAX_DEPTH)),
        format!("{}file", format!("{}/", "a".repeat(128)).repeat(8)),
    ] {
        assert_eq!(
            ProofFixture::with_files(&[
                ("Cargo.lock", LOCK),
                ("Cargo.toml", MANIFEST),
                ("src/main.rs", MAIN),
                (&path, b"bounded")
            ])
            .verify()
            .err(),
            Some(Error::Capacity)
        );
    }
    // Repeated references amplify a small supplied graph into too many owned bytes.
    let mut files = vec![
        ("Cargo.lock", LOCK),
        ("Cargo.toml", MANIFEST),
        ("src/main.rs", MAIN),
    ];
    for name in &names[..8] {
        files.push((name, &large));
    }
    assert_eq!(
        ProofFixture::with_files(&files).verify().err(),
        Some(Error::Capacity)
    );
}

#[test]
fn input_byte_and_expanded_entry_limits_apply_before_snapshot_publication() {
    let mut f = ProofFixture::new();
    f.commit.resize(cargo_source::MAX_COMMIT_BYTES + 1, b'x');
    assert_eq!(f.verify().err(), Some(Error::Capacity));
    let mut f = ProofFixture::new();
    f.objects
        .push((ObjectKind::Tree, vec![0; cargo_source::MAX_TREE_BYTES + 1]));
    assert_eq!(f.verify().err(), Some(Error::Capacity));
    let mut f = ProofFixture::new();
    for byte in 0..8 {
        f.objects
            .push((ObjectKind::Blob, vec![byte; MAX_FILE_BYTES]));
    }
    assert_eq!(f.verify().err(), Some(Error::Capacity));
    // Many references to one shared chain stay below unique-object/file caps,
    // but total traversal work must still stop at the expanded-entry cap.
    let names: Vec<_> = (0..64)
        .map(|n| format!("tree{n:03}/one/two/three/file"))
        .collect();
    let mut files = vec![
        ("Cargo.lock", LOCK),
        ("Cargo.toml", MANIFEST),
        ("src/main.rs", MAIN),
    ];
    for name in &names {
        files.push((name, b"shared"));
    }
    let f = ProofFixture::with_files(&files);
    assert!(f.objects.len() < MAX_OBJECTS);
    assert_eq!(f.verify().err(), Some(Error::Capacity));
}

fn git(path: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .current_dir(path)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn actual_git_commit_supplies_complete_owned_snapshot_without_checkout_writes() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();
    git(base, &["init", "--initial-branch=main"]);
    fs::create_dir(base.join("src")).unwrap();
    for (path, bytes) in [
        ("Cargo.toml", MANIFEST),
        ("Cargo.lock", LOCK),
        ("src/main.rs", MAIN),
        ("src/helper.rs", b"pub fn helper() {}\n"),
    ] {
        fs::write(base.join(path), bytes).unwrap();
    }
    git(base, &["add", "--all"]);
    git(base, &["commit", "-m", "Public snapshot fixture"]);
    let selected = String::from_utf8(git(base, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    let commit = git(base, &["cat-file", "commit", &selected]);
    let listing = String::from_utf8(git(
        base,
        &[
            "cat-file",
            "--batch-all-objects",
            "--batch-check=%(objectname) %(objecttype)",
        ],
    ))
    .unwrap();
    let objects: Vec<_> = listing
        .lines()
        .filter_map(|line| {
            let (id, kind) = line.split_once(' ').unwrap();
            let object_kind = match kind {
                "tree" => ObjectKind::Tree,
                "blob" => ObjectKind::Blob,
                "commit" => return None,
                _ => panic!("fixture type"),
            };
            Some((object_kind, git(base, &["cat-file", kind, id])))
        })
        .collect();
    let snapshot = || {
        verify(
            RegistryId::new("fixture-registry").unwrap(),
            &selected,
            &commit,
            &objects
                .iter()
                .map(|(kind, bytes)| Object { kind: *kind, bytes })
                .collect::<Vec<_>>(),
        )
        .unwrap()
    };
    let paths = [
        ".git/index",
        ".git/config",
        "Cargo.toml",
        "Cargo.lock",
        "src/main.rs",
        "src/helper.rs",
    ];
    let observations = || {
        paths
            .iter()
            .map(|p| {
                let p = base.join(p);
                (
                    fs::read(&p).unwrap(),
                    fs::metadata(p).unwrap().modified().unwrap(),
                )
            })
            .collect::<Vec<_>>()
    };
    let before = observations();
    let source = snapshot();
    assert_eq!(before, observations());
    assert!(!base.join("target").exists());
    assert_eq!(source.commit(), selected);
    assert_eq!(source.files().len(), 4);
    for file in source.files() {
        assert_eq!(file.bytes(), fs::read(base.join(file.path())).unwrap());
    }
    fs::write(
        base.join("src/main.rs"),
        "changed checkout, outside the snapshot\n",
    )
    .unwrap();
    assert!(snapshot() == source);
    drop(temp);
    assert!(snapshot() == source);
}
