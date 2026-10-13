use super::*;
use std::cell::Cell;

fn digest(byte: u8) -> Sha256 {
    Sha256::from_hex(&format!("{byte:02x}").repeat(32)).unwrap()
}

fn registry(name: &str) -> RegistryId {
    RegistryId::new(name).unwrap()
}

fn package(name: &str) -> LockedPackage {
    LockedPackage {
        origin: CargoOrigin::CratesIo,
        name: name.into(),
        version: "1.2.3".into(),
        archive_sha256: digest(1),
    }
}

fn request(packages: Vec<LockedPackage>) -> CargoDependencyRequest {
    CargoDependencyRequest::new(registry("local-registry"), digest(4), digest(2), packages).unwrap()
}

struct FixtureProvider {
    response: Result<CargoMaterialization, Error>,
    calls: Cell<usize>,
}

impl CargoRegistryProvider for FixtureProvider {
    fn materialize(&self, _: &CargoDependencyRequest) -> Result<CargoMaterialization, Error> {
        self.calls.set(self.calls.get() + 1);
        self.response.clone()
    }
}

#[test]
fn requests_own_their_metadata_and_order_does_not_change_identity() {
    let mut packages = vec![package("z-last"), package("a-first")];
    let original = request(packages.clone());
    packages.reverse();
    assert!(original == request(packages.clone()));
    packages[0].name = "edited".into();
    packages[0].archive_sha256 = digest(3);
    assert_eq!(original.packages()[0].name, "a-first");
    assert!(original.packages()[0].archive_sha256 == digest(1));
    assert_eq!(original.registry().as_str(), "local-registry");
    assert!(original.lockfile_sha256() == digest(2));
    assert!(original.manifest_sha256() == digest(4));
    assert!(request(Vec::new()).packages().is_empty());
}

#[test]
fn checksums_require_complete_canonical_hex_without_claiming_byte_verification() {
    let parsed = Sha256::from_hex(&"0123456789abcdef".repeat(4)).unwrap();
    assert_eq!(&parsed.bytes()[..8], &[1, 35, 69, 103, 137, 171, 205, 239]);
    for value in [
        "",
        &"a".repeat(63),
        &"a".repeat(65),
        &"A".repeat(64),
        &"g".repeat(64),
        &"é".repeat(32),
        &format!("{}\n", "a".repeat(63)),
    ] {
        assert_eq!(Sha256::from_hex(value).err(), Some(Error::InvalidChecksum));
    }
}

#[test]
fn logical_registry_and_package_names_are_bounded_and_cannot_be_locations() {
    for name in [
        "",
        "1name",
        "https://example.invalid",
        "/tmp/name",
        "../name",
        "a b",
        "a\nb",
        "研究",
        &"a".repeat(65),
    ] {
        assert_eq!(RegistryId::new(name).err(), Some(Error::InvalidRegistry));
        let mut p = package("valid");
        p.name = name.into();
        assert_eq!(
            CargoDependencyRequest::new(registry("local"), digest(4), digest(1), vec![p]).err(),
            Some(Error::InvalidPackageName)
        );
    }
    assert!(RegistryId::new(&"a".repeat(MAX_REGISTRY_ID_BYTES)).is_ok());
    assert_eq!(RegistryId::new("UPPER").err(), Some(Error::InvalidRegistry));
    assert_eq!(
        request(vec![package(&"a".repeat(MAX_PACKAGE_NAME_BYTES))])
            .packages()
            .len(),
        1
    );
}

#[test]
fn cargo_names_preserve_case_but_reject_case_colliding_coordinates() {
    let input = request(vec![package("Example"), package("Another_Name")]);
    assert_eq!(input.packages()[0].name, "Another_Name");
    assert_eq!(input.packages()[1].name, "Example");
    assert_eq!(
        CargoDependencyRequest::new(
            registry("local"),
            digest(4),
            digest(1),
            vec![package("Example"), package("example")]
        )
        .err(),
        Some(Error::DuplicatePackage)
    );
    // A provider cannot change the exact spelling from the claimed lockfile.
    let expected = request(vec![package("Example")]);
    let provider = FixtureProvider {
        response: Ok(CargoMaterialization::new(
            request(vec![package("example")]),
            digest(3),
            100,
        )),
        calls: Cell::new(0),
    };
    assert_eq!(
        prepare_dependencies(&provider, &expected).err(),
        Some(Error::MismatchedRequest)
    );
}

#[test]
fn versions_are_exact_including_prerelease_and_build_metadata() {
    for version in [
        "0.0.0",
        "1.2.3-alpha.1",
        "1.2.3+build.001",
        "1.2.3-rc.1+build-2",
        "18446744073709551615.0.0",
        &format!("1.0.0+{}", "a".repeat(MAX_VERSION_BYTES - 6)),
    ] {
        let mut p = package("valid");
        p.version = version.into();
        assert_eq!(request(vec![p]).packages()[0].version, version);
    }
    for version in [
        "",
        "*",
        "^1.2.3",
        ">=1.2.3",
        "1.2",
        "1.2.3.4",
        "01.2.3",
        "1.2.3-01",
        "1.2.3-",
        "1.2.3+",
        "1.2.3-a..b",
        "1.2.3+a+b",
        "1.2.3 ",
        "1.2.3\n",
        "1.2.3-研究",
        "18446744073709551616.0.0",
        &format!("1.0.0+{}", "a".repeat(MAX_VERSION_BYTES - 5)),
    ] {
        let mut p = package("valid");
        p.version = version.into();
        assert_eq!(
            CargoDependencyRequest::new(registry("local"), digest(4), digest(1), vec![p]).err(),
            Some(Error::InvalidVersion)
        );
    }
}

#[test]
fn original_registry_identity_prevents_private_public_package_confusion() {
    let public = package("same-name");
    let mut hosted = public.clone();
    hosted.origin = CargoOrigin::DevOption(registry("team-one"));
    hosted.archive_sha256 = digest(8);
    let mut other_hosted = hosted.clone();
    other_hosted.origin = CargoOrigin::DevOption(registry("team-two"));
    let mut other_version = public.clone();
    other_version.version = "2.0.0".into();
    assert_eq!(
        request(vec![public.clone(), hosted, other_hosted, other_version])
            .packages()
            .len(),
        4
    );
    for variant in [
        public.clone(),
        LockedPackage {
            archive_sha256: digest(9),
            ..public.clone()
        },
        LockedPackage {
            version: "1.2.3+different-build".into(),
            ..public.clone()
        },
    ] {
        assert_eq!(
            CargoDependencyRequest::new(
                registry("local"),
                digest(4),
                digest(1),
                vec![public.clone(), variant]
            )
            .err(),
            Some(Error::DuplicatePackage)
        );
    }
}

#[test]
fn package_capacity_is_a_metadata_bound() {
    let mut packages: Vec<_> = (0..MAX_PACKAGES)
        .map(|i| package(&format!("package-{i}")))
        .collect();
    assert_eq!(request(packages.clone()).packages().len(), MAX_PACKAGES);
    packages.push(package("overflow"));
    assert_eq!(
        CargoDependencyRequest::new(registry("local"), digest(4), digest(1), packages).err(),
        Some(Error::TooManyPackages)
    );
}

#[test]
fn provider_must_return_the_exact_request_with_no_origin_or_checksum_substitution() {
    let expected = request(vec![package("expected")]);
    let changed = |p| request(vec![p]);
    let variants = [
        CargoDependencyRequest::new(
            expected.registry().clone(),
            digest(98),
            expected.lockfile_sha256(),
            expected.packages().to_vec(),
        )
        .unwrap(),
        CargoDependencyRequest::new(
            registry("other-provider"),
            digest(4),
            expected.lockfile_sha256(),
            expected.packages().to_vec(),
        )
        .unwrap(),
        CargoDependencyRequest::new(
            expected.registry().clone(),
            digest(4),
            digest(99),
            expected.packages().to_vec(),
        )
        .unwrap(),
        request(Vec::new()),
        request(vec![package("expected"), package("additional")]),
        changed(package("replacement")),
        changed(LockedPackage {
            version: "2.0.0".into(),
            ..package("expected")
        }),
        changed(LockedPackage {
            archive_sha256: digest(99),
            ..package("expected")
        }),
        changed(LockedPackage {
            origin: CargoOrigin::DevOption(registry("private")),
            ..package("expected")
        }),
    ];
    for returned in variants {
        let provider = FixtureProvider {
            response: Ok(CargoMaterialization::new(returned, digest(3), 100)),
            calls: Cell::new(0),
        };
        assert_eq!(
            prepare_dependencies(&provider, &expected).err(),
            Some(Error::MismatchedRequest)
        );
        assert_eq!(provider.calls.get(), 1);
    }
    let materialization = CargoMaterialization::new(expected.clone(), digest(3), 100);
    let provider = FixtureProvider {
        response: Ok(materialization.clone()),
        calls: Cell::new(0),
    };
    assert!(prepare_dependencies(&provider, &expected).unwrap() == materialization);
    assert_eq!(provider.calls.get(), 1);
}

#[test]
fn unavailable_denied_and_integrity_failures_propagate_without_retry_or_fallback() {
    for error in [Error::Unavailable, Error::Denied, Error::Integrity] {
        let provider = FixtureProvider {
            response: Err(error),
            calls: Cell::new(0),
        };
        assert_eq!(
            prepare_dependencies(&provider, &request(vec![package("example")])).err(),
            Some(error)
        );
        assert_eq!(provider.calls.get(), 1);
    }
}

#[test]
fn materialization_identity_includes_provenance_digest_and_claimed_size() {
    let input = request(vec![package("example")]);
    let original = CargoMaterialization::new(input.clone(), digest(3), 100);
    assert!(original == original.clone());
    assert!(original.bundle_sha256() == digest(3));
    assert_eq!(original.bundle_bytes(), 100);
    assert!(original != CargoMaterialization::new(input.clone(), digest(4), 100));
    assert!(original != CargoMaterialization::new(input, digest(3), 101));
    assert!(original != CargoMaterialization::new(request(Vec::new()), digest(3), 100));
    // Zero and large sizes are claims, not an applied storage policy.
    assert_eq!(
        CargoMaterialization::new(request(Vec::new()), digest(3), 0).bundle_bytes(),
        0
    );
}
