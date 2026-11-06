use super::*;
use crate::{HostedRegistries, HostedRegistry};
const INDEX: &str = "sparse+https://packages.example.test/index/";
const PUBLIC: &str = "registry+https://github.com/rust-lang/crates.io-index";

fn bindings() -> HostedRegistries {
    HostedRegistries::new(vec![
        HostedRegistry::new(RegistryId::new("team").unwrap(), INDEX).unwrap(),
    ])
    .unwrap()
}
fn manifest() -> String {
    "[package]\nname='consumer'\nversion='0.1.0'\nedition='2024'\nbuild=false\n[dependencies]\nhelper={version='=0.1.0',registry='team'}\nshared='=1.0.0'\n".into()
}
fn locked() -> String {
    format!(
        r#"version=4
[[package]]
name='consumer'
version='0.1.0'
dependencies=['helper', 'shared 1.0.0 ({PUBLIC})']
[[package]]
name='helper'
version='0.1.0'
source='{INDEX}'
checksum='{}'
dependencies=['shared 1.0.0 ({INDEX})']
[[package]]
name='shared'
version='1.0.0'
source='{PUBLIC}'
checksum='{}'
[[package]]
name='shared'
version='1.0.0'
source='{INDEX}'
checksum='{}'
"#,
        "1".repeat(64),
        "2".repeat(64),
        "3".repeat(64)
    )
}
fn check(
    manifest: &str,
    lock: &str,
    bindings: &HostedRegistries,
) -> Result<CargoDependencyRequest, Error> {
    validate_with_registries(
        RegistryId::new("provider").unwrap(),
        manifest.as_bytes(),
        lock.as_bytes(),
        InputHashes {
            manifest: Sha256::digest(manifest.as_bytes()),
            lockfile: Sha256::digest(lock.as_bytes()),
        },
        bindings,
    )
}
#[test]
fn identical_coordinates_in_distinct_origins_resolve_only_by_exact_source() {
    let request = check(&manifest(), &locked(), &bindings()).unwrap();
    assert_eq!(request.packages().len(), 3);
    let shared: Vec<_> = request
        .packages()
        .iter()
        .filter(|p| p.name == "shared")
        .collect();
    assert_eq!(shared.len(), 2);
    assert!(shared[0].origin != shared[1].origin);
    assert!(shared[0].archive_sha256 != shared[1].archive_sha256);
    assert!(request.hosted_registries() == &bindings());
    for reference in ["shared", "shared 1.0.0"] {
        assert_eq!(
            check(
                &manifest(),
                &locked().replace(&format!("shared 1.0.0 ({INDEX})"), reference),
                &bindings()
            )
            .err(),
            Some(Error::AmbiguousDependency)
        );
    }
    // A known but wrong namespace is not a substitute for the direct pin.
    let swapped = locked().replace(
        &format!("dependencies=['helper', 'shared 1.0.0 ({PUBLIC})']"),
        &format!("dependencies=['helper', 'shared 1.0.0 ({INDEX})']"),
    );
    assert_eq!(
        check(&manifest(), &swapped, &bindings()).err(),
        Some(Error::DependencyMismatch)
    );
}
#[test]
fn empty_or_changed_binding_sets_never_infer_sources_from_repository_bytes() {
    assert_eq!(
        inspect(manifest().as_bytes(), locked().as_bytes()).err(),
        Some(Error::Unsupported)
    );
    assert_eq!(
        check(&manifest(), &locked(), &HostedRegistries::default()).err(),
        Some(Error::Unsupported)
    );
    let renamed = HostedRegistries::new(vec![
        HostedRegistry::new(RegistryId::new("other").unwrap(), INDEX).unwrap(),
    ])
    .unwrap();
    assert!(check(&manifest(), &locked(), &renamed).is_err());
    let rebound = HostedRegistries::new(vec![
        HostedRegistry::new(
            RegistryId::new("team").unwrap(),
            "sparse+https://other.example.test/index/",
        )
        .unwrap(),
    ])
    .unwrap();
    assert!(check(&manifest(), &locked(), &rebound).is_err());
    let ordinary = check(&manifest(), &locked(), &bindings()).unwrap();
    let extra = HostedRegistries::new(vec![
        bindings().bindings()[0].clone(),
        HostedRegistry::new(
            RegistryId::new("extra").unwrap(),
            "https://extra.example.test/index",
        )
        .unwrap(),
    ])
    .unwrap();
    let changed = check(&manifest(), &locked(), &extra).unwrap();
    assert!(ordinary != changed);
    assert!(ordinary.packages() == changed.packages());
    assert!(ordinary.manifest_sha256() == changed.manifest_sha256());
}
#[test]
fn hosted_dependency_tables_preserve_exact_pin_and_feature_restrictions() {
    let original = manifest();
    let disabled = original.replace(
        "version='=0.1.0',registry='team'",
        "version='=0.1.0',registry='team',default-features=false",
    );
    assert!(check(&disabled, &locked(), &bindings()).is_ok());
    for fields in [
        "version='0.1.0',registry='team'",
        "version='=0.1.0',registry='unknown'",
        "version='=0.1.0',registry=42",
        "version='=0.1.0',registry='team',default-features=true",
        "version='=0.1.0',registry='team',features=['custom']",
        "version='=0.1.0',registry='team',optional=true",
        "version='=0.1.0',registry='team',path='../helper'",
        "version='=0.1.0',registry='team',git='https://other.example.test/source'",
        "version='=0.1.0',registry='team',package='renamed'",
    ] {
        assert!(
            check(
                &original.replace("version='=0.1.0',registry='team'", fields),
                &locked(),
                &bindings()
            )
            .is_err(),
            "{fields}"
        );
    }
    assert_eq!(
        check(
            &original.replace("=0.1.0", "=0.2.0"),
            &locked(),
            &bindings()
        )
        .err(),
        Some(Error::DependencyMismatch)
    );
    let private_direct = original.replace(
        "shared='=1.0.0'",
        "shared={version='=1.0.0',registry='team'}",
    );
    assert_eq!(
        check(&private_direct, &locked(), &bindings()).err(),
        Some(Error::DependencyMismatch)
    );
}
#[test]
fn qualifiers_and_graph_checks_remain_closed_and_bounded() {
    for reference in [
        "shared 1.0.0 (sparse+https://unknown.example.test/index/)",
        "shared 1.0.0 (sparse+https://packages.example.test/index/)",
        "shared 1.0.0 sparse+https://packages.example.test/index/",
        "shared 1.0.0 ()",
        "shared 1.0.0 (extra source)",
    ] {
        let mut lock = locked();
        if reference == format!("shared 1.0.0 ({INDEX})") {
            lock = lock.replace("version='1.0.0'\nsource=", "version='1.0.1'\nsource=");
        }
        lock = lock.replace(&format!("shared 1.0.0 ({INDEX})"), reference);
        assert!(
            check(&manifest(), &lock, &bindings()).is_err(),
            "{reference}"
        );
    }
    let duplicate = locked().replace(
        &format!("dependencies=['shared 1.0.0 ({INDEX})']"),
        &format!("dependencies=['shared 1.0.0 ({INDEX})','shared 1.0.0 ({INDEX})']"),
    );
    assert_eq!(
        check(&manifest(), &duplicate, &bindings()).err(),
        Some(Error::InvalidGraph)
    );
    let disconnected = locked().replace(
        &format!("dependencies=['shared 1.0.0 ({INDEX})']"),
        "dependencies=[]",
    );
    assert_eq!(
        check(&manifest(), &disconnected, &bindings()).err(),
        Some(Error::InvalidGraph)
    );
    let hash = InputHashes {
        manifest: Sha256::digest(manifest().as_bytes()),
        lockfile: Sha256::digest(locked().as_bytes()),
    };
    let changed = manifest() + "\n# different reviewed bytes\n";
    assert_eq!(
        validate_with_registries(
            RegistryId::new("provider").unwrap(),
            changed.as_bytes(),
            locked().as_bytes(),
            hash,
            &bindings()
        )
        .err(),
        Some(Error::HashMismatch)
    );
}

#[test]
fn provider_cannot_reuse_materialization_for_a_changed_registry_binding_set() {
    use crate::{CargoMaterialization, CargoRegistryProvider, prepare_dependencies};
    struct Provider(CargoMaterialization);
    impl CargoRegistryProvider for Provider {
        fn materialize(
            &self,
            _: &CargoDependencyRequest,
        ) -> Result<CargoMaterialization, crate::Error> {
            Ok(self.0.clone())
        }
    }
    let original = check(&manifest(), &locked(), &bindings()).unwrap();
    let extra = HostedRegistries::new(vec![
        bindings().bindings()[0].clone(),
        HostedRegistry::new(
            RegistryId::new("extra").unwrap(),
            "https://extra.example.test/index",
        )
        .unwrap(),
    ])
    .unwrap();
    let changed = check(&manifest(), &locked(), &extra).unwrap();
    let provider = Provider(CargoMaterialization::new(
        original.clone(),
        Sha256::digest(b"verified bundle fixture"),
        23,
    ));
    assert!(prepare_dependencies(&provider, &original).is_ok());
    assert_eq!(
        prepare_dependencies(&provider, &changed).err(),
        Some(crate::Error::MismatchedRequest)
    );
}
