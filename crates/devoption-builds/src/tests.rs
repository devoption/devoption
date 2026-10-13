use super::*;
use devoption_projects::{CatalogStore, worktree::PermittedRoot};
use devoption_registry::{
    CargoDependencyRequest, CargoMaterialization, CargoOrigin, CargoRegistryProvider,
    LockedPackage, RegistryId, Sha256, prepare_dependencies,
};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

fn digest(byte: u8) -> Sha256 {
    Sha256::from_hex(&format!("{byte:02x}").repeat(32)).unwrap()
}

fn dependencies() -> CargoMaterialization {
    struct FixtureProvider;
    impl CargoRegistryProvider for FixtureProvider {
        fn materialize(
            &self,
            request: &CargoDependencyRequest,
        ) -> Result<CargoMaterialization, devoption_registry::Error> {
            Ok(CargoMaterialization::new(request.clone(), digest(3), 100))
        }
    }
    let request = CargoDependencyRequest::new(
        RegistryId::new("local-registry").unwrap(),
        digest(4),
        digest(2),
        vec![LockedPackage {
            origin: CargoOrigin::CratesIo,
            name: "example-dependency".into(),
            version: "1.0.0".into(),
            archive_sha256: digest(1),
        }],
    )
    .unwrap();
    prepare_dependencies(&FixtureProvider, &request).unwrap()
}

// Synthetic Git metadata only: no Git commands, commits, hooks or build scripts.
// The directories disappear before validation, demonstrating that it reads only
// the supplied snapshot and cannot attest to the current filesystem state.
fn fixture() -> (Catalog, PlanInput) {
    let temporary = tempfile::tempdir().unwrap();
    let base = temporary.path().canonicalize().unwrap();
    let root_path = base.join("repositories");
    let data = base.join("data");
    for path in [&root_path, &data] {
        fs::create_dir(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    for name in ["first", "second"] {
        let git = root_path.join(name).join(".git");
        fs::create_dir_all(git.join("objects")).unwrap();
        fs::create_dir(git.join("refs")).unwrap();
        fs::write(
            git.join("config"),
            "[core]\nrepositoryformatversion = 0\nbare = false\n",
        )
        .unwrap();
        fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    }
    let mut catalog = Catalog::default();
    let project = catalog.create_project("Example").unwrap();
    let other = catalog.create_project("Shared source").unwrap();
    let mut store = CatalogStore::open_absolute(&data).unwrap();
    store.save(&catalog).unwrap();
    let root = PermittedRoot::open_absolute(&root_path).unwrap();
    let repository = store
        .attach_repository(project, &root, Path::new("first"))
        .unwrap();
    store
        .attach_repository(project, &root, Path::new("second"))
        .unwrap();
    store
        .attach_repository(other, &root, Path::new("first"))
        .unwrap();
    let catalog = store.load().unwrap().unwrap();
    let input = PlanInput {
        project,
        repository,
        commit: "0123456789abcdef0123456789abcdef01234567".into(),
        dependencies: dependencies(),
        rust_version: "1.99.0".into(),
        cargo_executable: "/example/toolchains/1.99.0/bin/cargo".into(),
        work_directory: "/example/jobs/one".into(),
        package: "example-app".into(),
        binary: "example-server".into(),
        environment: vec![EnvironmentEntry {
            name: "LANG".into(),
            value: "C.UTF-8".into(),
        }],
        limits: Limits {
            elapsed_seconds: 60,
            log_bytes: 1024,
        },
    };
    (catalog, input)
}

fn rejected(catalog: &Catalog, input: PlanInput, expected: Error) {
    assert_eq!(BuildPlan::validate(input, catalog).err(), Some(expected));
}

#[test]
fn immutable_snapshot_produces_only_fixed_cargo_arguments() {
    let (catalog, mut input) = fixture();
    let before = catalog.clone();
    let plan = BuildPlan::validate(input.clone(), &catalog).unwrap();
    input.package = "edited-after-validation".into();
    input.environment[0].value = "changed".into();
    assert_eq!(plan.details().package, "example-app");
    assert_eq!(plan.details().environment[0].value, "C.UTF-8");
    assert_eq!(
        plan.cargo_arguments(),
        [
            "build",
            "--release",
            "--locked",
            "--offline",
            "--package",
            "example-app",
            "--bin",
            "example-server"
        ]
    );
    assert_eq!(catalog, before);
    let copy = plan.clone();
    assert!(copy == plan);
}

#[test]
fn every_reviewed_field_changes_plan_equality() {
    let (catalog, input) = fixture();
    let original = BuildPlan::validate(input.clone(), &catalog).unwrap();
    let changes: &[fn(&mut PlanInput)] = &[
        |p| p.commit.replace_range(..1, "a"),
        |p| p.rust_version = "1.98.0".into(),
        |p| p.cargo_executable = "/other/bin/cargo".into(),
        |p| p.work_directory = "/example/jobs/two".into(),
        |p| p.package = "other-package".into(),
        |p| p.binary = "other-binary".into(),
        |p| p.environment[0].name = "LC_ALL".into(),
        |p| p.environment[0].value = "C".into(),
        |p| p.environment.clear(),
        |p| p.limits.elapsed_seconds += 1,
        |p| p.limits.log_bytes += 1,
    ];
    for change in changes {
        let mut changed = input.clone();
        change(&mut changed);
        assert!(BuildPlan::validate(changed, &catalog).unwrap() != original);
    }
    let mut changed = input.clone();
    changed.project = catalog.projects()[1].id();
    assert!(BuildPlan::validate(changed, &catalog).unwrap() != original);
    let mut changed = input;
    changed.repository = catalog.repositories()[1].id();
    assert!(BuildPlan::validate(changed, &catalog).unwrap() != original);
}

#[test]
fn dependency_provenance_lockfile_packages_and_bundle_identity_change_the_plan() {
    let (catalog, input) = fixture();
    let original = BuildPlan::validate(input.clone(), &catalog).unwrap();
    let snapshot = input.dependencies.clone();
    let request = snapshot.request();
    let mut requests = vec![
        CargoDependencyRequest::new(
            request.registry().clone(),
            digest(96),
            request.lockfile_sha256(),
            request.packages().to_vec(),
        )
        .unwrap(),
        CargoDependencyRequest::new(
            RegistryId::new("different-provider").unwrap(),
            digest(4),
            request.lockfile_sha256(),
            request.packages().to_vec(),
        )
        .unwrap(),
        CargoDependencyRequest::new(
            request.registry().clone(),
            digest(4),
            digest(99),
            request.packages().to_vec(),
        )
        .unwrap(),
        CargoDependencyRequest::new(
            request.registry().clone(),
            digest(4),
            request.lockfile_sha256(),
            Vec::new(),
        )
        .unwrap(),
    ];
    for change in [
        (|p: &mut LockedPackage| {
            p.origin = CargoOrigin::DevOption(RegistryId::new("private").unwrap())
        }) as fn(&mut LockedPackage),
        |p| p.name = "other-package".into(),
        |p| p.version = "2.0.0".into(),
        |p| p.archive_sha256 = digest(98),
    ] {
        let mut packages = request.packages().to_vec();
        change(&mut packages[0]);
        requests.push(
            CargoDependencyRequest::new(
                request.registry().clone(),
                digest(4),
                request.lockfile_sha256(),
                packages,
            )
            .unwrap(),
        );
    }
    let mut variants: Vec<_> = requests
        .into_iter()
        .map(|r| CargoMaterialization::new(r, snapshot.bundle_sha256(), snapshot.bundle_bytes()))
        .collect();
    variants.push(CargoMaterialization::new(
        request.clone(),
        digest(97),
        snapshot.bundle_bytes(),
    ));
    variants.push(CargoMaterialization::new(
        request.clone(),
        snapshot.bundle_sha256(),
        101,
    ));
    for dependencies in variants {
        let mut changed = input.clone();
        changed.dependencies = dependencies;
        assert!(BuildPlan::validate(changed, &catalog).unwrap() != original);
    }
}

#[test]
fn rejects_unknown_unassociated_and_metadata_only_sources_without_catalog_changes() {
    let (mut catalog, input) = fixture();
    let (other, _) = fixture();
    let mut changed = input.clone();
    changed.project = other.projects()[0].id();
    rejected(&catalog, changed, Error::UnknownProject);
    let mut changed = input.clone();
    changed.repository = other.repositories()[0].id();
    rejected(&catalog, changed, Error::UnknownRepository);
    let mut changed = input.clone();
    changed.project = catalog.projects()[1].id();
    changed.repository = catalog.repositories()[1].id();
    rejected(&catalog, changed, Error::NotAssociated);
    let metadata = catalog.create_repository("Metadata only").unwrap();
    catalog.associate(input.project, metadata).unwrap();
    let before = catalog.clone();
    let mut changed = input;
    changed.repository = metadata;
    rejected(&catalog, changed, Error::NotRegistered);
    assert_eq!(catalog, before);
}

#[test]
fn rejects_refs_abbreviations_null_and_unsupported_commit_formats() {
    let (catalog, input) = fixture();
    for value in [
        "",
        "HEAD",
        "refs/heads/main",
        "0123456",
        &"a".repeat(39),
        &"a".repeat(41),
        &"a".repeat(64),
        &"0".repeat(40),
        &"A".repeat(40),
        &"g".repeat(40),
        &format!("{}\n", "a".repeat(39)),
    ] {
        let mut changed = input.clone();
        changed.commit = value.into();
        rejected(&catalog, changed, Error::InvalidCommit);
    }
}

#[test]
fn rejects_floating_malformed_or_oversized_toolchain_versions() {
    let (catalog, input) = fixture();
    for value in [
        "",
        "stable",
        "nightly-2026-01-01",
        "1.99",
        "1.99.0.1",
        "1.99.0-beta",
        "1.99.0 ",
        "01.99.0",
        "1.099.0",
        "0.99.0",
        "1.+99.0",
        "1.65536.0",
        "1.99.\n",
        "１.99.0",
    ] {
        let mut changed = input.clone();
        changed.rust_version = value.into();
        rejected(&catalog, changed, Error::InvalidRustVersion);
    }
}

#[test]
fn rejects_ambiguous_paths_and_argument_injection() {
    let (catalog, input) = fixture();
    for value in [
        "",
        "/",
        "relative",
        "//jobs",
        "/jobs/",
        "/jobs/../other",
        "/jobs/./one",
        "/jobs//one",
        "/jobs/\0one",
        "/jobs/\none",
        &format!("/{}", "x".repeat(MAX_PATH_BYTES)),
    ] {
        let mut changed = input.clone();
        changed.work_directory = value.into();
        rejected(&catalog, changed, Error::InvalidWorkDirectory);
        let mut changed = input.clone();
        changed.cargo_executable = if value.is_empty() {
            String::new()
        } else {
            format!("{value}/cargo")
        };
        rejected(&catalog, changed, Error::InvalidExecutable);
    }
    for value in ["cargo", "/bin/sh", "/bin/cargo --version"] {
        let mut changed = input.clone();
        changed.cargo_executable = value.into();
        rejected(&catalog, changed, Error::InvalidExecutable);
    }
    for value in [
        "",
        "--help",
        "app --config evil",
        "app;command",
        "app\n",
        "../app",
        "研究",
        &"a".repeat(MAX_TARGET_BYTES + 1),
    ] {
        let mut changed = input.clone();
        changed.package = value.into();
        rejected(&catalog, changed, Error::InvalidTarget);
        let mut changed = input.clone();
        changed.binary = value.into();
        rejected(&catalog, changed, Error::InvalidTarget);
    }
}

#[test]
fn environment_has_count_name_value_and_duplicate_bounds() {
    let (catalog, input) = fixture();
    for name in [
        "",
        "1NAME",
        "A=B",
        "SPACE NAME",
        "LANG\n",
        "研究",
        &"A".repeat(MAX_ENVIRONMENT_NAME_BYTES + 1),
    ] {
        let mut changed = input.clone();
        changed.environment[0].name = name.into();
        rejected(&catalog, changed, Error::InvalidEnvironment);
    }
    for value in [
        "a\0b",
        "a\nb",
        "a\tb",
        "\u{0085}",
        &"a".repeat(MAX_ENVIRONMENT_VALUE_BYTES + 1),
    ] {
        let mut changed = input.clone();
        changed.environment[0].value = value.into();
        rejected(&catalog, changed, Error::InvalidEnvironment);
    }
    let mut duplicate = input.clone();
    duplicate.environment.push(duplicate.environment[0].clone());
    rejected(&catalog, duplicate, Error::InvalidEnvironment);
    let mut changed = input;
    changed.environment = (0..=MAX_ENVIRONMENT_ENTRIES)
        .map(|i| EnvironmentEntry {
            name: format!("VARIABLE_{i}"),
            value: String::new(),
        })
        .collect();
    rejected(&catalog, changed.clone(), Error::InvalidEnvironment);
    changed.environment.pop();
    changed.environment[0].name = "A".repeat(MAX_ENVIRONMENT_NAME_BYTES);
    changed.environment[0].value = "é".repeat(MAX_ENVIRONMENT_VALUE_BYTES / 2);
    assert!(BuildPlan::validate(changed, &catalog).is_ok());
}

#[test]
fn required_limits_and_metadata_boundaries_have_no_execution_defaults() {
    let (catalog, mut input) = fixture();
    let mut zero = input.clone();
    zero.limits.elapsed_seconds = 0;
    rejected(&catalog, zero, Error::InvalidLimits);
    let mut zero = input.clone();
    zero.limits.log_bytes = 0;
    rejected(&catalog, zero, Error::InvalidLimits);
    input.work_directory = format!("/{}", "x".repeat(MAX_PATH_BYTES - 1));
    input.cargo_executable = format!("/{}/cargo", "x".repeat(MAX_PATH_BYTES - 7));
    input.package = "p".repeat(MAX_TARGET_BYTES);
    input.binary = "b".repeat(MAX_TARGET_BYTES);
    input.environment.clear();
    input.limits = Limits {
        elapsed_seconds: u32::MAX,
        log_bytes: u32::MAX,
    };
    assert!(BuildPlan::validate(input, &catalog).is_ok());
}
