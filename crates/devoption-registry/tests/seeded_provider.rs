use devoption_registry::{
    CargoDependencyRequest, CargoOrigin, Error, RegistryId, Sha256, cargo_inputs,
    seeded::{MAX_SEED_BYTES, SeededCargoProvider},
};

const ARCHIVE: &[u8] = include_bytes!("fixtures/registry/itoa-1.0.18.crate");
const MANIFEST: &[u8] = include_bytes!("fixtures/offline-app/Cargo.toml");
const LOCK: &[u8] = include_bytes!("fixtures/offline-app/Cargo.lock");

fn request() -> CargoDependencyRequest {
    cargo_inputs::validate(
        RegistryId::new("fixture-registry").unwrap(),
        MANIFEST,
        LOCK,
        cargo_inputs::InputHashes {
            manifest: Sha256::digest(MANIFEST),
            lockfile: Sha256::digest(LOCK),
        },
    )
    .unwrap()
}

#[test]
fn provider_owns_actual_archive_bytes_bound_to_the_exact_request() {
    let request = request();
    let mut supplied = ARCHIVE.to_vec();
    let provider = SeededCargoProvider::new(request.clone(), &supplied).unwrap();
    supplied.fill(0);
    let bundle = provider.bundle(&request).unwrap();
    assert_eq!(bundle.archive(), ARCHIVE);
    assert!(bundle.materialization().request() == &request);
    assert!(bundle.materialization().bundle_sha256() == Sha256::digest(ARCHIVE));
    assert_eq!(bundle.materialization().bundle_bytes(), 15935);
}

#[test]
fn missing_corrupt_truncated_and_oversized_seeds_are_refused() {
    assert_eq!(
        SeededCargoProvider::new(request(), &[]).err(),
        Some(Error::Unavailable)
    );
    let mut corrupt = ARCHIVE.to_vec();
    let middle = corrupt.len() / 2;
    corrupt[middle] ^= 1;
    for bad in [&corrupt[..], &ARCHIVE[..ARCHIVE.len() - 1]] {
        assert_eq!(
            SeededCargoProvider::new(request(), bad).err(),
            Some(Error::Integrity)
        );
    }
    assert_eq!(
        SeededCargoProvider::new(request(), &vec![0; MAX_SEED_BYTES + 1]).err(),
        Some(Error::SeedTooLarge)
    );
}

#[test]
fn changed_request_identity_cannot_reuse_a_verified_bundle() {
    let expected = request();
    let provider = SeededCargoProvider::new(expected.clone(), ARCHIVE).unwrap();
    for changed in [
        CargoDependencyRequest::new(
            RegistryId::new("other").unwrap(),
            expected.manifest_sha256(),
            expected.lockfile_sha256(),
            expected.packages().to_vec(),
        )
        .unwrap(),
        CargoDependencyRequest::new(
            expected.registry().clone(),
            Sha256::digest(b"different manifest"),
            expected.lockfile_sha256(),
            expected.packages().to_vec(),
        )
        .unwrap(),
        CargoDependencyRequest::new(
            expected.registry().clone(),
            expected.manifest_sha256(),
            Sha256::digest(b"different lock"),
            expected.packages().to_vec(),
        )
        .unwrap(),
    ] {
        assert_eq!(
            provider.bundle(&changed).err(),
            Some(Error::MismatchedRequest)
        );
    }
}

#[test]
fn seed_profile_refuses_empty_multiple_and_private_packages() {
    let expected = request();
    let mut private = expected.packages()[0].clone();
    private.origin = CargoOrigin::DevOption(RegistryId::new("private").unwrap());
    let mut second = expected.packages()[0].clone();
    second.name = "another-package".into();
    for packages in [
        vec![],
        vec![private],
        vec![expected.packages()[0].clone(), second],
    ] {
        let unsupported = CargoDependencyRequest::new(
            expected.registry().clone(),
            expected.manifest_sha256(),
            expected.lockfile_sha256(),
            packages,
        )
        .unwrap();
        assert_eq!(
            SeededCargoProvider::new(unsupported, ARCHIVE).err(),
            Some(Error::UnsupportedSeed)
        );
    }
}
