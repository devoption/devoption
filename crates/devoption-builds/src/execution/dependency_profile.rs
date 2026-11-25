//! Selection of an already operator-audited lock, never an error fallback.
use crate::source_snapshot::SourceSnapshot;
use devoption_registry::{
    CargoOrigin, Sha256,
    cargo_inputs::{DirectDependency, SourceTarget},
};
use std::io;

pub(super) fn validate(dependencies: &[DirectDependency]) -> io::Result<()> {
    if dependencies.iter().any(|d| {
        d.origin == CargoOrigin::CratesIo
            && d.name == "wit-bindgen"
            && (d.requirement != "=0.46.0" || d.default_features)
    }) {
        return Err(io::Error::other(
            "public wit-bindgen requires the pinned runtime-only profile",
        ));
    }
    // SourceSnapshot admission already refuses every explicit features key,
    // aliases, workspace dependencies, target dependencies and build dependencies.
    Ok(())
}

pub(super) fn matches(source: &SourceSnapshot, lock: Sha256) -> io::Result<bool> {
    validate(source.direct_dependencies())?;
    Ok(matches!(source.target(), SourceTarget::Cdylib { .. })
        && source.request().lockfile_sha256() == lock
        && source
            .request()
            .packages()
            .iter()
            .all(|p| p.origin == CargoOrigin::CratesIo))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_snapshot::tests::ProofFixture;
    const MANIFEST: &str = include_str!("../../tests/fixtures/records-json/Cargo.toml");
    const LOCK: &str = include_str!("../../tests/fixtures/records-json/Cargo.lock");
    fn source(manifest: &str, lock: &str) -> Result<SourceSnapshot, crate::source_snapshot::Error> {
        ProofFixture::with_files(&[
            ("Cargo.toml", manifest.as_bytes()),
            ("Cargo.lock", lock.as_bytes()),
            ("src/lib.rs", b"pub fn fixture() {}"),
        ])
        .verify()
    }
    #[test]
    fn prepared_app_requires_exact_operator_lock_and_runtime_only_bindings() {
        let lock = Sha256::digest(LOCK.as_bytes());
        let valid = source(MANIFEST, LOCK).unwrap();
        assert!(matches(&valid, lock).unwrap());
        let library = source(&MANIFEST.replace("cdylib", "rlib"), LOCK).unwrap();
        assert!(!matches(&library, lock).unwrap());
        assert!(!matches(&valid, Sha256::digest(b"another audited lock")).unwrap());
        let checksum = LOCK.replace("e7e9cc8b1b852640", "a7e9cc8b1b852640");
        assert!(!matches(&source(MANIFEST, &checksum).unwrap(), lock).unwrap());
        let edges = LOCK.replace(" \"serde_derive\",\n", "");
        assert!(source(MANIFEST, &edges).is_err());
        let defaults = MANIFEST.replace(
            "{ version = \"=0.46.0\", default-features = false }",
            "\"=0.46.0\"",
        );
        assert!(matches(&source(&defaults, LOCK).unwrap(), lock).is_err());
        for declaration in [
            "{ version = \"=0.46.0\" }",
            "{ version = \"=0.46.0\", default-features = true }",
            "{ version = \"=0.46.0\", default-features = false, features = [\"macros\"] }",
            "{ version = \"=0.46.0\", default-features = false, features = [\"async\"] }",
            "{ version = \"0.46.0\", default-features = false }",
        ] {
            let manifest = MANIFEST.replace(
                "{ version = \"=0.46.0\", default-features = false }",
                declaration,
            );
            assert!(source(&manifest, LOCK).is_err(), "{declaration}");
        }
        // A valid new public node still changes the approved lock identity.
        let manifest = format!("{MANIFEST}\nunapproved='=1.0.0'\n");
        let lock_extra = LOCK.replace(" \"serde_json\",", " \"unapproved\",\n \"serde_json\",")
            + "\n[[package]]\nname='unapproved'\nversion='1.0.0'\nsource='registry+https://github.com/rust-lang/crates.io-index'\nchecksum='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\n";
        assert!(!matches(&source(&manifest, &lock_extra).unwrap(), lock).unwrap());
    }
    #[test]
    fn static_site_can_use_prepared_lock_but_hosted_nodes_never_can() {
        let manifest = include_str!("../../../../examples/static-site/Cargo.toml");
        let lock = include_str!("../../../../examples/static-site/Cargo.lock");
        assert!(
            matches(
                &source(manifest, lock).unwrap(),
                Sha256::digest(lock.as_bytes())
            )
            .unwrap()
        );
        let manifest = MANIFEST.replace(
            "serde_json = \"=1.0.154\"",
            "serde_json = {version='=1.0.154',registry='team'}",
        );
        let registries = devoption_registry::HostedRegistries::new(vec![
            devoption_registry::HostedRegistry::new(
                devoption_registry::RegistryId::new("team").unwrap(),
                "sparse+https://packages.example.test/index/",
            )
            .unwrap(),
        ])
        .unwrap();
        // Use a single hosted dependency to keep this namespace proof independent
        // of the public fixture's internal dependency origins.
        let manifest = manifest.replace("wasip2 = \"=1.0.1\"", "").replace(
            "wit-bindgen = { version = \"=0.46.0\", default-features = false }",
            "",
        );
        let lock = String::from(
            "version=4\n[[package]]\nname='records-json-fixture'\nversion='0.1.0'\ndependencies=['serde_json']\n[[package]]\nname='serde_json'\nversion='1.0.154'\nsource='sparse+https://packages.example.test/index/'\nchecksum='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\n",
        );
        let snapshot = ProofFixture::with_files(&[
            ("Cargo.toml", manifest.as_bytes()),
            ("Cargo.lock", lock.as_bytes()),
            ("src/lib.rs", b"pub fn fixture() {}"),
        ])
        .verify_with_registries(&registries)
        .unwrap();
        assert!(!matches(&snapshot, Sha256::digest(lock.as_bytes())).unwrap());
    }
}
