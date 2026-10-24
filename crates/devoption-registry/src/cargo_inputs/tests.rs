use super::*;

const MANIFEST: &[u8] = include_bytes!("fixtures/standalone.toml");
const LOCK: &[u8] = include_bytes!("fixtures/standalone.lock");

fn hashes(manifest: &[u8], lock: &[u8]) -> InputHashes {
    InputHashes {
        manifest: Sha256::digest(manifest),
        lockfile: Sha256::digest(lock),
    }
}

fn check(manifest: &[u8], lock: &[u8]) -> Result<CargoDependencyRequest, Error> {
    validate(
        RegistryId::new("fixture-registry").unwrap(),
        manifest,
        lock,
        hashes(manifest, lock),
    )
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

fn extra_package(name: &str, version: &str, dependencies: &str) -> String {
    format!(
        "\n[[package]]\nname = {name:?}\nversion = {version:?}\nsource = {CRATES_IO_SOURCE:?}\nchecksum = {:?}\ndependencies = [{dependencies}]\n",
        "3".repeat(64)
    )
}

#[test]
fn supplied_bytes_form_an_owned_request_with_exact_hashes_and_transitive_packages() {
    let mut manifest = MANIFEST.to_vec();
    let request = check(&manifest, LOCK).unwrap();
    manifest.fill(0);
    assert_eq!(request.registry().as_str(), "fixture-registry");
    assert!(request.manifest_sha256() == Sha256::digest(MANIFEST));
    assert!(request.lockfile_sha256() == Sha256::digest(LOCK));
    assert_eq!(
        request
            .packages()
            .iter()
            .map(|p| (p.name.as_str(), p.version.as_str()))
            .collect::<Vec<_>>(),
        [("bytes", "1.0.0"), ("http", "1.2.3")]
    );
    assert!(
        request
            .packages()
            .iter()
            .all(|p| p.origin == CargoOrigin::CratesIo)
    );
    assert!(request.packages()[0].archive_sha256 == Sha256::from_hex(&"2".repeat(64)).unwrap());
}

#[test]
fn input_hashing_matches_known_sha256_vectors() {
    for (bytes, expected) in [
        (
            b"".as_slice(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc".as_slice(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
    ] {
        assert!(Sha256::digest(bytes) == Sha256::from_hex(expected).unwrap());
    }
}

#[test]
fn comments_and_whitespace_change_identity_and_stale_expectations_are_rejected() {
    let original = check(MANIFEST, LOCK).unwrap();
    for (manifest, lock) in [
        (
            [MANIFEST, b"\n# changed manifest\n"].concat(),
            LOCK.to_vec(),
        ),
        (
            MANIFEST.to_vec(),
            [LOCK, b"\n# changed lockfile\n"].concat(),
        ),
    ] {
        assert_eq!(
            validate(
                RegistryId::new("fixture-registry").unwrap(),
                &manifest,
                &lock,
                hashes(MANIFEST, LOCK)
            )
            .err(),
            Some(Error::HashMismatch)
        );
        let changed = check(&manifest, &lock).unwrap();
        assert!(changed != original);
        assert!(changed.packages() == original.packages());
    }
}

#[test]
fn missing_oversized_invalid_utf8_duplicate_keys_and_deep_toml_fail_closed() {
    assert_eq!(check(b"", LOCK).err(), Some(Error::MissingInput));
    assert_eq!(check(MANIFEST, b"").err(), Some(Error::MissingInput));
    for input in [b"\xff".as_slice(), b"[package", b"name=1\nname=2"] {
        assert_eq!(check(input, LOCK).err(), Some(Error::Syntax));
        assert_eq!(check(MANIFEST, input).err(), Some(Error::Syntax));
    }
    let nested = format!("x={}0{}", "[".repeat(300), "]".repeat(300));
    assert_eq!(check(nested.as_bytes(), LOCK).err(), Some(Error::Syntax));
    for (input, limit, is_manifest) in [
        (MANIFEST, MAX_MANIFEST_BYTES, true),
        (LOCK, MAX_LOCKFILE_BYTES, false),
    ] {
        let mut padded = input.to_vec();
        padded.push(b'#');
        padded.resize(limit, b'x');
        let result = if is_manifest {
            check(&padded, LOCK)
        } else {
            check(MANIFEST, &padded)
        };
        assert!(result.is_ok());
        padded.push(b'x');
        let result = if is_manifest {
            check(&padded, LOCK)
        } else {
            check(MANIFEST, &padded)
        };
        assert_eq!(result.err(), Some(Error::Capacity));
    }
}

#[test]
fn unsupported_manifest_semantics_are_rejected_instead_of_ignored() {
    for addition in [
        "\n[workspace]\nmembers=[]\n",
        "\n[features]\ndefault=[]\n",
        "\n[dev-dependencies]\nhelper='=1.0.0'\n",
        "\n[build-dependencies]\nhelper='=1.0.0'\n",
        "\n[target.'cfg(unix)'.dependencies]\nhelper='=1.0.0'\n",
        "\n[patch.crates-io]\nhelper={path='../helper'}\n",
        "\n[profile.release]\nopt-level=1\n",
        "\n[[bin]]\nname='alternate'\n",
    ] {
        assert_eq!(
            check(format!("{}{addition}", text(MANIFEST)).as_bytes(), LOCK).err(),
            Some(Error::Unsupported)
        );
    }
    for dependency in [
        "'1.2.3'",
        "'^1.2.3'",
        "'*'",
        "{ version='=1.2.3' }",
        "{ path='../http' }",
        "{ git='https://example.invalid/source' }",
        "{ version='=1.2.3', registry='private' }",
        "{ workspace=true }",
        "{ version='=1.2.3', optional=true }",
        "{ package='renamed', version='=1.2.3' }",
    ] {
        let manifest = text(MANIFEST).replace("\"=1.2.3\"", dependency);
        assert!(check(manifest.as_bytes(), LOCK).is_err());
    }
    for replacement in ["build = true", "build = 'build.rs'", "", "build = []"] {
        let manifest = text(MANIFEST).replace("build = false", replacement);
        assert_eq!(
            check(manifest.as_bytes(), LOCK).err(),
            Some(Error::Unsupported)
        );
    }
}

#[test]
fn root_and_direct_dependency_pins_must_match_the_supplied_lockfile() {
    for lock in [
        text(LOCK).replace("ExampleService", "OtherService"),
        text(LOCK).replace("0.1.0", "0.2.0"),
    ] {
        assert_eq!(
            check(MANIFEST, lock.as_bytes()).err(),
            Some(Error::RootMismatch)
        );
    }
    let mismatch = text(MANIFEST).replace("=1.2.3", "=1.2.4");
    assert_eq!(
        check(mismatch.as_bytes(), LOCK).err(),
        Some(Error::DependencyMismatch)
    );
    let missing_direct = text(LOCK).replace("dependencies = [\"http\"]", "dependencies = []");
    assert_eq!(
        check(MANIFEST, missing_direct.as_bytes()).err(),
        Some(Error::DependencyMismatch)
    );
    let extra_direct = text(LOCK).replace(
        "dependencies = [\"http\"]",
        "dependencies = [\"http\", \"bytes\"]",
    );
    assert_eq!(
        check(MANIFEST, extra_direct.as_bytes()).err(),
        Some(Error::DependencyMismatch)
    );
}

#[test]
fn exact_pin_with_defaults_disabled_is_bound_to_its_exact_manifest() {
    let changed = text(MANIFEST).replace(
        "http = \"=1.2.3\"",
        "http = { version = \"=1.2.3\", default-features = false }",
    );
    let request = check(changed.as_bytes(), LOCK).unwrap();
    let ordinary = check(MANIFEST, LOCK).unwrap();
    assert!(request.packages() == ordinary.packages());
    assert!(request.manifest_sha256() != ordinary.manifest_sha256());
    assert_eq!(
        validate(
            RegistryId::new("fixture-registry").unwrap(),
            changed.as_bytes(),
            LOCK,
            hashes(MANIFEST, LOCK)
        )
        .err(),
        Some(Error::HashMismatch)
    );
    let mismatch = changed.replace("=1.2.3", "=1.2.4");
    assert_eq!(
        check(mismatch.as_bytes(), LOCK).err(),
        Some(Error::DependencyMismatch)
    );
}

#[test]
fn disabling_defaults_does_not_admit_arbitrary_dependency_semantics() {
    for table in [
        "{version='=1.2.3'}",
        "{version='=1.2.3',default-features=true}",
        "{version='=1.2.3',default-features='false'}",
        "{version='1.2.3',default-features=false}",
        "{version='=1.2.3',default-features=false,features=[]}",
        "{version='=1.2.3',default-features=false,path='../x'}",
        "{version='=1.2.3',default-features=false,git='https://example.invalid/x'}",
        "{version='=1.2.3',default-features=false,package='other'}",
        "{version='=1.2.3',default-features=false,optional=false}",
        "{version='=1.2.3',default-features=false,registry='other'}",
        "{default-features=false}",
    ] {
        let changed = text(MANIFEST).replace("\"=1.2.3\"", table);
        assert!(check(changed.as_bytes(), LOCK).is_err(), "admitted {table}");
    }
}

#[test]
fn unsupported_lockfile_sources_versions_and_fields_are_rejected() {
    for source in [
        "git+https://example.invalid/source#abcdef",
        "registry+https://private.invalid/index",
        "sparse+https://index.crates.io/",
        "",
    ] {
        let lock = text(LOCK).replace(CRATES_IO_SOURCE, source);
        assert_eq!(
            check(MANIFEST, lock.as_bytes()).err(),
            Some(Error::Unsupported)
        );
    }
    for version in ["1", "2", "3", "5", "4.0", "'4'"] {
        let lock = text(LOCK).replacen("version = 4", &format!("version = {version}"), 1);
        assert_eq!(
            check(MANIFEST, lock.as_bytes()).err(),
            Some(Error::Unsupported)
        );
    }
    assert_eq!(
        check(
            MANIFEST,
            format!("{}\n[metadata]\nx='ignored?'\n", text(LOCK)).as_bytes()
        )
        .err(),
        Some(Error::Unsupported)
    );
    let extra_root = format!(
        "{}\n[[package]]\nname='path-package'\nversion='1.0.0'\n",
        text(LOCK)
    );
    assert_eq!(
        check(MANIFEST, extra_root.as_bytes()).err(),
        Some(Error::Unsupported)
    );
}

#[test]
fn registry_checksums_are_required_and_well_formed_but_not_archive_verification() {
    let original = format!("checksum = \"{}\"", "1".repeat(64));
    for checksum in [
        "".into(),
        "checksum = 1".into(),
        "checksum = 'invalid'".into(),
        format!("checksum = {:?}", "A".repeat(64)),
    ] {
        let lock = text(LOCK).replace(&original, &checksum);
        assert_eq!(
            check(MANIFEST, lock.as_bytes()).err(),
            Some(Error::InvalidChecksum)
        );
    }
    let lock = text(LOCK).replace(&"1".repeat(64), &"9".repeat(64));
    assert_eq!(
        validate(
            RegistryId::new("fixture-registry").unwrap(),
            MANIFEST,
            lock.as_bytes(),
            hashes(MANIFEST, LOCK)
        )
        .err(),
        Some(Error::HashMismatch)
    );
    // New caller-selected expectations can describe different package bytes;
    // the adapter has no archives with which to prove or disprove the claim.
    let changed = check(MANIFEST, lock.as_bytes()).unwrap();
    assert!(changed != check(MANIFEST, LOCK).unwrap());
}

#[test]
fn missing_ambiguous_or_duplicate_references_never_guess_a_package() {
    let missing = text(LOCK).replace("bytes 1.0.0", "absent");
    assert_eq!(
        check(MANIFEST, missing.as_bytes()).err(),
        Some(Error::MissingDependency)
    );
    let qualified = format!(
        "{}{}",
        text(LOCK).replace("\"bytes 1.0.0\"", "\"bytes 1.0.0\", \"bytes 2.0.0\""),
        extra_package("bytes", "2.0.0", "")
    );
    assert_eq!(
        check(MANIFEST, qualified.as_bytes())
            .unwrap()
            .packages()
            .len(),
        3
    );
    let ambiguous = qualified.replace("\"bytes 1.0.0\"", "\"bytes\"");
    assert_eq!(
        check(MANIFEST, ambiguous.as_bytes()).err(),
        Some(Error::AmbiguousDependency)
    );
    let repeated = text(LOCK).replace("\"bytes 1.0.0\"", "\"bytes 1.0.0\", \"bytes\"");
    assert_eq!(
        check(MANIFEST, repeated.as_bytes()).err(),
        Some(Error::InvalidGraph)
    );
    let source_qualified =
        text(LOCK).replace("bytes 1.0.0", &format!("bytes 1.0.0 ({CRATES_IO_SOURCE})"));
    assert_eq!(
        check(MANIFEST, source_qualified.as_bytes()).err(),
        Some(Error::Unsupported)
    );
}

#[test]
fn duplicate_cyclic_and_disconnected_lock_graphs_are_rejected() {
    for (name, version) in [
        ("bytes", "1.0.0"),
        ("Bytes", "1.0.0"),
        ("bytes", "1.0.0+other"),
    ] {
        let lock = format!("{}{}", text(LOCK), extra_package(name, version, ""));
        assert_eq!(
            check(MANIFEST, lock.as_bytes()).err(),
            Some(Error::InvalidPackage)
        );
    }
    let cycle = format!("{}\ndependencies = ['http']\n", text(LOCK));
    assert_eq!(
        check(MANIFEST, cycle.as_bytes()).err(),
        Some(Error::InvalidGraph)
    );
    let disconnected = format!("{}{}", text(LOCK), extra_package("unused", "1.0.0", ""));
    assert_eq!(
        check(MANIFEST, disconnected.as_bytes()).err(),
        Some(Error::InvalidGraph)
    );
}

#[test]
fn dependency_free_packages_and_bounded_deep_graphs_are_supported() {
    let manifest = "[package]\nname='app'\nversion='1.0.0'\nedition='2021'\nbuild=false\n";
    let mut lock = "version=4\n[[package]]\nname='app'\nversion='1.0.0'\n".to_owned();
    assert!(
        check(manifest.as_bytes(), lock.as_bytes())
            .unwrap()
            .packages()
            .is_empty()
    );
    let manifest = format!("{manifest}\n[dependencies]\np0='=1.0.0'\n");
    lock.push_str("dependencies=['p0']\n");
    for i in 0..crate::MAX_PACKAGES {
        let edge = if i + 1 < crate::MAX_PACKAGES {
            format!("'p{}'", i + 1)
        } else {
            String::new()
        };
        lock.push_str(&extra_package(&format!("p{i}"), "1.0.0", &edge));
    }
    assert_eq!(
        check(manifest.as_bytes(), lock.as_bytes())
            .unwrap()
            .packages()
            .len(),
        crate::MAX_PACKAGES
    );
    lock.push_str(&extra_package("overflow", "1.0.0", ""));
    assert_eq!(
        check(manifest.as_bytes(), lock.as_bytes()).err(),
        Some(Error::Capacity)
    );
    let many_edges = text(LOCK).replace(
        "\"bytes 1.0.0\"",
        &vec!["'bytes'"; MAX_DEPENDENCY_EDGES + 1].join(","),
    );
    assert_eq!(
        check(MANIFEST, many_edges.as_bytes()).err(),
        Some(Error::Capacity)
    );
}

#[test]
fn errors_do_not_echo_untrusted_toml() {
    let private = "private-package-and-credential-like-text";
    let manifest = format!("[package]\nname = \"{private}\n");
    let error = check(manifest.as_bytes(), LOCK).err().unwrap();
    assert!(!error.to_string().contains(private));
    assert!(!format!("{error:?}").contains(private));
}

#[test]
fn inspection_preserves_adapter_results_without_selecting_a_registry() {
    let inputs = inspect(MANIFEST, LOCK).unwrap();
    let request = check(MANIFEST, LOCK).unwrap();
    assert_eq!(
        (
            inputs.package(),
            inputs.version(),
            inputs.dependency_count()
        ),
        ("ExampleService", "0.1.0", 2)
    );
    assert!(inputs.hashes().manifest == request.manifest_sha256());
    assert!(inputs.hashes().lockfile == request.lockfile_sha256());
    for (manifest, lock) in [
        (b"[workspace]".as_slice(), LOCK),
        (MANIFEST, b"invalid".as_slice()),
        (MANIFEST, b"".as_slice()),
    ] {
        assert_eq!(inspect(manifest, lock).err(), check(manifest, lock).err());
    }
}

#[test]
fn cdylib_requires_one_conventional_named_library() {
    let manifest = format!(
        "{}\n[lib]\nname = 'ExampleService'\ncrate-type = ['cdylib']\n",
        text(MANIFEST)
    );
    let inputs = inspect(manifest.as_bytes(), LOCK).unwrap();
    assert_eq!(
        inputs.target(),
        &SourceTarget::Cdylib {
            name: "ExampleService".into()
        }
    );
    assert_eq!(inputs.target().path(), "src/lib.rs");
    assert_eq!(
        inspect(MANIFEST, LOCK).unwrap().target(),
        &SourceTarget::Binary
    );
    for changed in [
        manifest.replace("name = 'ExampleService'", "name = 'different'"),
        manifest.replace("crate-type = ['cdylib']", "crate-type = ['cdylib', 'rlib']"),
        manifest.replace("crate-type = ['cdylib']", "crate-type = ['rlib']"),
        manifest.replace("crate-type = ['cdylib']", "crate-type = []"),
        manifest.replace("crate-type = ['cdylib']", "crate-type = 'cdylib'"),
        manifest.replace("crate-type = ['cdylib']", ""),
        format!("{manifest}path = 'other.rs'\n"),
        format!("{manifest}proc-macro = true\n"),
        format!("{manifest}[[bin]]\nname = 'extra'\n"),
    ] {
        assert!(inspect(changed.as_bytes(), LOCK).is_err());
    }
}

#[test]
fn exact_requirements_ignore_only_build_metadata_and_keep_full_locked_identity() {
    for (pin, locked, accepted) in [
        ("1.2.3", "1.2.3+wasi-0.2.4", true),
        ("1.2.3+declared", "1.2.3+locked", true),
        ("1.2.3-alpha.1", "1.2.3-alpha.1+locked", true),
        ("1.2.3", "1.2.4+locked", false),
        ("1.2.3", "1.2.3-alpha.1+locked", false),
        ("1.2.3-alpha.1", "1.2.3-alpha.2+locked", false),
        ("1.2.3-alpha.1", "1.2.3+locked", false),
    ] {
        let manifest = text(MANIFEST).replace("=1.2.3", &format!("={pin}"));
        let lock = text(LOCK).replace("1.2.3", locked);
        let result = check(manifest.as_bytes(), lock.as_bytes());
        if accepted {
            let request = result.unwrap();
            assert_eq!(
                request
                    .packages()
                    .iter()
                    .find(|p| p.name == "http")
                    .unwrap()
                    .version,
                locked
            );
            assert!(request != check(MANIFEST, LOCK).unwrap());
        } else {
            assert_eq!(result.err(), Some(Error::DependencyMismatch));
        }
    }
}
