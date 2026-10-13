//! Trusted, fixed Linux fixture only. This is not a production job runner,
//! an untrusted-code sandbox, or proof of operating-system network isolation.
#![cfg(target_os = "linux")]

use devoption_registry::{RegistryId, Sha256, cargo_inputs, seeded::SeededCargoProvider};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

const MANIFEST: &[u8] = include_bytes!("fixtures/offline-app/Cargo.toml");
const LOCK: &[u8] = include_bytes!("fixtures/offline-app/Cargo.lock");
const SOURCE: &[u8] = include_bytes!("fixtures/offline-app/src/main.rs");
const ARCHIVE: &[u8] = include_bytes!("fixtures/registry/itoa-1.0.18.crate");
const INDEX: &[u8] = include_bytes!("fixtures/registry/itoa.index");

#[test]
#[ignore = "explicit Linux CI step compiles and runs the trusted offline fixture"]
fn trusted_fixture_builds_only_from_the_verified_seed() {
    let request = cargo_inputs::validate(
        RegistryId::new("fixture-registry").unwrap(),
        MANIFEST,
        LOCK,
        cargo_inputs::InputHashes {
            manifest: Sha256::digest(MANIFEST),
            lockfile: Sha256::digest(LOCK),
        },
    )
    .unwrap();
    let provider = SeededCargoProvider::new(request.clone(), ARCHIVE).unwrap();
    let bundle = provider.bundle(&request).unwrap();
    assert!(Sha256::digest(bundle.archive()) == bundle.materialization().bundle_sha256());

    // Cargo sets this to the installed Cargo used to build this test. Use its
    // sibling compiler directly; never discover/download another toolchain.
    let cargo = Path::new(env!("CARGO"));
    let rustc = cargo.with_file_name("rustc");
    assert!(cargo.is_absolute() && cargo.is_file() && rustc.is_file());

    for seed_state in ["valid", "missing", "corrupt"] {
        // Outside the checkout/workspace, with fresh homes and output for EVERY
        // case, so the successful build cannot warm a later refusal case.
        let temporary = tempfile::Builder::new()
            .prefix("devoption-offline-")
            .tempdir_in("/tmp")
            .unwrap();
        let root = temporary.path();
        for directory in [
            "app/src",
            "registry/index/it/oa",
            "cargo-home",
            "home",
            "output",
            "tmp",
        ] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        let sources = [
            ("Cargo.toml", MANIFEST),
            ("Cargo.lock", LOCK),
            ("src/main.rs", SOURCE),
        ];
        for (name, bytes) in sources {
            let path = root.join("app").join(name);
            fs::write(&path, bytes).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o444)).unwrap();
        }
        // Index/configuration are fixed trusted harness inputs, not part of the
        // provider's single-archive bundle and not production approval policy.
        fs::write(root.join("registry/index/it/oa/itoa"), INDEX).unwrap();
        let registry = root.join("registry");
        // The generated /tmp path contains no TOML quoting/control characters.
        let registry = registry.to_str().unwrap();
        assert!(
            registry
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
        );
        fs::write(root.join("cargo-home/config.toml"), format!(
            "[source.crates-io]\nreplace-with = 'fixture-seed'\n[source.fixture-seed]\nlocal-registry = '{registry}'\n"
        )).unwrap();
        let archive_path = root.join("registry/itoa-1.0.18.crate");
        match seed_state {
            "valid" => fs::write(&archive_path, bundle.archive()).unwrap(),
            "missing" => {}
            "corrupt" => {
                // Simulate corruption AFTER the provider handoff; Cargo must
                // refuse the on-disk bytes, too, without a warm-cache fallback.
                let mut bytes = bundle.archive().to_vec();
                bytes[100] ^= 1;
                fs::write(&archive_path, bytes).unwrap();
            }
            _ => unreachable!(),
        }
        let result = Command::new("/usr/bin/timeout")
            .args(["--kill-after=5s", "60s"])
            .arg(cargo)
            .args([
                "build",
                "--release",
                "--locked",
                "--offline",
                "--package",
                "offline-fixture",
                "--bin",
                "offline-fixture",
            ])
            .current_dir(root.join("app"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", root.join("home"))
            .env("CARGO_HOME", root.join("cargo-home"))
            .env("CARGO_TARGET_DIR", root.join("output"))
            .env("TMPDIR", root.join("tmp"))
            .env("RUSTC", &rustc)
            .env("RUSTC_WRAPPER", "")
            .env("RUSTC_WORKSPACE_WRAPPER", "")
            .env("CARGO_NET_OFFLINE", "true")
            .env("CARGO_INCREMENTAL", "0")
            .env("CARGO_BUILD_JOBS", "1")
            .output()
            .unwrap();
        let artifact = root.join("output/release/offline-fixture");
        if seed_state == "valid" {
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let length = fs::metadata(&artifact).unwrap().len();
            assert!((1..4 * 1024 * 1024).contains(&length));
            let hash = Sha256::digest(&fs::read(&artifact).unwrap());
            let run = Command::new("/usr/bin/timeout")
                .args(["--kill-after=1s", "5s"])
                .arg(&artifact)
                .env_clear()
                .current_dir(root)
                .output()
                .unwrap();
            assert!(run.status.success());
            assert_eq!(run.stdout, b"verified seed: 42\n");
            assert!(run.stderr.is_empty());
            assert!(hash == Sha256::digest(&fs::read(&artifact).unwrap()));
            println!(
                "fixture artifact sha256={} bytes={length}",
                hash.bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            );
        } else {
            assert_eq!(
                result.status.code(),
                Some(101),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(!artifact.exists());
            println!("{seed_state} seed refused by offline Cargo; no artifact");
        }
        for (name, bytes) in sources {
            assert_eq!(fs::read(root.join("app").join(name)).unwrap(), bytes);
        }
        temporary.close().unwrap();
    }
}
