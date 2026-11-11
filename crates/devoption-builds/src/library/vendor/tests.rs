use super::*;
use crate::library::{
    publication::{
        dependencies::tests::{grant, public, source},
        tests::{Fixture, OWNER},
    },
    source_tar,
};
use flate2::{Compression, GzBuilder};
use std::io::Write;
const MANIFEST: &[u8] =
    b"[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\nbuild=false\n";
fn package() -> LockedPackage {
    LockedPackage {
        origin: CargoOrigin::CratesIo,
        name: "fixture".into(),
        version: "0.1.0".into(),
        archive_sha256: Sha256::digest(b""),
    }
}
fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut w = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::fast());
    w.write_all(bytes).unwrap();
    w.finish().unwrap()
}
fn tar() -> Vec<u8> {
    source_tar(
        "fixture",
        "0.1.0",
        [
            ("Cargo.toml", MANIFEST),
            ("src/lib.rs", b"pub fn value() {}".as_slice()),
        ],
    )
    .unwrap()
}
fn sum(bytes: &mut [u8]) {
    bytes[148..156].fill(b' ');
    let n: usize = bytes[..512].iter().map(|b| usize::from(*b)).sum();
    bytes[148..156].copy_from_slice(format!("{n:06o}\0 ").as_bytes());
}
fn parse(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    archive::read(bytes, &package(), &mut { MAX_VENDOR_TAR_BYTES })
}
#[test]
fn tar_parser_rejects_paths_types_checksums_padding_and_truncated_or_extra_streams() {
    assert_eq!(parse(&gzip(&tar())).unwrap().len(), 2);
    let mut bad = tar();
    bad[10] ^= 1;
    assert!(parse(&gzip(&bad)).is_err());
    for kind in *b"12xLS6" {
        let mut bad = tar();
        bad[156] = kind;
        sum(&mut bad);
        assert!(parse(&gzip(&bad)).is_err());
    }
    for name in [
        "fixture-0.1.0/../escape",
        "/fixture-0.1.0/absolute",
        "other-0.1.0/Cargo.toml",
        "fixture-0.1.0/.cargo-checksum.json",
        "fixture-0.1.0/.CARGO/config.toml",
        "fixture-0.1.0/src\\escape",
    ] {
        let mut bad = tar();
        bad[..100].fill(0);
        bad[..name.len()].copy_from_slice(name.as_bytes());
        sum(&mut bad);
        assert!(parse(&gzip(&bad)).is_err(), "{name}");
    }
    let mut bad = tar();
    bad[124] = 0x80;
    sum(&mut bad);
    assert!(parse(&gzip(&bad)).is_err());
    let mut bad = tar();
    bad[512 + MANIFEST.len()] = 1;
    assert!(parse(&gzip(&bad)).is_err());
    let mut bad = tar();
    bad.truncate(bad.len() - 512);
    assert!(parse(&gzip(&bad)).is_err());
    let mut bad = gzip(&tar());
    let n = bad.len();
    bad[n - 8] ^= 1;
    assert!(parse(&bad).is_err());
    let mut bad = gzip(&tar());
    bad.extend(gzip(&tar()));
    assert!(parse(&bad).is_err());
    let mut bad = tar();
    bad[345] = b'x';
    bad[257..265].copy_from_slice(b"ustar  \0");
    sum(&mut bad);
    assert!(parse(&gzip(&bad)).is_err());
}
#[test]
fn directory_aliases_and_file_collisions_are_not_extracted_or_normalized() {
    for extra in [
        "Cargo.toml",
        "cargo.toml",
        "SRC/other.rs",
        "src",
        "src/lib.rs/child",
    ] {
        let bad = source_tar(
            "fixture",
            "0.1.0",
            [
                ("Cargo.toml", MANIFEST),
                ("src/lib.rs", b"x".as_slice()),
                (extra, b"y".as_slice()),
            ],
        )
        .unwrap();
        assert!(parse(&gzip(&bad)).is_err(), "{extra}");
    }
    let bomb = gzip(&vec![0; MAX_PACKAGE_TAR_BYTES + 512]);
    assert_eq!(parse(&bomb).err(), Some(Error::Capacity));
    let valid = gzip(&tar());
    let mut remaining = tar().len();
    archive::read(&valid, &package(), &mut remaining).unwrap();
    assert_eq!(remaining, 0);
    assert_eq!(
        archive::read(&valid, &package(), &mut remaining).err(),
        Some(Error::Capacity)
    );
}
#[test]
fn owned_vendor_bytes_checksums_descriptor_and_config_are_exact_and_revalidated() {
    let mut f = Fixture::new();
    grant(&mut f, 0);
    grant(&mut f, 1);
    let source = source(&f, 1, false);
    let captured = f
        .policy
        .locked_archives(
            OWNER,
            f.first,
            &f.catalog,
            &f.candidates,
            &source,
            &[public(&source)],
        )
        .unwrap();
    let inputs = f
        .policy
        .vendor_inputs(OWNER, &f.catalog, &f.candidates, &source, &captured)
        .unwrap();
    assert!(inputs.archive_fingerprint() == captured.fingerprint());
    assert_eq!(&inputs.bundle_bytes()[..9], b"DVVENDOR1");
    assert!(Sha256::digest(inputs.bundle_bytes()) == inputs.materialization().bundle_sha256());
    assert_eq!(
        inputs.bundle_bytes().len() as u64,
        inputs.materialization().bundle_bytes()
    );
    assert!(
        devoption_registry::prepare_dependencies(&inputs, source.request()).unwrap()
            == *inputs.materialization()
    );
    let again = f
        .policy
        .vendor_inputs(OWNER, &f.catalog, &f.candidates, &source, &captured)
        .unwrap();
    assert_eq!(inputs.bundle_bytes(), again.bundle_bytes());
    let wrong = devoption_registry::CargoDependencyRequest::new(
        devoption_registry::RegistryId::new("other").unwrap(),
        Sha256::digest(b"m"),
        Sha256::digest(b"l"),
        vec![],
    )
    .unwrap();
    assert_eq!(
        devoption_registry::prepare_dependencies(&inputs, &wrong).err(),
        Some(devoption_registry::Error::MismatchedRequest)
    );
    // Render-only synthetic alias collision; the verified inputs remain unchanged.
    let mut render_case = again;
    let id = devoption_registry::RegistryId::new("devoption-vendor-0").unwrap();
    render_case.sources.push(VendorSource {
        origin: CargoOrigin::DevOption(id.clone()),
        binding: Some(HostedRegistry::new(id, "sparse+https://other.example.test/index/").unwrap()),
        directory: "extra".into(),
    });
    let _: toml::Table = render_case
        .cargo_config(Path::new("/trusted/vendor"))
        .unwrap()
        .parse()
        .unwrap();

    let files: BTreeMap<_, _> = inputs
        .files()
        .map(|file| {
            assert_eq!(file.mode(), 0o644);
            (file.path().to_owned(), file.bytes().to_vec())
        })
        .collect();
    assert_eq!(
        files
            .keys()
            .filter(|p| p.ends_with("/.cargo-checksum.json"))
            .count(),
        3
    );
    for (path, bytes) in &files {
        if let Some(prefix) = path.strip_suffix("/.cargo-checksum.json") {
            let sum: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            let archive = captured
                .archives()
                .iter()
                .find(|a| {
                    prefix.ends_with(&format!("{}-{}", a.package().name, a.package().version))
                })
                .unwrap();
            assert_eq!(sum["package"], hex(archive.package().archive_sha256));
            for (name, digest) in sum["files"].as_object().unwrap() {
                assert_eq!(
                    digest,
                    &hex(Sha256::digest(&files[&format!("{prefix}/{name}")]))
                )
            }
        }
    }
    let config = inputs.cargo_config(Path::new("/trusted/vendor")).unwrap();
    assert!(config.contains("[source.crates-io]"));
    assert!(config.contains("sparse+https://packages.example.test/index/"));
    assert!(!config.contains("token"));
    assert!(inputs.cargo_config(Path::new("relative")).is_err());
    assert!(
        inputs
            .cargo_config(Path::new("/trusted/../vendor"))
            .is_err()
    );
    inputs
        .revalidate(
            &f.policy,
            OWNER,
            &f.catalog,
            &f.candidates,
            &source,
            &captured,
        )
        .unwrap();
    f.change(
        0,
        super::super::publication::Change::Readers {
            readers: vec![f.second],
        },
    );
    assert_eq!(
        inputs.revalidate(
            &f.policy,
            OWNER,
            &f.catalog,
            &f.candidates,
            &source,
            &captured
        ),
        Err(Error::Denied)
    );
}
#[test]
fn actual_manifest_identity_graph_features_and_executable_profiles_are_checked() {
    let p = package();
    let request = devoption_registry::CargoDependencyRequest::new(
        devoption_registry::RegistryId::new("prepared").unwrap(),
        Sha256::digest(b"m"),
        Sha256::digest(b"l"),
        vec![],
    )
    .unwrap();
    let check = |manifest: &str, edges: &[LockedPackage]| {
        manifest::check(
            &BTreeMap::from([
                ("Cargo.toml".into(), manifest.as_bytes().to_vec()),
                ("src/lib.rs".into(), b"pub fn value() {}".to_vec()),
            ]),
            &p,
            edges,
            &request,
        )
    };
    let base = std::str::from_utf8(MANIFEST).unwrap();
    assert!(check(base, &[]).is_ok());
    assert_eq!(
        check(&base.replace("name='fixture'", "name='other'"), &[]),
        Err(Error::Integrity)
    );
    for extra in [
        "[dependencies]\nx={path='../escape'}\n",
        "[features]\ndefault=['enabled']\n",
        "[lib]\nproc-macro=true\n",
        "[target.'cfg(unix)'.dependencies]\nx='1'\n",
        "[build-dependencies]\nx='1'\n",
        "[patch.crates-io]\nx={path='../escape'}\n",
    ] {
        assert!(check(&format!("{base}{extra}"), &[]).is_err(), "{extra}");
    }
    assert!(check(&base.replace("build=false", "build='build.rs'"), &[]).is_err());
    let child = LockedPackage {
        origin: CargoOrigin::CratesIo,
        name: "child".into(),
        version: "1.2.3".into(),
        archive_sha256: Sha256::digest(b"child"),
    };
    assert!(
        check(
            &format!("{base}[dependencies]\nchild='^1.0'\n"),
            std::slice::from_ref(&child)
        )
        .is_ok()
    );
    assert_eq!(
        check(
            &format!("{base}[dependencies]\nchild='^2.0'\n"),
            std::slice::from_ref(&child)
        ),
        Err(Error::Integrity)
    );
    assert!(
        check(
            &format!("{base}[dependencies]\nalias={{package='child',version='>=1.0, <2'}}\n"),
            std::slice::from_ref(&child)
        )
        .is_ok()
    );
    assert!(
        check(
            &format!("{base}[dependencies]\nchild={{version='1',features=['extra']}}\n"),
            std::slice::from_ref(&child)
        )
        .is_err()
    );
    assert!(
        check(
            &format!("{base}[dependencies]\nchild={{version='1',optional=true}}\n"),
            &[]
        )
        .is_ok()
    );
    assert!(
        check(
            &format!("{base}[dependencies]\nchild={{version='1',optional=true}}\n"),
            std::slice::from_ref(&child)
        )
        .is_err()
    );
}

#[test]
fn package_directory_metadata_has_a_bounded_count() {
    let mut bytes = Vec::new();
    for n in 0..=MAX_PACKAGE_ENTRIES {
        let mut header = tar()[..512].to_vec();
        header[..100].fill(0);
        let name = format!("fixture-0.1.0/d{n}/");
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[156] = b'5';
        header[124..136].copy_from_slice(b"00000000000\0");
        sum(&mut header);
        bytes.extend(header);
    }
    bytes.extend([0; 1024]);
    assert_eq!(parse(&gzip(&bytes)).err(), Some(Error::Capacity));
}
#[test]
fn matching_archive_checksum_does_not_bypass_manifest_or_reserved_metadata_checks() {
    use crate::library::publication::dependencies::PublicArchive;
    use crate::source_snapshot::tests::ProofFixture;
    let f = Fixture::new();
    for (manifest, extra, expected) in [
        (
            std::str::from_utf8(MANIFEST)
                .unwrap()
                .replace("name='fixture'", "name='other'"),
            None,
            Error::Integrity,
        ),
        (
            std::str::from_utf8(MANIFEST)
                .unwrap()
                .replace("build=false", "build=true"),
            None,
            Error::Invalid,
        ),
        (
            std::str::from_utf8(MANIFEST).unwrap().to_owned(),
            Some((".cargo-checksum.json", b"{}".as_slice())),
            Error::Invalid,
        ),
    ] {
        let mut files = vec![
            ("Cargo.toml", manifest.as_bytes()),
            ("src/lib.rs", b"pub fn value() {}".as_slice()),
        ];
        if let Some(extra) = extra {
            files.push(extra);
        }
        let bytes = gzip(&source_tar("fixture", "0.1.0", files).unwrap());
        let lock = format!(
            "version=4\n[[package]]\nname='consumer'\nversion='0.1.0'\ndependencies=['fixture']\n[[package]]\nname='fixture'\nversion='0.1.0'\nsource='registry+https://github.com/rust-lang/crates.io-index'\nchecksum='{}'\n",
            hex(Sha256::digest(&bytes))
        );
        let source=ProofFixture::with_files(&[("Cargo.toml",b"[package]\nname='consumer'\nversion='0.1.0'\nedition='2024'\nbuild=false\n[dependencies]\nfixture='=0.1.0'\n"),("Cargo.lock",lock.as_bytes()),("src/main.rs",b"fn main() {}")]).verify().unwrap();
        let captured = f
            .policy
            .locked_archives(
                OWNER,
                f.first,
                &f.catalog,
                &f.candidates,
                &source,
                &[PublicArchive {
                    package: &source.request().packages()[0],
                    bytes: &bytes,
                }],
            )
            .unwrap();
        assert_eq!(
            f.policy
                .vendor_inputs(OWNER, &f.catalog, &f.candidates, &source, &captured)
                .err(),
            Some(expected)
        );
    }
}
#[test]
fn empty_dependency_set_has_an_exact_nonempty_canonical_bundle() {
    use crate::source_snapshot::tests::ProofFixture;
    let f = Fixture::new();
    let source = ProofFixture::with_files(&[
        (
            "Cargo.toml",
            b"[package]\nname='empty'\nversion='0.1.0'\nedition='2024'\nbuild=false\n",
        ),
        (
            "Cargo.lock",
            b"version=4\n[[package]]\nname='empty'\nversion='0.1.0'\n",
        ),
        ("src/main.rs", b"fn main() {}"),
    ])
    .verify()
    .unwrap();
    let captured = f
        .policy
        .locked_archives(OWNER, f.first, &f.catalog, &f.candidates, &source, &[])
        .unwrap();
    let inputs = f
        .policy
        .vendor_inputs(OWNER, &f.catalog, &f.candidates, &source, &captured)
        .unwrap();
    assert_eq!(inputs.files().count(), 0);
    assert_eq!(inputs.bundle_bytes(), b"DVVENDOR1\0\0\0\0");
    assert_eq!(inputs.materialization().bundle_bytes(), 13);
}

pub(crate) fn attempt_fixture() -> VendorInputs {
    let mut f = Fixture::new();
    grant(&mut f, 0);
    grant(&mut f, 1);
    let source = source(&f, 1, false);
    let captured = f
        .policy
        .locked_archives(
            OWNER,
            f.first,
            &f.catalog,
            &f.candidates,
            &source,
            &[public(&source)],
        )
        .unwrap();
    f.policy
        .vendor_inputs(OWNER, &f.catalog, &f.candidates, &source, &captured)
        .unwrap()
}

#[test]
fn public_static_site_closure_is_pinned_without_relaxing_the_generic_profile() {
    use crate::library::publication::dependencies::PublicArchive;
    use crate::source_snapshot::tests::ProofFixture;
    let archives: [&[u8]; 3] = [
        include_bytes!("../../../tests/fixtures/wasip2-closure/wasip2-1.0.1+wasi-0.2.4.crate"),
        include_bytes!("../../../tests/fixtures/wasip2-closure/wit-bindgen-0.46.0.crate"),
        include_bytes!("../../../tests/fixtures/wasip2-closure/bitflags-2.13.2.crate"),
    ];
    let manifest = include_bytes!("../../../../../examples/static-site/Cargo.toml");
    let lock = include_bytes!("../../../../../examples/static-site/Cargo.lock");
    let source = ProofFixture::with_files(&[
        ("Cargo.toml", manifest),
        ("Cargo.lock", lock),
        ("src/lib.rs", b"pub fn fixture() {}"),
    ])
    .verify()
    .unwrap();
    let graph = devoption_registry::cargo_inputs::inspect(manifest, lock).unwrap();
    let f = Fixture::new();
    let mut public = Vec::new();
    for bytes in archives {
        let package = source
            .request()
            .packages()
            .iter()
            .find(|p| p.archive_sha256 == Sha256::digest(bytes))
            .unwrap();
        let edges = graph
            .package_dependencies()
            .iter()
            .find(|e| e.package() == package)
            .unwrap();
        let files = archive::read(bytes, package, &mut { MAX_VENDOR_TAR_BYTES }).unwrap();
        // Reproduce the restrictive validator's failure on the real upstream
        // manifests; the new exception does not alter that general policy.
        assert!(manifest::check(&files, package, edges.dependencies(), source.request()).is_err());
        assert_eq!(
            wasip2::check(bytes, package, edges.dependencies()),
            Ok(true)
        );
        let mut changed = bytes.to_vec();
        changed[0] ^= 1;
        assert_eq!(
            wasip2::check(&changed, package, edges.dependencies()),
            Err(Error::Integrity)
        );
        let mut identity = package.clone();
        identity.archive_sha256 = Sha256::digest(&changed);
        assert_eq!(
            wasip2::check(&changed, &identity, edges.dependencies()),
            Err(Error::Integrity)
        );
        identity = package.clone();
        identity.origin =
            CargoOrigin::DevOption(devoption_registry::RegistryId::new("team").unwrap());
        assert_eq!(
            wasip2::check(bytes, &identity, edges.dependencies()),
            Ok(false)
        );
        identity = package.clone();
        identity.version = "9.9.9".into();
        assert_eq!(
            wasip2::check(bytes, &identity, edges.dependencies()),
            Ok(false)
        );
        let wrong_edges = if edges.dependencies().is_empty() {
            vec![package.clone()]
        } else {
            vec![]
        };
        assert_eq!(
            wasip2::check(bytes, package, &wrong_edges),
            Err(Error::Invalid)
        );
        public.push(PublicArchive { package, bytes });
    }
    assert_eq!(
        wasip2::context(
            b"[dependencies]\nwit-bindgen='=0.46.0'",
            graph.package_dependencies()
        ),
        Err(Error::Invalid)
    );
    let manifest_text = std::str::from_utf8(manifest).unwrap();
    let lock_text = std::str::from_utf8(lock).unwrap();
    let wrapper_manifest = format!("{manifest_text}\nwrapper='=1.0.0'\n");
    let wrapper_lock = lock_text.replace(" \"wasip2\",", " \"wasip2\",\n \"wrapper\",")
        + "\n[[package]]\nname='wrapper'\nversion='1.0.0'\nsource='registry+https://github.com/rust-lang/crates.io-index'\nchecksum='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\ndependencies=['wit-bindgen']\n";
    let wrapper_graph = devoption_registry::cargo_inputs::inspect(
        wrapper_manifest.as_bytes(),
        wrapper_lock.as_bytes(),
    )
    .unwrap();
    assert_eq!(
        wasip2::context(
            wrapper_manifest.as_bytes(),
            wrapper_graph.package_dependencies()
        ),
        Err(Error::Invalid)
    );
    let captured = f
        .policy
        .locked_archives(OWNER, f.first, &f.catalog, &f.candidates, &source, &public)
        .unwrap();
    let inputs = f
        .policy
        .vendor_inputs(OWNER, &f.catalog, &f.candidates, &source, &captured)
        .unwrap();
    assert_eq!(
        inputs
            .files()
            .filter(|f| f.path().ends_with("/.cargo-checksum.json"))
            .count(),
        3
    );
    assert!(
        inputs
            .files()
            .any(|f| f.path().ends_with("/src/rt/libwit_bindgen_cabi.a"))
    );
}
