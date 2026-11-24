use super::*;
use crate::source_snapshot::tests::ProofFixture;
use devoption_registry::{CargoMaterialization, HostedRegistries, RegistryId};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Output},
};
const INDEX: &str = "sparse+https://packages.example.test/index/";
const ARCHIVE: &[u8] =
    include_bytes!("../../../../devoption-registry/tests/fixtures/registry/itoa-1.0.18.crate");
const INDEX_ENTRY: &[u8] =
    include_bytes!("../../../../devoption-registry/tests/fixtures/registry/itoa.index");
fn binding() -> HostedRegistry {
    HostedRegistry::new(RegistryId::new("team").unwrap(), INDEX).unwrap()
}
fn bindings() -> HostedRegistries {
    HostedRegistries::new(vec![binding()]).unwrap()
}
fn checksum(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(in crate::library) fn plan(
    name: &str,
    version: &str,
    dependencies: &str,
    edges: &str,
    rows: &str,
    code: &str,
    extra: Option<(&str, &[u8])>,
) -> BuildPlan {
    let manifest = format!(
        "[package]\nname='{name}'\nversion='{version}'\nedition='2024'\nbuild=false\n[lib]\nname='{}'\ncrate-type=['rlib']\n{dependencies}",
        name.replace('-', "_")
    );
    let lock = format!(
        "version=4\n[[package]]\nname='{name}'\nversion='{version}'\ndependencies=[{edges}]\n{rows}"
    );
    let mut files = vec![
        ("Cargo.toml", manifest.as_bytes()),
        ("Cargo.lock", lock.as_bytes()),
        ("src/lib.rs", code.as_bytes()),
    ];
    if let Some(extra) = extra {
        files.push(extra);
    }
    let source = ProofFixture::with_files(&files)
        .verify_with_registries(&bindings())
        .unwrap();
    let (catalog, mut input) = crate::tests::fixture();
    input.commit = source.commit().into();
    input.dependencies = CargoMaterialization::new(
        source.request().clone(),
        Sha256::digest(b"fixture vendor metadata"),
        1,
    );
    input.source = source;
    input.package = name.into();
    input.binary = name.replace('-', "_");
    input.rust_version = "1.99.0".into();
    BuildPlan::validate(input, &catalog).unwrap()
}
pub(in crate::library) fn base() -> CargoPackage {
    CargoPackage::from_plan(
        binding(),
        &plan(
            "private-base",
            "0.1.0",
            "",
            "",
            "",
            "pub fn value() -> u32 { 42 }\n",
            None,
        ),
    )
    .unwrap()
}
pub(in crate::library) fn helper(version: &str, addition: u32) -> CargoPackage {
    helper_for(binding(), version, addition)
}
pub(in crate::library) fn helper_for(
    destination: HostedRegistry,
    version: &str,
    addition: u32,
) -> CargoPackage {
    CargoPackage::from_plan(destination, &helper_plan(version, addition)).unwrap()
}
fn helper_plan(version: &str, addition: u32) -> BuildPlan {
    let base = base();
    let rows = format!(
        "[[package]]\nname='private-base'\nversion='0.1.0'\nsource='{INDEX}'\nchecksum='{}'\n[[package]]\nname='itoa'\nversion='1.0.18'\nsource='registry+{PUBLIC_INDEX}'\nchecksum='{}'\n",
        checksum(base.archive()),
        checksum(ARCHIVE)
    );
    let code = format!(
        "pub fn label() -> String {{ let mut b = itoa::Buffer::new(); b.format(private_base::value()+{addition}).to_owned() }}\n"
    );
    plan(
        "shared-helper",
        version,
        "[dependencies]\nitoa={version='=1.0.18',default-features=false}\nprivate-base={version='=0.1.0',registry='team'}\n",
        "'itoa','private-base'",
        &rows,
        &code,
        None,
    )
}
pub(in crate::library) fn publication_fixture() -> (devoption_projects::Catalog, [CargoPackage; 3])
{
    let (mut catalog, input) = crate::tests::fixture();
    catalog.create_project("Second consumer").unwrap();
    let plans = [
        plan(
            "private-base",
            "0.1.0",
            "",
            "",
            "",
            "pub fn value() -> u32 { 42 }\n",
            None,
        ),
        helper_plan("0.1.0", 0),
        helper_plan("0.2.0", 1),
    ];
    let packages = plans.map(|plan| {
        let mut details = plan.details().clone();
        details.project = input.project;
        details.repository = input.repository;
        let plan = BuildPlan::validate(details, &catalog).unwrap();
        CargoPackage::from_plan(binding(), &plan).unwrap()
    });
    (catalog, packages)
}
fn unpack(package: &CargoPackage) -> BTreeMap<String, Vec<u8>> {
    let mut tar = Vec::new();
    flate2::read::GzDecoder::new(package.archive())
        .read_to_end(&mut tar)
        .unwrap();
    assert!(tar.len() <= MAX_PACKAGE_BYTES);
    let mut files = BTreeMap::new();
    let mut at = 0;
    while tar[at..at + 512].iter().any(|&b| b != 0) {
        let header = &tar[at..at + 512];
        let text = |bytes: &[u8]| {
            String::from_utf8(bytes.iter().copied().take_while(|&b| b != 0).collect()).unwrap()
        };
        let name = text(&header[..100]);
        let prefix = text(&header[345..500]);
        let name = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let size = usize::from_str_radix(text(&header[124..136]).trim(), 8).unwrap();
        at += 512;
        assert!(files.insert(name, tar[at..at + size].to_vec()).is_none());
        at += size.div_ceil(512) * 512;
    }
    assert_eq!(&tar[at..], &[0; 1024]);
    files
}

#[test]
fn deterministic_crate_binds_exact_source_and_protocol_metadata() {
    let first = helper("0.1.0", 0);
    let second = helper("0.1.0", 0);
    // Synthetic catalog IDs differ per fixture, but canonical source artifacts do not.
    assert_eq!(first.archive(), second.archive());
    assert_eq!(first.index_line(), second.index_line());
    assert!(first.source().source_sha256() == second.source().source_sha256());
    assert!(first.archive_sha256() == Sha256::digest(first.archive()));
    assert_eq!(&first.archive()[..10], &[31, 139, 8, 0, 0, 0, 0, 0, 0, 255]);
    assert_eq!(first.index_path(), "sh/ar/shared-helper");
    let row: serde_json::Value = serde_json::from_slice(first.index_line()).unwrap();
    assert_eq!(row["cksum"], checksum(first.archive()));
    assert_eq!(row["deps"][0]["registry"], PUBLIC_INDEX);
    assert_eq!(row["deps"][0]["default_features"], false);
    assert!(row["deps"][1]["registry"].is_null());
    assert_eq!(first.index_line().last(), Some(&b'\n'));
    let files = unpack(&first);
    let normalized = std::str::from_utf8(&files["shared-helper-0.1.0/Cargo.toml"]).unwrap();
    let manifest: toml::Table = toml::from_str(normalized).unwrap();
    assert_eq!(
        manifest["dependencies"]["itoa"]["registry-index"].as_str(),
        Some(PUBLIC_INDEX)
    );
    assert_eq!(
        manifest["dependencies"]["private-base"]["registry-index"].as_str(),
        Some(INDEX)
    );
    assert!(
        manifest["dependencies"]["private-base"]
            .get("registry")
            .is_none()
    );
    assert!(
        std::str::from_utf8(&files["shared-helper-0.1.0/Cargo.toml.orig"])
            .unwrap()
            .contains("registry='team'")
    );
    assert!(files.contains_key("shared-helper-0.1.0/Cargo.lock"));
    assert!(files.contains_key("shared-helper-0.1.0/src/lib.rs"));
    assert_ne!(first.archive(), helper("0.2.0", 1).archive());
}

#[test]
fn index_paths_and_reserved_generated_names_are_checked() {
    for (name, path) in [
        ("a", "1/a"),
        ("ab", "2/ab"),
        ("AbC", "3/a/abc"),
        ("AbCd", "ab/cd/abcd"),
    ] {
        let package = CargoPackage::from_plan(
            binding(),
            &plan(name, "0.1.0", "", "", "", "pub fn value() {}", None),
        )
        .unwrap();
        assert_eq!(package.index_path(), path);
    }
    for name in [
        "Cargo.toml.orig",
        "CARGO.TOML.ORIG",
        "Cargo.toml.orig/contents",
        ".cargo_vcs_info.json",
        ".cargo-checksum.json",
    ] {
        let plan = plan(
            "fixture",
            "0.1.0",
            "",
            "",
            "",
            "pub fn value() {}",
            Some((name, b"do not overwrite")),
        );
        assert!(CargoPackage::from_plan(binding(), &plan).is_err());
    }
    let (catalog, input) = crate::tests::fixture();
    assert!(
        CargoPackage::from_plan(binding(), &BuildPlan::validate(input, &catalog).unwrap()).is_err()
    );
}

#[test]
fn a_different_destination_preserves_both_dependency_registry_urls() {
    let destination = HostedRegistry::new(
        RegistryId::new("other").unwrap(),
        "https://other.example.test/index",
    )
    .unwrap();
    let package = helper_for(destination.clone(), "0.1.0", 0);
    let row: serde_json::Value = serde_json::from_slice(package.index_line()).unwrap();
    assert_eq!(row["deps"][0]["registry"], PUBLIC_INDEX);
    assert_eq!(row["deps"][1]["registry"], INDEX);
    assert!(package.source().coordinates().registry() == &destination);
    assert!(row["features"].as_object().unwrap().is_empty());
    assert_eq!(row["yanked"], false);
}

#[test]
fn normalized_manifest_overhead_cannot_exceed_the_expanded_archive_limit() {
    let baseline = plan("fixture", "0.1.0", "", "", "", "pub fn value() {}", None);
    let mut files: Vec<(String, Vec<u8>)> = baseline
        .details()
        .source
        .files()
        .iter()
        .map(|file| (file.path().into(), file.bytes().to_vec()))
        .collect();
    for n in 0..8 {
        files.push((
            format!("data/asset{n}"),
            vec![0; 512 * 1024 - if n == 7 { 8192 } else { 0 }],
        ));
    }
    let source = ProofFixture::with_files(
        &files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
            .collect::<Vec<_>>(),
    )
    .verify_with_registries(&bindings())
    .unwrap();
    assert_eq!(
        super::super::source_package(&source).unwrap().len(),
        MAX_PACKAGE_BYTES
    );
    let (catalog, mut input) = crate::tests::fixture();
    input.commit = source.commit().into();
    input.package = source.package().into();
    input.binary = source.package().replace('-', "_");
    input.rust_version = "1.99.0".into();
    input.dependencies =
        CargoMaterialization::new(source.request().clone(), Sha256::digest(b"fixture"), 1);
    input.source = source;
    let plan = BuildPlan::validate(input, &catalog).unwrap();
    assert!(CargoPackage::from_plan(binding(), &plan).is_err());
}

fn cargo(root: &Path, home: &str, app: &Path, args: &[&str]) -> Output {
    let cargo = Path::new(env!("CARGO"));
    let home = root.join(home);
    fs::create_dir_all(home.join("cargo")).unwrap();
    fs::create_dir_all(home.join("home")).unwrap();
    fs::create_dir_all(home.join("tmp")).unwrap();
    fs::write(home.join("cargo/config.toml"), format!("[registries.team]\nindex='{INDEX}'\n[source.team]\nregistry='{INDEX}'\nreplace-with='hosted-seed'\n[source.hosted-seed]\nlocal-registry='{}'\n[source.crates-io]\nreplace-with='public-seed'\n[source.public-seed]\nlocal-registry='{}'\n", root.join("hosted").display(),root.join("public").display())).unwrap();
    let vendor_config = root.join(format!(
        "vendor-config-{}.toml",
        app.file_name().unwrap().to_str().unwrap()
    ));
    if args.contains(&"--locked") && vendor_config.exists() {
        fs::copy(vendor_config, home.join("cargo/config.toml")).unwrap();
    }
    let mut command = if cfg!(target_os = "linux") {
        let mut c = Command::new("/usr/bin/timeout");
        c.args(["--kill-after=5s", "60s"]);
        c
    } else {
        let mut c = Command::new("/usr/bin/python3");
        c.args([
            "-c",
            "import subprocess,sys; sys.exit(subprocess.run(sys.argv[1:], timeout=60).returncode)",
        ]);
        c
    };
    command
        .arg(cargo)
        .args(args)
        .current_dir(app)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home.join("home"))
        .env("CARGO_HOME", home.join("cargo"))
        .env("CARGO_TARGET_DIR", home.join("target"))
        .env("TMPDIR", home.join("tmp"))
        .env("RUSTC", cargo.with_file_name("rustc"))
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
fn seed(root: &Path, package: &CargoPackage, index: &[u8], archive: &[u8]) {
    let path = root.join("hosted/index").join(package.index_path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, index).unwrap();
    fs::write(
        root.join("hosted").join(format!(
            "{}-{}.crate",
            package.source().coordinates().name(),
            package.source().coordinates().version()
        )),
        archive,
    )
    .unwrap();
}
fn app(root: &Path, name: &str, version: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(path.join("src")).unwrap();
    fs::write(path.join("Cargo.toml"),format!("[package]\nname='{name}'\nversion='0.1.0'\nedition='2024'\nbuild=false\n[dependencies]\nitoa='=1.0.18'\nshared-helper={{version='={version}',registry='team'}}\n")).unwrap();
    fs::write(path.join("src/main.rs"),"fn main() { let mut b = itoa::Buffer::new(); println!(\"{} {}\", shared_helper::label(), b.format(7)); }\n").unwrap();
    path
}

fn seed_locked_archives(
    policy: &crate::library::publication::Publications,
    project: devoption_projects::ProjectId,
    catalog: &devoption_projects::Catalog,
    store: &crate::library::package_store::PackageStore,
    app: &Path,
    root: &Path,
    packages: &[CargoPackage],
) {
    use crate::library::publication::dependencies::PublicArchive;
    use devoption_registry::CargoOrigin;
    let manifest = fs::read(app.join("Cargo.toml")).unwrap();
    let lock = fs::read(app.join("Cargo.lock")).unwrap();
    let main = fs::read(app.join("src/main.rs")).unwrap();
    let snapshot = ProofFixture::with_files(&[
        ("Cargo.toml", &manifest),
        ("Cargo.lock", &lock),
        ("src/main.rs", &main),
    ])
    .verify_with_registries(&bindings())
    .unwrap();
    let public: Vec<_> = snapshot
        .request()
        .packages()
        .iter()
        .filter(|p| p.origin == CargoOrigin::CratesIo)
        .map(|package| PublicArchive {
            package,
            bytes: ARCHIVE,
        })
        .collect();
    let acquired = policy
        .locked_archives(
            crate::approvals::OwnerId::from_bytes([1; 16]),
            project,
            catalog,
            store,
            &snapshot,
            &public,
        )
        .unwrap();
    policy
        .revalidate_archives(
            crate::approvals::OwnerId::from_bytes([1; 16]),
            catalog,
            store,
            &snapshot,
            &acquired,
        )
        .unwrap();
    assert_eq!(
        acquired.archives().len(),
        snapshot.request().packages().len()
    );
    for archive in acquired.archives() {
        match &archive.package().origin {
            CargoOrigin::CratesIo => fs::write(
                root.join("public").join(format!(
                    "{}-{}.crate",
                    archive.package().name,
                    archive.package().version
                )),
                archive.bytes(),
            )
            .unwrap(),
            CargoOrigin::DevOption(id) => {
                let template = packages
                    .iter()
                    .find(|p| {
                        p.source().coordinates().registry().id() == id
                            && p.source().coordinates().name() == archive.package().name
                            && p.source().coordinates().version() == archive.package().version
                    })
                    .unwrap();
                let entry = policy
                    .index_entry(
                        crate::approvals::OwnerId::from_bytes([1; 16]),
                        project,
                        catalog,
                        store,
                        template.source().coordinates().registry(),
                        template.index_path(),
                    )
                    .unwrap();
                assert!(archive.index_line().is_some());
                seed(root, template, &entry, archive.bytes());
            }
        }
    }
    let inputs = policy
        .vendor_inputs(
            crate::approvals::OwnerId::from_bytes([1; 16]),
            catalog,
            store,
            &snapshot,
            &acquired,
        )
        .unwrap();
    inputs
        .revalidate(
            policy,
            crate::approvals::OwnerId::from_bytes([1; 16]),
            catalog,
            store,
            &snapshot,
            &acquired,
        )
        .unwrap();
    assert!(Sha256::digest(inputs.bundle_bytes()) == inputs.materialization().bundle_sha256());
    let vendor = root.join("vendor").join(app.file_name().unwrap());
    if vendor.exists() {
        fs::remove_dir_all(&vendor).unwrap();
    }
    fs::create_dir_all(&vendor).unwrap();
    for source in inputs.sources() {
        fs::create_dir_all(vendor.join(source.directory())).unwrap();
    }
    for file in inputs.files() {
        let target = vendor.join(file.path());
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, file.bytes()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(file.mode().into())).unwrap();
    }
    fs::write(
        root.join(format!(
            "vendor-config-{}.toml",
            app.file_name().unwrap().to_str().unwrap()
        )),
        inputs.cargo_config(&vendor).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "explicit trusted Cargo consumers of DevOption-generated packages; no registry service"]
fn generated_cargo_packages_build_two_consumers_and_retain_older_versions() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("public/index/it/oa")).unwrap();
    fs::write(root.join("public/index/it/oa/itoa"), INDEX_ENTRY).unwrap();
    fs::write(root.join("public/itoa-1.0.18.crate"), ARCHIVE).unwrap();
    // Consumers receive bytes reloaded from the durable candidate store, never
    // directly from in-memory generation. This is still a private test seed.
    use crate::library::package_store::PackageStore;
    use std::os::unix::fs::PermissionsExt;
    let candidates = root.join("candidates");
    fs::create_dir(&candidates).unwrap();
    fs::set_permissions(&candidates, fs::Permissions::from_mode(0o700)).unwrap();
    let (mut catalog, packages) = publication_fixture();
    let ungranted = catalog.create_project("Ungrantable test consumer").unwrap();
    let source = devoption_projects::ProjectId::from_bytes(packages[0].source().project());
    let consumers: Vec<_> = catalog
        .projects()
        .iter()
        .filter(|p| p.id() != source && p.id() != ungranted)
        .map(|p| p.id())
        .collect();
    let owner = crate::approvals::OwnerId::from_bytes([1; 16]);
    use crate::library::publication::{Change, Error as PolicyError, Publications};
    let policy = root.join("policy");
    fs::create_dir(&policy).unwrap();
    fs::set_permissions(&policy, fs::Permissions::from_mode(0o700)).unwrap();
    let mut publication = Publications::open(policy.to_str().unwrap(), owner).unwrap();
    let mut store = PackageStore::open(candidates.to_str().unwrap()).unwrap();
    for package in &packages {
        store.stage(package).unwrap();
    }
    drop(store);
    let store = PackageStore::open(candidates.to_str().unwrap()).unwrap();
    for package in &packages[..2] {
        let coordinates = package.source().coordinates();
        assert_eq!(
            publication.index_line(owner, consumers[0], &catalog, &store, coordinates),
            Err(PolicyError::Denied)
        );
        let review = publication
            .review(
                owner,
                &catalog,
                &store,
                coordinates,
                Change::Publish {
                    readers: consumers.clone(),
                },
            )
            .unwrap();
        publication
            .approve(owner, &catalog, &store, review)
            .unwrap();
    }
    drop(publication);
    let mut publication = Publications::open(policy.to_str().unwrap(), owner).unwrap();
    for package in &packages[..2] {
        let coordinates = package.source().coordinates();
        let index = publication
            .index_entry(
                owner,
                consumers[0],
                &catalog,
                &store,
                coordinates.registry(),
                package.index_path(),
            )
            .unwrap();
        let archive = publication
            .archive(
                owner,
                consumers[0],
                &catalog,
                &store,
                coordinates,
                package.archive_sha256(),
            )
            .unwrap();
        assert_eq!(
            publication
                .archive(
                    owner,
                    consumers[1],
                    &catalog,
                    &store,
                    coordinates,
                    package.archive_sha256()
                )
                .unwrap(),
            archive
        );
        assert_eq!(
            publication.archive(
                owner,
                ungranted,
                &catalog,
                &store,
                coordinates,
                package.archive_sha256()
            ),
            Err(PolicyError::Denied)
        );
        seed(&root, package, &index, &archive);
    }
    let first = app(&root, "consumer-one", "0.1.0");
    let second = app(&root, "consumer-two", "0.1.0");
    for (path, home) in [(&first, "first"), (&second, "second")] {
        success(cargo(
            &root,
            home,
            path,
            &["generate-lockfile", "--offline"],
        ));
        let request = devoption_registry::cargo_inputs::inspect_with_registries(
            &fs::read(path.join("Cargo.toml")).unwrap(),
            &fs::read(path.join("Cargo.lock")).unwrap(),
            &bindings(),
        )
        .unwrap();
        assert_eq!(request.dependency_count(), 3);
        seed_locked_archives(
            &publication,
            if path == &first {
                consumers[0]
            } else {
                consumers[1]
            },
            &catalog,
            &store,
            path,
            &root,
            &packages,
        );
        assert_eq!(
            success(cargo(
                &root,
                home,
                path,
                &["run", "--release", "--locked", "--offline"]
            )),
            b"42 7\n"
        );
    }
    let lock = fs::read(first.join("Cargo.lock")).unwrap();
    // The update becomes available only after a fresh explicit owner review.
    let package = &packages[2];
    let coordinates = package.source().coordinates();
    assert_eq!(
        publication.index_line(owner, consumers[0], &catalog, &store, coordinates),
        Err(PolicyError::Denied)
    );
    let review = publication
        .review(
            owner,
            &catalog,
            &store,
            coordinates,
            Change::Publish {
                readers: consumers.clone(),
            },
        )
        .unwrap();
    publication
        .approve(owner, &catalog, &store, review)
        .unwrap();
    let index = publication
        .index_entry(
            owner,
            consumers[0],
            &catalog,
            &store,
            coordinates.registry(),
            package.index_path(),
        )
        .unwrap();
    let archive = publication
        .archive(
            owner,
            consumers[0],
            &catalog,
            &store,
            coordinates,
            package.archive_sha256(),
        )
        .unwrap();
    seed(&root, package, &index, &archive);
    app(&root, "consumer-one", "0.2.0");
    success(cargo(
        &root,
        "updated",
        &first,
        &["generate-lockfile", "--offline"],
    ));
    seed_locked_archives(
        &publication,
        consumers[0],
        &catalog,
        &store,
        &first,
        &root,
        &packages,
    );
    assert_eq!(
        success(cargo(
            &root,
            "updated",
            &first,
            &["run", "--release", "--locked", "--offline"]
        )),
        b"43 7\n"
    );
    let coordinates = packages[1].source().coordinates();
    let review = publication
        .review(
            owner,
            &catalog,
            &store,
            coordinates,
            Change::Yank { yanked: true },
        )
        .unwrap();
    publication
        .approve(owner, &catalog, &store, review)
        .unwrap();
    let index = publication
        .index_entry(
            owner,
            consumers[0],
            &catalog,
            &store,
            coordinates.registry(),
            packages[1].index_path(),
        )
        .unwrap();
    fs::write(
        root.join("hosted/index").join(packages[1].index_path()),
        index,
    )
    .unwrap();
    let fresh = app(&root, "new-yanked-consumer", "0.1.0");
    assert!(
        !cargo(
            &root,
            "new-yanked",
            &fresh,
            &["generate-lockfile", "--offline"]
        )
        .status
        .success()
    );
    assert!(!fresh.join("Cargo.lock").exists());
    app(&root, "consumer-one", "0.1.0");
    fs::write(first.join("Cargo.lock"), lock).unwrap();
    seed_locked_archives(
        &publication,
        consumers[0],
        &catalog,
        &store,
        &first,
        &root,
        &packages,
    );
    assert_eq!(
        success(cargo(
            &root,
            "older-fresh",
            &first,
            &["run", "--release", "--locked", "--offline"]
        )),
        b"42 7\n"
    );
    let vendor_source = fs::read_dir(root.join("vendor/consumer-one"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("hosted-")
        })
        .unwrap()
        .join("private-base-0.1.0/src/lib.rs");
    fs::remove_file(&vendor_source).unwrap();
    let archive = root.join("hosted/private-base-0.1.0.crate");
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
    fs::write(&archive, b"corrupt archive").unwrap();
    fs::write(&vendor_source, b"corrupt vendor source").unwrap();
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
    for home in ["missing", "corrupt"] {
        assert!(!root.join(home).join("target/release/consumer-one").exists());
    }
    let review = publication
        .review(
            owner,
            &catalog,
            &store,
            coordinates,
            Change::Readers {
                readers: vec![consumers[1]],
            },
        )
        .unwrap();
    publication
        .approve(owner, &catalog, &store, review)
        .unwrap();
    assert_eq!(
        publication.archive(
            owner,
            consumers[0],
            &catalog,
            &store,
            coordinates,
            packages[1].archive_sha256()
        ),
        Err(PolicyError::Denied)
    );
    assert!(
        publication
            .archive(
                owner,
                consumers[1],
                &catalog,
                &store,
                coordinates,
                packages[1].archive_sha256()
            )
            .is_ok()
    );
    println!(
        "DevOption-generated Cargo verified vendor bundles/locked archives/aggregated indexes: reviewed publication and project access, two consumers, mixed registry closure, exact update, yanked fresh-resolution refusal, old locked-version rebuild and fresh-cache missing/corrupt refusal passed."
    );
}

#[test]
fn unbuilt_candidate_codec_remains_version_one_and_byte_stable() {
    let package = base();
    assert!(package.build_provenance().is_none());
    let encoded = package.encode();
    assert_eq!(&encoded[..8], b"DVCARGO1");
    let decoded = CargoPackage::decode(&encoded).unwrap();
    assert!(decoded == package);
    assert_eq!(decoded.encode(), encoded);
}

#[test]
fn decoder_rejects_dependency_metadata_edits_with_valid_record_checksums() {
    use super::super::staging::Error;
    // Exercise both null (same destination) and explicit hosted registry fields.
    for package in [
        helper("0.1.0", 0),
        helper_for(
            HostedRegistry::new(
                RegistryId::new("other").unwrap(),
                "sparse+https://other.example.test/index/",
            )
            .unwrap(),
            "0.1.0",
            0,
        ),
    ] {
        assert!(CargoPackage::decode(&package.encode()).unwrap() == package);
        let original: serde_json::Value = serde_json::from_slice(package.index_line()).unwrap();
        let mut edits = Vec::new();
        for (field, value) in [
            ("name", json!("different-package")),
            ("req", json!("=99.0.0")),
            ("features", json!(["extra"])),
            ("optional", json!(true)),
            ("default_features", json!(true)),
            ("target", json!("cfg(unix)")),
            ("kind", json!("build")),
            ("registry", json!(INDEX)),
        ] {
            let mut row = original.clone();
            row["deps"][0][field] = value;
            edits.push(row);
        }
        let mut row = original.clone();
        row["deps"][1]["registry"] = json!(PUBLIC_INDEX);
        edits.push(row);
        let mut row = original.clone();
        row["deps"] = json!([]);
        edits.push(row);
        let mut row = original.clone();
        row["deps"].as_array_mut().unwrap().reverse();
        edits.push(row);
        let mut row = original.clone();
        row["deps"]
            .as_array_mut()
            .unwrap()
            .push(original["deps"][0].clone());
        edits.push(row);
        for row in edits {
            let mut changed = package.clone();
            changed.index_line = serde_json::to_vec(&row).unwrap();
            changed.index_line.push(b'\n');
            // encode recomputes the complete record checksum: refusal must be
            // based on archive/index disagreement, not accidental corruption.
            assert_eq!(
                CargoPackage::decode(&changed.encode()).err(),
                Some(Error::Integrity)
            );
        }
    }
}

#[test]
fn decoder_rejects_injected_dependencies_in_dependency_free_package() {
    let mut package = base();
    let mut row: serde_json::Value = serde_json::from_slice(package.index_line()).unwrap();
    row["deps"] = json!([{"name":"injected","req":"=1.0.0","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null}]);
    package.index_line = serde_json::to_vec(&row).unwrap();
    package.index_line.push(b'\n');
    assert_eq!(
        CargoPackage::decode(&package.encode()).err(),
        Some(super::super::staging::Error::Integrity)
    );
}

#[test]
fn decoder_bounds_archive_inspection_even_with_matching_index_checksums() {
    for expanded in [b"not a tar".to_vec(), vec![0; MAX_PACKAGE_BYTES + 512]] {
        let mut package = base();
        let mut encoder = GzBuilder::new().write(Vec::new(), Compression::new(6));
        encoder.write_all(&expanded).unwrap();
        package.archive = encoder.finish().unwrap();
        let mut row: serde_json::Value = serde_json::from_slice(package.index_line()).unwrap();
        row["cksum"] = json!(checksum(&package.archive));
        package.index_line = serde_json::to_vec(&row).unwrap();
        package.index_line.push(b'\n');
        assert_eq!(
            CargoPackage::decode(&package.encode()).err(),
            Some(super::super::staging::Error::Integrity)
        );
    }
}

#[test]
fn decoder_rejects_coordinated_archive_and_index_edits() {
    let package = helper("0.1.0", 0);
    let root = "shared-helper-0.1.0/";
    let original: BTreeMap<String, Vec<u8>> = unpack(&package)
        .into_iter()
        .map(|(path, bytes)| (path.strip_prefix(root).unwrap().to_owned(), bytes))
        .collect();
    for edit in [
        "code",
        "added",
        "removed",
        "original",
        "requirement",
        "defaults",
        "registry",
        "edition",
        "build",
        "path",
    ] {
        let mut files = original.clone();
        match edit {
            "code" => {
                files.insert("src/lib.rs".into(), b"pub fn changed() {}\n".to_vec());
            }
            "added" => {
                files.insert("src/extra.rs".into(), b"// injected\n".to_vec());
            }
            "removed" => {
                files.remove("src/lib.rs");
            }
            "original" => {
                files
                    .get_mut("Cargo.toml.orig")
                    .unwrap()
                    .extend(b"\n# changed\n");
            }
            _ => {
                let manifest = std::str::from_utf8(&files["Cargo.toml"]).unwrap();
                let (from, to) = match edit {
                    "requirement" => ("=1.0.18", "=99.0.0"),
                    "defaults" => ("default-features=false", "default-features=true"),
                    "registry" => (INDEX, "sparse+https://changed.example.test/index/"),
                    "edition" => ("edition=\"2024\"", "edition=\"2021\""),
                    "build" => ("build=false", "build=true"),
                    "path" => ("src/lib.rs", "src/other.rs"),
                    _ => unreachable!(),
                };
                assert!(manifest.contains(from), "{edit}");
                files.insert("Cargo.toml".into(), manifest.replace(from, to).into_bytes());
            }
        }
        let mut changed = package.clone();
        let tar = source_tar(
            "shared-helper",
            "0.1.0",
            files
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        )
        .unwrap();
        let mut encoder = GzBuilder::new()
            .mtime(0)
            .operating_system(255)
            .write(Vec::new(), Compression::new(6));
        encoder.write_all(&tar).unwrap();
        changed.archive = encoder.finish().unwrap();
        changed.sha256 = Sha256::digest(&changed.archive);
        let manifest: toml::Table = std::str::from_utf8(&files["Cargo.toml"])
            .unwrap()
            .parse()
            .unwrap();
        let deps: Vec<_> = manifest["dependencies"]
            .as_table()
            .unwrap()
            .iter()
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .map(|(name, value)| {
                let fields = value.as_table().unwrap();
                let index = fields["registry-index"].as_str().unwrap();
                json!({
                    "name": name,
                    "req": fields["version"].as_str().unwrap(),
                    "features": [],
                    "optional": false,
                    "default_features": fields["default-features"].as_bool().unwrap(),
                    "target": null,
                    "kind": "normal",
                    "registry": if index == changed.source.coordinates().registry().index() { None } else { Some(index) }
                })
            })
            .collect();
        let mut line = serde_json::to_vec(&json!({
            "name": changed.source.coordinates().name(),
            "vers": changed.source.coordinates().version(),
            "deps": deps,
            "cksum": checksum(&changed.archive),
            "features": {},
            "yanked": false
        }))
        .unwrap();
        line.push(b'\n');
        changed.index_line = line;
        // Archive, index and outer digest agree; only the source binding detects this.
        assert_eq!(
            CargoPackage::decode(&changed.encode()).err(),
            Some(super::super::staging::Error::Integrity),
            "{edit}"
        );
    }
}

#[test]
fn decoder_uses_source_qualified_direct_lock_edges() {
    let rows = format!(
        "[[package]]\nname='private-base'\nversion='0.1.0'\nsource='{INDEX}'\nchecksum='{}'\n\
         [[package]]\nname='private-base'\nversion='0.1.0'\nsource='registry+{PUBLIC_INDEX}'\nchecksum='{}'\n\
         [[package]]\nname='bridge'\nversion='0.1.0'\nsource='registry+{PUBLIC_INDEX}'\nchecksum='{}'\ndependencies=['private-base 0.1.0 (registry+{PUBLIC_INDEX})']\n",
        checksum(b"hosted"),
        checksum(b"public"),
        checksum(b"bridge"),
    );
    let package = CargoPackage::from_plan(binding(), &plan(
        "qualified-helper", "0.1.0",
        "[dependencies]\nprivate-base={version='=0.1.0',registry='team'}\nbridge={version='=0.1.0',default-features=false}\n",
        &format!("'private-base 0.1.0 ({INDEX})','bridge'"),
        &rows, "pub fn value() {}\n", None,
    )).unwrap();
    assert!(CargoPackage::decode(&package.encode()).unwrap() == package);
}

#[test]
fn package_generation_uses_admitted_direct_dependencies_and_edition() {
    let second_index = "https://other.example.test/index";
    let registries = HostedRegistries::new(vec![
        binding(),
        HostedRegistry::new(RegistryId::new("other").unwrap(), second_index).unwrap(),
    ])
    .unwrap();
    let manifest = "\
[package]
name = 'admitted-helper'
version = '0.2.0'
edition = '2021'
build = false

[lib]
name = 'admitted_helper'
crate-type = ['rlib']

[dependencies]
z-public = '=1.0.1'
a-public-min = { version = '=1.0.0', default-features = false }
m-hosted = { version = '=0.1.0', registry = 'team' }
b-hosted-min = { version = '=0.2.0', registry = 'other', default-features = false }
";
    let lock = format!(
        "version = 4
[[package]]
name = 'admitted-helper'
version = '0.2.0'
dependencies = ['a-public-min', 'b-hosted-min', 'm-hosted', 'z-public']
[[package]]
name = 'a-public-min'
version = '1.0.0'
source = 'registry+{PUBLIC_INDEX}'
checksum = '{}'
[[package]]
name = 'b-hosted-min'
version = '0.2.0+meta.1'
source = 'registry+{second_index}'
checksum = '{}'
[[package]]
name = 'm-hosted'
version = '0.1.0'
source = '{INDEX}'
checksum = '{}'
[[package]]
name = 'z-public'
version = '1.0.1+build.9'
source = 'registry+{PUBLIC_INDEX}'
checksum = '{}'
",
        checksum(b"a-public-min"),
        checksum(b"b-hosted-min"),
        checksum(b"m-hosted"),
        checksum(b"z-public"),
    );
    let source = ProofFixture::with_files(&[
        ("Cargo.toml", manifest.as_bytes()),
        ("Cargo.lock", lock.as_bytes()),
        ("src/lib.rs", b"pub fn ok() {}\n"),
    ])
    .verify_with_registries(&registries)
    .unwrap();
    assert_eq!(source.edition(), "2021");
    assert!(
        source.direct_dependencies()
            == [
                DirectDependency {
                    name: "a-public-min".into(),
                    requirement: "=1.0.0".into(),
                    default_features: false,
                    origin: CargoOrigin::CratesIo,
                },
                DirectDependency {
                    name: "b-hosted-min".into(),
                    requirement: "=0.2.0".into(),
                    default_features: false,
                    origin: CargoOrigin::DevOption(RegistryId::new("other").unwrap()),
                },
                DirectDependency {
                    name: "m-hosted".into(),
                    requirement: "=0.1.0".into(),
                    default_features: true,
                    origin: CargoOrigin::DevOption(RegistryId::new("team").unwrap()),
                },
                DirectDependency {
                    name: "z-public".into(),
                    requirement: "=1.0.1".into(),
                    default_features: true,
                    origin: CargoOrigin::CratesIo,
                },
            ]
    );
    let package = CargoPackage::from_source(binding(), [1; 16], [2; 16], &source).unwrap();
    let files = unpack(&package);
    let normalized: toml::Table = std::str::from_utf8(&files["admitted-helper-0.2.0/Cargo.toml"])
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(normalized["package"]["edition"].as_str(), Some("2021"));
    assert_eq!(
        normalized["dependencies"]["a-public-min"]["default-features"].as_bool(),
        Some(false)
    );
    assert_eq!(
        normalized["dependencies"]["b-hosted-min"]["registry-index"].as_str(),
        Some(second_index)
    );
    assert_eq!(
        normalized["dependencies"]["m-hosted"]["default-features"].as_bool(),
        Some(true)
    );
    assert_eq!(
        normalized["dependencies"]["z-public"]["version"].as_str(),
        Some("=1.0.1")
    );
    let row: serde_json::Value = serde_json::from_slice(package.index_line()).unwrap();
    assert_eq!(
        row["deps"],
        json!([
            {"name":"a-public-min","req":"=1.0.0","features":[],"optional":false,"default_features":false,"target":null,"kind":"normal","registry":PUBLIC_INDEX},
            {"name":"b-hosted-min","req":"=0.2.0","features":[],"optional":false,"default_features":false,"target":null,"kind":"normal","registry":second_index},
            {"name":"m-hosted","req":"=0.1.0","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":null},
            {"name":"z-public","req":"=1.0.1","features":[],"optional":false,"default_features":true,"target":null,"kind":"normal","registry":PUBLIC_INDEX},
        ])
    );
    assert!(CargoPackage::decode(&package.encode()).unwrap() == package);
}

#[test]
fn decoder_round_trips_transitive_hosted_registry_dependencies() {
    let other_index = "sparse+https://other.example.test/index/";
    let registries = HostedRegistries::new(vec![
        binding(),
        HostedRegistry::new(RegistryId::new("other").unwrap(), other_index).unwrap(),
    ])
    .unwrap();
    let manifest = "\
[package]
name = 'transitive-helper'
version = '0.1.0'
edition = '2024'
build = false

[lib]
name = 'transitive_helper'
crate-type = ['rlib']

[dependencies]
direct-hosted = { version = '=0.1.0', registry = 'team' }
";
    let lock = format!(
        "version = 4
[[package]]
name = 'transitive-helper'
version = '0.1.0'
dependencies = ['direct-hosted']
[[package]]
name = 'direct-hosted'
version = '0.1.0'
source = '{INDEX}'
checksum = '{}'
dependencies = ['indirect-hosted']
[[package]]
name = 'indirect-hosted'
version = '0.1.0'
source = '{other_index}'
checksum = '{}'
",
        checksum(b"direct-hosted"),
        checksum(b"indirect-hosted"),
    );
    let source = ProofFixture::with_files(&[
        ("Cargo.toml", manifest.as_bytes()),
        ("Cargo.lock", lock.as_bytes()),
        ("src/lib.rs", b"pub fn ok() {}\n"),
    ])
    .verify_with_registries(&registries)
    .unwrap();
    let package = CargoPackage::from_source(binding(), [3; 16], [4; 16], &source).unwrap();
    let row: serde_json::Value = serde_json::from_slice(package.index_line()).unwrap();
    assert_eq!(row["deps"].as_array().unwrap().len(), 1);
    assert_eq!(row["deps"][0]["name"], "direct-hosted");
    assert!(CargoPackage::decode(&package.encode()).unwrap() == package);
}
