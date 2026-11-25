//! Explicit trusted Cargo proof; no broker, HTTP worker or database execution.
use super::{dependency_profile, inputs};
use crate::source_snapshot::tests::ProofFixture;
use devoption_registry::Sha256;
use std::{fs, path::Path, process::Command};

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            assert!(kind.is_file());
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
#[ignore = "requires operator-verified vendor, its digest and an installed WASIp2 toolchain"]
fn prepared_records_json_build_is_locked_offline_and_rejects_tampering() {
    let vendor =
        std::env::var_os("DEVOPTION_TEST_WASIP2_VENDOR").expect("verified vendor directory");
    let expected = Sha256::from_hex(
        &std::env::var("DEVOPTION_TEST_WASIP2_VENDOR_SHA256").expect("vendor tree SHA-256"),
    )
    .unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let vendor_copy = root.join("vendor");
    copy_tree(Path::new(&vendor), &vendor_copy);
    assert!(inputs::digest_tree(&vendor_copy).unwrap() == expected);
    let manifest = include_bytes!("../../tests/fixtures/records-json/Cargo.toml");
    let lock = include_bytes!("../../tests/fixtures/records-json/Cargo.lock");
    let source = ProofFixture::with_files(&[
        ("Cargo.toml", manifest),
        ("Cargo.lock", lock),
        (
            "src/lib.rs",
            include_bytes!("../../tests/fixtures/records-json/src/lib.rs"),
        ),
        (
            "src/bindings.rs",
            include_bytes!("../../tests/fixtures/records-json/src/bindings.rs"),
        ),
    ])
    .verify()
    .unwrap();
    assert!(dependency_profile::matches(&source, Sha256::digest(lock)).unwrap());
    let project = root.join("app");
    fs::create_dir_all(project.join("src")).unwrap();
    for file in source.files() {
        fs::write(project.join(file.path()), file.bytes()).unwrap();
    }
    let run = |name: &str, project: &Path| {
        let cargo_home = root.join(format!("cargo-{name}"));
        fs::create_dir(&cargo_home).unwrap();
        fs::write(
            cargo_home.join("config.toml"),
            format!(
                "[source.crates-io]\nreplace-with='prepared'\n[source.prepared]\ndirectory='{}'\n",
                vendor_copy.display()
            ),
        )
        .unwrap();
        let cargo = Path::new(env!("CARGO"));
        let rustc = std::env::var_os("DEVOPTION_TEST_WASIP2_RUSTC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| cargo.with_file_name("rustc"));
        Command::new("/usr/bin/python3")
            .args(["-c", "import subprocess,sys; sys.exit(subprocess.run(sys.argv[1:], timeout=120).returncode)"])
            .arg(cargo)
            .args(["build", "--release", "--locked", "--offline", "--target", "wasm32-wasip2"])
            .current_dir(project).env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &root).env("CARGO_HOME", cargo_home)
            .env("CARGO_TARGET_DIR", root.join(format!("target-{name}")))
            .env("RUSTC", rustc).env("CARGO_NET_OFFLINE", "true")
            .env("CARGO_BUILD_JOBS", "1")
            .output().unwrap()
    };
    let built = run("valid", &project);
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let component =
        fs::read(root.join("target-valid/wasm32-wasip2/release/records_json_fixture.wasm"))
            .unwrap();
    assert!(component.starts_with(b"\0asm\x0d\0\x01\0"));
    assert!(
        component
            .windows(b"devoption:app/records".len())
            .any(|w| w == b"devoption:app/records")
    );
    let static_site = root.join("static-site");
    fs::create_dir_all(static_site.join("src")).unwrap();
    fs::create_dir(static_site.join("assets")).unwrap();
    for (name, bytes) in [
        (
            "Cargo.toml",
            include_bytes!("../../../../examples/static-site/Cargo.toml").as_slice(),
        ),
        (
            "Cargo.lock",
            include_bytes!("../../../../examples/static-site/Cargo.lock").as_slice(),
        ),
        (
            "src/lib.rs",
            include_bytes!("../../../../examples/static-site/src/lib.rs").as_slice(),
        ),
        (
            "assets/index.html",
            include_bytes!("../../../../examples/static-site/assets/index.html").as_slice(),
        ),
        (
            "assets/site.css",
            include_bytes!("../../../../examples/static-site/assets/site.css").as_slice(),
        ),
        (
            "assets/site.js",
            include_bytes!("../../../../examples/static-site/assets/site.js").as_slice(),
        ),
    ] {
        fs::write(static_site.join(name), bytes).unwrap();
    }
    let static_build = run("static", &static_site);
    assert!(
        static_build.status.success(),
        "{}",
        String::from_utf8_lossy(&static_build.stderr)
    );
    let manifest = vendor_copy.join("serde_json/src/lib.rs");
    fs::write(manifest, b"compile_error!(\"tampered\");").unwrap();
    // The installed prepare/approval/execute path uses this tree digest; Cargo's
    // own per-file checksum refusal is an additional independent check.
    assert!(inputs::digest_tree(&vendor_copy).unwrap() != expected);
    let denied = run("tampered", &project);
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("checksum"));
}
