use super::*;

const COMMIT: &[u8] = include_bytes!("fixtures/root.commit");
const TREE: &[u8] = include_bytes!("fixtures/root.tree");
const MANIFEST: &[u8] =
    include_bytes!("../../../devoption-registry/src/cargo_inputs/fixtures/standalone.toml");
const LOCK: &[u8] =
    include_bytes!("../../../devoption-registry/src/cargo_inputs/fixtures/standalone.lock");
const SELECTED: &str = "d968ac8a8213026f1e229bc64c687f51a4f1c09c";

fn object(kind: &str, bytes: &[u8]) -> [u8; 20] {
    checked_hash(&[format!("{kind} {}\0", bytes.len()).as_bytes(), bytes]).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn check(
    selected: &str,
    commit: &[u8],
    tree: &[u8],
    manifest: &[u8],
    lock: &[u8],
) -> Result<RootCargoSource, Error> {
    verify_root(
        RegistryId::new("fixture-registry").unwrap(),
        selected,
        RootProof {
            commit,
            tree,
            manifest,
            lockfile: lock,
        },
    )
}

fn rebind_tree(tree: &[u8], manifest: &[u8], lock: &[u8]) -> Result<RootCargoSource, Error> {
    let commit = format!(
        "tree {}\nauthor Fixture <fixture@example.invalid> 1 +0000\ncommitter Fixture <fixture@example.invalid> 1 +0000\n\nFixture\n",
        hex(&object("tree", tree))
    );
    check(
        &hex(&object("commit", commit.as_bytes())),
        commit.as_bytes(),
        tree,
        manifest,
        lock,
    )
}

fn entry(mode: &[u8], name: &[u8], id: [u8; 20]) -> Vec<u8> {
    [mode, b" ", name, b"\0", &id].concat()
}

fn tree_with(manifest: &[u8], lock: &[u8]) -> Vec<u8> {
    [
        entry(b"100644", b"Cargo.lock", object("blob", lock)),
        entry(b"100644", b"Cargo.toml", object("blob", manifest)),
    ]
    .concat()
}

#[test]
fn independent_golden_objects_bind_owned_cargo_metadata() {
    let source = check(SELECTED, COMMIT, TREE, MANIFEST, LOCK).unwrap();
    assert_eq!(hex(&object("commit", COMMIT)), SELECTED);
    assert_eq!(
        hex(&object("tree", TREE)),
        "b49be84ec97d8778a679d9c0ee1fc7b71f2efa13"
    );
    assert_eq!(
        hex(&object("blob", b"test content\n")),
        "d670460b4b4aece5915caf5c68d12f560a9fe3e4"
    );
    assert_eq!(source.commit(), SELECTED);
    assert!(
        source.commit_sha256()
            == Sha256::from_hex("0fd131ec5f3dfd9969444b227a84895a5924374acf3c554ca8aa8158c14583bd")
                .unwrap()
    );
    assert!(source.tree_sha256() == Sha256::digest(TREE));
    assert!(source.request().manifest_sha256() == Sha256::digest(MANIFEST));
    assert!(source.request().lockfile_sha256() == Sha256::digest(LOCK));
    assert_eq!(source.request().packages().len(), 2);
    let mut bytes = MANIFEST.to_vec();
    let owned = check(SELECTED, COMMIT, TREE, &bytes, LOCK).unwrap();
    bytes.fill(0);
    assert!(owned == source);
}

#[test]
fn every_link_is_verified_against_its_parent_and_object_type() {
    for index in 0..4 {
        let mut inputs = [
            COMMIT.to_vec(),
            TREE.to_vec(),
            MANIFEST.to_vec(),
            LOCK.to_vec(),
        ];
        inputs[index][0] ^= 1;
        assert_eq!(
            check(SELECTED, &inputs[0], &inputs[1], &inputs[2], &inputs[3]).err(),
            Some(Error::ObjectMismatch)
        );
    }
    assert_eq!(
        check(&hex(&object("blob", COMMIT)), COMMIT, TREE, MANIFEST, LOCK).err(),
        Some(Error::ObjectMismatch)
    );
    let mut tree = tree_with(MANIFEST, LOCK);
    tree[18..38].copy_from_slice(&checked_hash(&[LOCK]).unwrap());
    assert_eq!(
        rebind_tree(&tree, MANIFEST, LOCK).err(),
        Some(Error::ObjectMismatch)
    );
    let framed = [format!("commit {}\0", COMMIT.len()).as_bytes(), COMMIT].concat();
    assert_eq!(
        check(SELECTED, &framed, TREE, MANIFEST, LOCK).err(),
        Some(Error::ObjectMismatch)
    );
}

#[test]
fn self_selected_revisions_are_consistent_but_not_authenticated() {
    let changed = [MANIFEST, b"\n# Different source bytes\n"].concat();
    let tree = tree_with(&changed, LOCK);
    assert_eq!(
        check(SELECTED, COMMIT, &tree, &changed, LOCK).err(),
        Some(Error::ObjectMismatch)
    );
    let supplied = rebind_tree(&tree, &changed, LOCK).unwrap();
    let original = check(SELECTED, COMMIT, TREE, MANIFEST, LOCK).unwrap();
    assert_ne!(supplied.commit(), original.commit());
    assert!(supplied.request() != original.request());
    assert!(supplied.request().packages() == original.request().packages());
}

#[test]
fn empty_oversized_and_noncanonical_selected_ids_are_rejected() {
    for selected in [
        "",
        "main",
        "HEAD",
        &"0".repeat(40),
        &SELECTED.to_uppercase(),
        &"a".repeat(64),
    ] {
        assert_eq!(
            check(selected, COMMIT, TREE, MANIFEST, LOCK).err(),
            Some(Error::InvalidCommit)
        );
    }
    for (index, limit) in [
        MAX_COMMIT_BYTES,
        MAX_TREE_BYTES,
        cargo_inputs::MAX_MANIFEST_BYTES,
        cargo_inputs::MAX_LOCKFILE_BYTES,
    ]
    .into_iter()
    .enumerate()
    {
        let mut inputs = [
            COMMIT.to_vec(),
            TREE.to_vec(),
            MANIFEST.to_vec(),
            LOCK.to_vec(),
        ];
        inputs[index].clear();
        assert_eq!(
            check(SELECTED, &inputs[0], &inputs[1], &inputs[2], &inputs[3]).err(),
            Some(Error::MissingInput)
        );
        inputs[index].resize(limit + 1, b'x');
        assert_eq!(
            check(SELECTED, &inputs[0], &inputs[1], &inputs[2], &inputs[3]).err(),
            Some(Error::Capacity)
        );
    }
}

#[test]
fn commit_headers_reject_ambiguous_and_unsupported_shapes() {
    let original = std::str::from_utf8(COMMIT).unwrap();
    let tree_line = original.lines().next().unwrap();
    for changed in [
        original.replace("tree ", "object "),
        original.replacen('\n', &format!("\n{tree_line}\n"), 1),
        original.replace("\n\n", "\ngpgsig signature\n continuation\n\n"),
        original.replace("\n\n", "\nencoding ISO-8859-1\n\n"),
        original.replace("\ncommitter ", "\nauthor "),
        original.replace("\n\n", "\n"),
        original.replace("Fixture <", "private\ridentity <"),
        original.replace("Fixture <", "\0 <"),
        original.replace("author Fixture <fixture@example.invalid> 1 +0000\n", ""),
    ] {
        assert_eq!(
            check(
                &hex(&object("commit", changed.as_bytes())),
                changed.as_bytes(),
                TREE,
                MANIFEST,
                LOCK
            )
            .err(),
            Some(Error::UnsupportedCommit)
        );
    }
    let parent = format!("parent {}\n", "1".repeat(40));
    for count in [2, 3] {
        let parents: String = (1..=count)
            .map(|i| format!("parent {}\n", i.to_string().repeat(40)))
            .collect();
        let changed = original.replacen('\n', &format!("\n{parents}"), 1);
        let result = check(
            &hex(&object("commit", changed.as_bytes())),
            changed.as_bytes(),
            TREE,
            MANIFEST,
            LOCK,
        );
        if count == 2 {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.err(), Some(Error::UnsupportedCommit));
        }
    }
    let valid = original.replacen('\n', &format!("\n{parent}"), 1);
    assert!(
        check(
            &hex(&object("commit", valid.as_bytes())),
            valid.as_bytes(),
            TREE,
            MANIFEST,
            LOCK
        )
        .is_ok()
    );
    let duplicate = original.replacen('\n', &format!("\n{parent}{parent}"), 1);
    assert_eq!(
        check(
            &hex(&object("commit", duplicate.as_bytes())),
            duplicate.as_bytes(),
            TREE,
            MANIFEST,
            LOCK
        )
        .err(),
        Some(Error::UnsupportedCommit)
    );
}

#[test]
fn root_entries_reject_path_type_case_and_order_ambiguity() {
    let lock = entry(b"100644", b"Cargo.lock", object("blob", LOCK));
    for name in [
        b"Cargo.toml/child".as_slice(),
        b"../Cargo.toml",
        b"Cargo.toml.",
        b"Cargo.toml ",
        b"Cargo.toml:stream",
        b"Cargo~1.toml",
        b".",
        b"..",
        b"",
        b"\xff",
        b"cargo.toml",
    ] {
        let tree = [
            lock.clone(),
            entry(b"100644", name, object("blob", MANIFEST)),
        ]
        .concat();
        assert_eq!(
            rebind_tree(&tree, MANIFEST, LOCK).err(),
            Some(Error::InvalidTree)
        );
    }
    for mode in [
        b"100755".as_slice(),
        b"120000",
        b"160000",
        b"40000",
        b"040000",
        b"0100644",
        b"100664",
    ] {
        let tree = [
            lock.clone(),
            entry(mode, b"Cargo.toml", object("blob", MANIFEST)),
        ]
        .concat();
        assert_eq!(
            rebind_tree(&tree, MANIFEST, LOCK).err(),
            Some(Error::InvalidTree)
        );
    }
    for extra in [
        entry(b"100644", b"Cargo.toml", [1; 20]),
        entry(b"100644", b"cargo.toml", [1; 20]),
        entry(b"100644", b"AAAA", [1; 20]),
        entry(b"100644", b"z", [0; 20]),
    ] {
        let tree = [tree_with(MANIFEST, LOCK), extra].concat();
        assert_eq!(
            rebind_tree(&tree, MANIFEST, LOCK).err(),
            Some(Error::InvalidTree)
        );
    }
}

#[test]
fn root_configuration_is_rejected_without_reading_its_content() {
    for name in [
        b".cargo".as_slice(),
        b".CARGO",
        b".git",
        b".gitmodules",
        b".gitattributes",
        b"rust-toolchain",
        b"rust-toolchain.toml",
    ] {
        let extra = entry(b"40000", name, [1; 20]);
        let base = tree_with(MANIFEST, LOCK);
        let tree = if name[0] == b'.' {
            [extra, base].concat()
        } else {
            [base, extra].concat()
        };
        assert_eq!(
            rebind_tree(&tree, MANIFEST, LOCK).err(),
            Some(Error::UnsupportedContext)
        );
    }
}

#[test]
fn malformed_and_truncated_trees_never_panic_or_invent_missing_entries() {
    let tree = tree_with(MANIFEST, LOCK);
    for end in 0..tree.len() {
        assert!(rebind_tree(&tree[..end], MANIFEST, LOCK).is_err());
    }
    for suffix in [
        b"x".as_slice(),
        b"\0",
        b"100644 missing\0",
        b"100644 valid\0short",
    ] {
        assert_eq!(
            rebind_tree(&[tree.as_slice(), suffix].concat(), MANIFEST, LOCK).err(),
            Some(Error::InvalidTree)
        );
    }
    // Git orders a tree name as if it ends with '/', not NUL.
    let good = [
        tree.clone(),
        entry(b"100644", b"foo.bar", [1; 20]),
        entry(b"40000", b"foo", [2; 20]),
    ]
    .concat();
    assert!(rebind_tree(&good, MANIFEST, LOCK).is_ok());
    let bad = [
        tree,
        entry(b"40000", b"foo", [2; 20]),
        entry(b"100644", b"foo.bar", [1; 20]),
    ]
    .concat();
    assert_eq!(
        rebind_tree(&bad, MANIFEST, LOCK).err(),
        Some(Error::InvalidTree)
    );
}

#[test]
fn bounded_root_entry_count_and_commit_size_accept_the_exact_limit() {
    for length in [MAX_NAME_BYTES, MAX_NAME_BYTES + 1] {
        let tree = [
            tree_with(MANIFEST, LOCK),
            entry(b"100644", &vec![b'z'; length], [1; 20]),
        ]
        .concat();
        let result = rebind_tree(&tree, MANIFEST, LOCK);
        if length == MAX_NAME_BYTES {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.err(), Some(Error::InvalidTree));
        }
    }
    let mut tree = tree_with(MANIFEST, LOCK);
    for i in 2..MAX_ROOT_ENTRIES {
        tree.extend(entry(b"100644", format!("p{i:04}").as_bytes(), [1; 20]));
    }
    assert!(rebind_tree(&tree, MANIFEST, LOCK).is_ok());
    tree.extend(entry(b"100644", b"overflow", [1; 20]));
    assert_eq!(
        rebind_tree(&tree, MANIFEST, LOCK).err(),
        Some(Error::Capacity)
    );
    let mut commit = COMMIT.to_vec();
    commit.resize(MAX_COMMIT_BYTES, b'x');
    assert!(
        check(
            &hex(&object("commit", &commit)),
            &commit,
            TREE,
            MANIFEST,
            LOCK
        )
        .is_ok()
    );
}

#[test]
fn linked_objects_do_not_bypass_cargo_semantics() {
    let unsupported = [MANIFEST, b"\n[workspace]\nmembers=[]\n"].concat();
    let tree = tree_with(&unsupported, LOCK);
    assert_eq!(
        rebind_tree(&tree, &unsupported, LOCK).err(),
        Some(Error::CargoInputs(cargo_inputs::Error::Unsupported))
    );
}

#[test]
fn collision_detection_is_an_error_and_diagnostics_omit_supplied_data() {
    // Public synthetic attack vector, tested at the common hash helper. It is
    // not a Git object; adding a Git header changes its collision construction.
    let vector = include_bytes!("fixtures/sha-mbles-1.bin");
    assert_eq!(checked_hash(&[vector]), Err(Error::Collision));
    assert_eq!(
        checked_hash(&[&vector[..63], &vector[63..]]),
        Err(Error::Collision)
    );
    let err = check("private-identity", COMMIT, TREE, MANIFEST, LOCK)
        .err()
        .unwrap();
    assert!(!err.to_string().contains("private-identity"));
    assert!(!format!("{err:?}").contains("private-identity"));
}
