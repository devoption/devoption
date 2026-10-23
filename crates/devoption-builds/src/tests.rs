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

fn dependencies(source: &source_snapshot::SourceSnapshot) -> CargoMaterialization {
    struct FixtureProvider;
    impl CargoRegistryProvider for FixtureProvider {
        fn materialize(
            &self,
            request: &CargoDependencyRequest,
        ) -> Result<CargoMaterialization, devoption_registry::Error> {
            Ok(CargoMaterialization::new(request.clone(), digest(3), 100))
        }
    }
    prepare_dependencies(&FixtureProvider, source.request()).unwrap()
}

// Complete synthetic source objects, without repository reads or execution.
fn source(changed_commit: bool) -> source_snapshot::SourceSnapshot {
    let mut fixture = source_snapshot::tests::ProofFixture::new();
    if changed_commit {
        fixture
            .commit
            .extend_from_slice(b"Another fixture revision\n");
    }
    fixture.verify().unwrap()
}

pub(crate) fn change_source_revision(input: &mut PlanInput) {
    input.source = source(true);
    input.commit = input.source.commit().to_owned();
}

pub(crate) fn change_source_bytes(input: &mut PlanInput) {
    input.source = source_snapshot::tests::ProofFixture::changed_source()
        .verify()
        .unwrap();
    input.commit = input.source.commit().into();
}

#[test]
fn exact_source_target_is_required_and_changed_bytes_change_the_plan() {
    let (catalog, input) = fixture();
    for field in ["package", "binary"] {
        let mut changed = input.clone();
        if field == "package" {
            changed.package = "different".into();
        } else {
            changed.binary = "different".into();
        }
        rejected(&catalog, changed, Error::TargetMismatch);
    }
    let original = BuildPlan::validate(input.clone(), &catalog).unwrap();
    let mut changed = input;
    change_source_bytes(&mut changed);
    let updated = BuildPlan::validate(changed, &catalog).unwrap();
    assert!(original != updated);
    assert!(original.details().source.sha256() != updated.details().source.sha256());
}

// Synthetic Git metadata only: no Git commands, commits, hooks or build scripts.
// The directories disappear before validation, demonstrating that it reads only
// the supplied snapshot and cannot attest to the current filesystem state.
pub(crate) fn fixture() -> (Catalog, PlanInput) {
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
    let source = source(false);
    let input = PlanInput {
        project,
        repository,
        commit: source.commit().to_owned(),
        dependencies: dependencies(&source),
        source,
        rust_version: "1.99.0".into(),
        cargo_executable: "/example/toolchains/1.99.0/bin/cargo".into(),
        work_directory: "/example/jobs/one".into(),
        package: "ExampleService".into(),
        binary: "ExampleService".into(),
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
    assert_eq!(plan.details().package, "ExampleService");
    assert_eq!(plan.details().environment[0].value, "C.UTF-8");
    assert_eq!(
        plan.cargo_arguments(),
        [
            "build",
            "--release",
            "--locked",
            "--offline",
            "--package",
            "ExampleService",
            "--bin",
            "ExampleService"
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
        change_source_revision,
        |p| p.rust_version = "1.98.0".into(),
        |p| p.cargo_executable = "/other/bin/cargo".into(),
        |p| p.work_directory = "/example/jobs/two".into(),
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
fn source_binding_rejects_mismatched_request_and_retains_bundle_identity() {
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
            request.manifest_sha256(),
            request.lockfile_sha256(),
            request.packages().to_vec(),
        )
        .unwrap(),
        CargoDependencyRequest::new(
            request.registry().clone(),
            request.manifest_sha256(),
            digest(99),
            request.packages().to_vec(),
        )
        .unwrap(),
        CargoDependencyRequest::new(
            request.registry().clone(),
            request.manifest_sha256(),
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
                request.manifest_sha256(),
                request.lockfile_sha256(),
                packages,
            )
            .unwrap(),
        );
    }
    for request in requests {
        let mut changed = input.clone();
        changed.dependencies =
            CargoMaterialization::new(request, snapshot.bundle_sha256(), snapshot.bundle_bytes());
        rejected(&catalog, changed, Error::SourceMismatch);
    }
    for dependencies in [
        CargoMaterialization::new(request.clone(), digest(97), snapshot.bundle_bytes()),
        CargoMaterialization::new(request.clone(), snapshot.bundle_sha256(), 101),
    ] {
        let mut changed = input.clone();
        changed.dependencies = dependencies;
        assert!(BuildPlan::validate(changed, &catalog).unwrap() != original);
    }
}

#[test]
fn selected_commit_and_proof_cannot_be_changed_independently() {
    let (catalog, input) = fixture();
    let before = catalog.clone();
    let original = BuildPlan::validate(input.clone(), &catalog).unwrap();
    let mut changed = input.clone();
    changed.commit = source(true).commit().to_owned();
    rejected(&catalog, changed, Error::SourceMismatch);
    let mut changed = input.clone();
    changed.source = source(true);
    rejected(&catalog, changed, Error::SourceMismatch);
    let mut changed = input.clone();
    change_source_revision(&mut changed);
    let rebound = BuildPlan::validate(changed, &catalog).unwrap();
    assert!(rebound != original);
    assert!(original.details().source == input.source);
    assert!(rebound.details().source.commit_sha256() != original.details().source.commit_sha256());
    assert!(rebound.details().source.tree_sha256() == original.details().source.tree_sha256());
    assert_eq!(catalog, before);
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
    input.environment.clear();
    input.limits = Limits {
        elapsed_seconds: u32::MAX,
        log_bytes: u32::MAX,
    };
    assert!(BuildPlan::validate(input, &catalog).is_ok());
}

#[test]
fn cdylib_snapshot_cannot_enter_the_native_plan_or_approval_path() {
    let (catalog, mut input) = fixture();
    input.source = source_snapshot::tests::ProofFixture::cdylib()
        .verify()
        .unwrap();
    input.commit = input.source.commit().into();
    input.dependencies = dependencies(&input.source);
    input.package = input.source.package().into();
    input.binary = input.source.package().into();
    let plan = BuildPlan::validate(input, &catalog).unwrap();
    assert!(plan.is_wasm());
    assert!(matches!(
        crate::review_context::ReviewContext::validate(
            &plan,
            crate::review_context::tests::input(&plan)
        ),
        Err(crate::review_context::Error::Policy)
    ));
}

#[test]
fn selected_web_app_plan_uses_only_the_pinned_wasip2_target() {
    let (catalog, mut input) = fixture();
    input.source = source_snapshot::tests::ProofFixture::with_files(&[
        (
            "Cargo.toml",
            include_bytes!("../../../examples/web-app/Cargo.toml"),
        ),
        (
            "Cargo.lock",
            include_bytes!("../../../examples/web-app/Cargo.lock"),
        ),
        (
            "src/lib.rs",
            include_bytes!("../../../examples/web-app/src/lib.rs"),
        ),
    ])
    .verify()
    .unwrap();
    input.commit = input.source.commit().into();
    input.dependencies = dependencies(&input.source);
    input.package = "devoption-web-fixture".into();
    input.binary = "devoption_web_fixture".into();
    input.rust_version = "1.99.0".into();
    let plan = BuildPlan::validate(input.clone(), &catalog).unwrap();
    assert!(plan.is_wasm());
    assert_eq!(
        plan.artifact_path(),
        "wasm32-wasip2/release/devoption_web_fixture.wasm"
    );
    assert_eq!(
        plan.cargo_arguments(),
        [
            "build",
            "--release",
            "--locked",
            "--offline",
            "--target",
            "wasm32-wasip2",
            "--package",
            "devoption-web-fixture",
            "--lib"
        ]
    );
    input.rust_version = "1.98.0".into();
    assert!(BuildPlan::validate(input, &catalog).is_err());
}

fn static_site_input(name: &str, css: &[u8]) -> (Catalog, PlanInput) {
    let (catalog, mut input) = fixture();
    let manifest = include_str!("../../../examples/static-site/Cargo.toml")
        .replace("sample-site", name)
        .replace("sample_site", &name.replace('-', "_"));
    let lock =
        include_str!("../../../examples/static-site/Cargo.lock").replace("sample-site", name);
    input.source = source_snapshot::tests::ProofFixture::with_files(&[
        ("Cargo.toml", manifest.as_bytes()),
        ("Cargo.lock", lock.as_bytes()),
        (
            "src/lib.rs",
            include_bytes!("../../../examples/static-site/src/lib.rs"),
        ),
        (
            "assets/index.html",
            include_bytes!("../../../examples/static-site/assets/index.html"),
        ),
        ("assets/site.css", css),
        (
            "assets/site.js",
            include_bytes!("../../../examples/static-site/assets/site.js"),
        ),
    ])
    .verify()
    .unwrap();
    input.commit = input.source.commit().into();
    input.dependencies = dependencies(&input.source);
    input.package = input.source.package().into();
    input.binary = name.replace('-', "_");
    input.rust_version = "1.99.0".into();
    (catalog, input)
}

#[test]
fn named_wasm_app_binds_library_command_artifact_and_all_asset_bytes() {
    let css = include_bytes!("../../../examples/static-site/assets/site.css");
    let (catalog, input) = static_site_input("sample-site", css);
    let plan = BuildPlan::validate(input.clone(), &catalog).unwrap();
    assert_eq!(
        plan.artifact_path(),
        "wasm32-wasip2/release/sample_site.wasm"
    );
    assert_eq!(
        plan.cargo_arguments(),
        [
            "build",
            "--release",
            "--locked",
            "--offline",
            "--target",
            "wasm32-wasip2",
            "--package",
            "sample-site",
            "--lib"
        ]
    );
    assert_eq!(plan.details().source.files().len(), 6);
    for (package, binary) in [
        ("devoption-web-fixture", "sample_site"),
        ("sample-site", "devoption_web_fixture"),
        ("../other", "sample_site"),
        ("sample-site", "../other"),
    ] {
        let mut changed = input.clone();
        changed.package = package.into();
        changed.binary = binary.into();
        assert!(BuildPlan::validate(changed, &catalog).is_err());
    }
    let (_, mut changed) = static_site_input("sample-site", b"body { color: navy; }\n");
    changed.project = input.project;
    changed.repository = input.repository;
    let changed = BuildPlan::validate(changed, &catalog).unwrap();
    assert!(plan != changed);
    assert!(plan.details().source.sha256() != changed.details().source.sha256());
}

#[test]
fn named_wasm_approval_is_invalidated_by_a_rename_or_asset_edit() {
    use crate::{
        approvals::{Approvals, OwnerId},
        jobs::{BuildJob, JobId},
        review_context::{Policy, ReviewContext},
    };
    let environment = [
        ("PATH", "/toolchain/bin:/usr/bin"),
        ("RUSTC", "/toolchain/bin/rustc"),
        ("HOME", "/job/home"),
        ("CARGO_HOME", "/job/cargo"),
        ("CARGO_TARGET_DIR", "/job/target"),
        ("TMPDIR", "/job/tmp"),
        ("LANG", "C.UTF-8"),
        ("CARGO_NET_OFFLINE", "true"),
        ("CARGO_INCREMENTAL", "0"),
        ("CARGO_BUILD_JOBS", "1"),
        ("RUSTC_WRAPPER", ""),
        ("RUSTC_WORKSPACE_WRAPPER", ""),
    ];
    let make = |name, css: &[u8]| {
        let (catalog, mut input) = static_site_input(name, css);
        input.cargo_executable = "/toolchain/bin/cargo".into();
        input.limits.log_bytes = 1_048_576;
        input.environment = environment
            .iter()
            .map(|(name, value)| EnvironmentEntry {
                name: (*name).into(),
                value: (*value).into(),
            })
            .collect();
        let plan = BuildPlan::validate(input, &catalog).unwrap();
        let mut context = crate::review_context::tests::input(&plan);
        context.policy = Policy::WasmBuildV1;
        context.rustc.path = "/toolchain/bin/rustc".into();
        let context = ReviewContext::validate(&plan, context).unwrap();
        (BuildJob::queued(JobId::from_bytes([7; 16]), plan), context)
    };
    let owner = OwnerId::from_bytes([1; 16]);
    let (original, context) = make("sample-site", b"body {}\n");
    for (name, css) in [
        ("other-site", b"body {}\n".as_slice()),
        ("sample-site", b"body { color: navy; }\n"),
    ] {
        let approvals = Approvals::default();
        let handle = approvals
            .approve(owner, &original, context.clone())
            .unwrap();
        let (changed, fresh) = make(name, css);
        // Isolate the source edit: do not accidentally test a catalog/owner change.
        let mut plan = changed.plan().clone();
        plan.0.project = original.plan().details().project;
        plan.0.repository = original.plan().details().repository;
        let changed = BuildJob::queued(original.id(), plan);
        assert_eq!(
            approvals.consume(&handle, owner, &changed, &fresh),
            Err(approvals::Error::ReviewChanged)
        );
        assert_eq!(
            approvals.consume(&handle, owner, &original, &context),
            Err(approvals::Error::Inactive(approvals::Status::Invalidated))
        );
    }
}
