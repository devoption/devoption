//! Trusted Cargo namespace proof in disposable directories; not a registry service.
#![cfg(unix)]
use devoption_registry::{
    CargoOrigin, HostedRegistries, HostedRegistry, RegistryId, Sha256, cargo_inputs,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
const INDEX: &str = "sparse+https://packages.example.test/index/";
const ARCHIVE: &[u8] = include_bytes!("fixtures/registry/itoa-1.0.18.crate");
const INDEX_ENTRY: &[u8] = include_bytes!("fixtures/registry/itoa.index");

fn cargo(root: &Path, home: &str, project: &Path, args: &[&str]) -> Output {
    let cargo = Path::new(env!("CARGO"));
    let rustc = cargo.with_file_name("rustc");
    assert!(cargo.is_absolute() && cargo.is_file() && rustc.is_file());
    let home = root.join(home);
    for path in [home.join("cargo"), home.join("home"), home.join("tmp")] {
        fs::create_dir_all(path).unwrap();
    }
    // Source replacement uses distinct stores. Private packages never enter the
    // crates.io source namespace, and every case gets fresh homes/output.
    fs::write(home.join("cargo/config.toml"), format!(
        "[registries.team]\nindex='{INDEX}'\n[source.team]\nregistry='{INDEX}'\nreplace-with='hosted-seed'\n[source.hosted-seed]\nlocal-registry='{}'\n[source.crates-io]\nreplace-with='public-seed'\n[source.public-seed]\nlocal-registry='{}'\n", root.join("hosted").display(), root.join("public").display()
    )).unwrap();
    let mut command = if cfg!(target_os = "linux") {
        let mut command = Command::new("/usr/bin/timeout");
        command.args(["--kill-after=5s", "60s"]);
        command
    } else {
        let mut command = Command::new("/usr/bin/python3");
        command.args([
            "-c",
            "import subprocess,sys; sys.exit(subprocess.run(sys.argv[1:], timeout=60).returncode)",
        ]);
        command
    };
    command
        .arg(cargo)
        .args(args)
        .current_dir(project)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.join("home"))
        .env("CARGO_HOME", home.join("cargo"))
        .env("CARGO_TARGET_DIR", home.join("target"))
        .env("TMPDIR", home.join("tmp"))
        .env("RUSTC", rustc)
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_BUILD_JOBS", "1")
        .env("RUSTC_WRAPPER", "")
        .env("RUSTC_WORKSPACE_WRAPPER", "")
        .output()
        .unwrap()
}
fn success(output: Output) -> Vec<u8> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn bindings() -> HostedRegistries {
    HostedRegistries::new(vec![
        HostedRegistry::new(RegistryId::new("team").unwrap(), INDEX).unwrap(),
    ])
    .unwrap()
}
fn admit(project: &Path) -> devoption_registry::CargoDependencyRequest {
    let manifest = fs::read(project.join("Cargo.toml")).unwrap();
    let lock = fs::read(project.join("Cargo.lock")).unwrap();
    cargo_inputs::validate_with_registries(
        RegistryId::new("fixture-provider").unwrap(),
        &manifest,
        &lock,
        cargo_inputs::InputHashes {
            manifest: Sha256::digest(&manifest),
            lockfile: Sha256::digest(&lock),
        },
        &bindings(),
    )
    .unwrap()
}
fn manifest(name: &str, version: &str) -> String {
    format!(
        "[package]\nname='{name}'\nversion='0.1.0'\nedition='2024'\nbuild=false\n[dependencies]\nitoa='=1.0.18'\nshared-helper={{version='={version}',registry='team'}}\n"
    )
}
fn app(root: &Path, name: &str, version: &str) -> PathBuf {
    let app = root.join(name);
    fs::create_dir_all(app.join("src")).unwrap();
    fs::write(app.join("Cargo.toml"), manifest(name, version)).unwrap();
    fs::write(app.join("src/main.rs"), "fn main() { let mut b = itoa::Buffer::new(); println!(\"{} {}\", shared_helper::label(), b.format(7)); }\n").unwrap();
    app
}
#[test]
#[ignore = "explicit trusted fixture builds two offline consumers from distinct hosted/public sources"]
fn cargo_generated_lock_graphs_keep_registry_origins_and_versioned_consumers_distinct() {
    let temporary = tempfile::Builder::new()
        .prefix("devoption-hosted-cargo-")
        .tempdir()
        .unwrap();
    let root = temporary.path().canonicalize().unwrap();
    for source in ["hosted", "public"] {
        fs::create_dir_all(root.join(source).join("index/it/oa")).unwrap();
        fs::write(root.join(source).join("index/it/oa/itoa"), INDEX_ENTRY).unwrap();
        fs::write(root.join(source).join("itoa-1.0.18.crate"), ARCHIVE).unwrap();
    }
    let helper = root.join("helper");
    fs::create_dir_all(helper.join("src")).unwrap();
    fs::create_dir_all(root.join("hosted/index/sh/ar")).unwrap();
    let mut entries = String::new();
    for (version, value) in [("0.1.0", 42), ("0.1.1", 43)] {
        fs::write(helper.join("Cargo.toml"), format!("[package]\nname='shared-helper'\nversion='{version}'\nedition='2024'\nbuild=false\n[lib]\nname='shared_helper'\ncrate-type=['rlib']\n[dependencies]\nitoa={{version='=1.0.18',registry='team'}}\n")).unwrap();
        fs::write(helper.join("src/lib.rs"), format!("pub fn label() -> String {{ let mut b = itoa::Buffer::new(); b.format({value}).to_owned() }}\n")).unwrap();
        success(cargo(
            &root,
            "package-home",
            &helper,
            &["generate-lockfile", "--offline"],
        ));
        success(cargo(
            &root,
            "package-home",
            &helper,
            &[
                "package",
                "--registry",
                "team",
                "--locked",
                "--offline",
                "--no-verify",
            ],
        ));
        let archive = fs::read(root.join(format!(
            "package-home/target/package/shared-helper-{version}.crate"
        )))
        .unwrap();
        let checksum = digest(&archive);
        fs::write(
            root.join(format!("hosted/shared-helper-{version}.crate")),
            &archive,
        )
        .unwrap();
        entries += &format!(
            "{{\"name\":\"shared-helper\",\"vers\":\"{version}\",\"deps\":[{{\"name\":\"itoa\",\"req\":\"=1.0.18\",\"features\":[],\"optional\":false,\"default_features\":true,\"target\":null,\"kind\":\"normal\",\"registry\":null}}],\"cksum\":\"{checksum}\",\"features\":{{}},\"yanked\":false}}\n"
        );
    }
    fs::write(root.join("hosted/index/sh/ar/shared-helper"), entries).unwrap();
    let first = app(&root, "consumer-one", "0.1.0");
    let second = app(&root, "consumer-two", "0.1.0");
    let mut original_lock = Vec::new();
    for (app, home) in [(&first, "first-a"), (&second, "second-a")] {
        success(cargo(&root, home, app, &["generate-lockfile", "--offline"]));
        let request = admit(app);
        assert_eq!(request.packages().len(), 3);
        let duplicates: Vec<_> = request
            .packages()
            .iter()
            .filter(|p| p.name == "itoa")
            .collect();
        assert_eq!(duplicates.len(), 2);
        assert!(duplicates.iter().any(|p| p.origin == CargoOrigin::CratesIo));
        assert!(
            duplicates
                .iter()
                .any(|p| p.origin == CargoOrigin::DevOption(RegistryId::new("team").unwrap()))
        );
        assert_eq!(
            success(cargo(
                &root,
                home,
                app,
                &["run", "--release", "--locked", "--offline"]
            )),
            b"42 7\n"
        );
        if app == &first {
            original_lock = fs::read(app.join("Cargo.lock")).unwrap();
        }
    }
    fs::write(first.join("Cargo.toml"), manifest("consumer-one", "0.1.1")).unwrap();
    success(cargo(
        &root,
        "first-b",
        &first,
        &["generate-lockfile", "--offline"],
    ));
    assert!(
        admit(&first)
            .packages()
            .iter()
            .any(|p| p.name == "shared-helper" && p.version == "0.1.1")
    );
    assert_eq!(
        success(cargo(
            &root,
            "first-b",
            &first,
            &["run", "--release", "--locked", "--offline"]
        )),
        b"43 7\n"
    );
    fs::write(first.join("Cargo.toml"), manifest("consumer-one", "0.1.0")).unwrap();
    fs::write(first.join("Cargo.lock"), &original_lock).unwrap();
    assert_eq!(
        success(cargo(
            &root,
            "first-a-again",
            &first,
            &["run", "--release", "--locked", "--offline"]
        )),
        b"42 7\n"
    );
    let manifest = fs::read(first.join("Cargo.toml")).unwrap();
    assert!(cargo_inputs::inspect(&manifest, &original_lock).is_err());
    let archive = root.join("hosted/shared-helper-0.1.0.crate");
    let valid = fs::read(&archive).unwrap();
    fs::remove_file(&archive).unwrap();
    assert!(
        !cargo(
            &root,
            "missing",
            &first,
            &["build", "--release", "--locked", "--offline"]
        )
        .status
        .success()
    );
    assert!(!root.join("missing/target/release/consumer-one").exists());
    fs::write(&archive, b"corrupt archive").unwrap();
    assert!(
        !cargo(
            &root,
            "corrupt",
            &first,
            &["build", "--release", "--locked", "--offline"]
        )
        .status
        .success()
    );
    assert!(!root.join("corrupt/target/release/consumer-one").exists());
    fs::write(&archive, &valid).unwrap();
    assert_eq!(fs::read(first.join("Cargo.lock")).unwrap(), original_lock);
    println!(
        "Two offline Cargo consumers, duplicate coordinates in public/hosted origins, exact lock admission, version update, retained older version and fresh-cache missing/corrupt refusal passed."
    );
}
