use super::*;

pub(crate) fn plan() -> BuildPlan {
    let (catalog, mut input) = crate::tests::fixture();
    input.environment = [
        ("PATH", "/example/toolchains/1.99.0/bin:/usr/bin"),
        ("RUSTC", "/example/toolchains/1.99.0/bin/rustc"),
        ("HOME", "/example/jobs/one/role-2"),
        ("CARGO_HOME", "/example/jobs/one/role-3"),
        ("CARGO_TARGET_DIR", "/example/jobs/one/role-4"),
        ("TMPDIR", "/example/jobs/one/role-5"),
        ("LANG", "C.UTF-8"),
        ("CARGO_NET_OFFLINE", "true"),
        ("CARGO_INCREMENTAL", "0"),
        ("CARGO_BUILD_JOBS", "1"),
        ("RUSTC_WRAPPER", ""),
        ("RUSTC_WORKSPACE_WRAPPER", ""),
    ]
    .into_iter()
    .map(|(name, value)| crate::EnvironmentEntry {
        name: name.into(),
        value: value.into(),
    })
    .collect();
    BuildPlan::validate(input, &catalog).unwrap()
}

pub(crate) fn input(plan: &BuildPlan) -> Input {
    Input {
        policy: Policy::InitialBuildV1,
        rust_version: plan.details().rust_version.clone(),
        cargo: Executable {
            path: plan.details().cargo_executable.clone(),
            sha256: Sha256::digest(b"cargo"),
        },
        rustc: Executable {
            path: "/example/toolchains/1.99.0/bin/rustc".into(),
            sha256: Sha256::digest(b"rustc"),
        },
        toolchain_manifest: Sha256::digest(b"toolchain"),
        catalog_binding: Sha256::digest(b"catalog"),
        source_snapshot: plan.details().source.sha256(),
        dependency_bundle: plan.details().dependencies.bundle_sha256(),
        configuration: Sha256::digest(b"configuration"),
        execution_uid: 1000,
        directories: ROLES
            .into_iter()
            .enumerate()
            .map(|(i, role)| Directory {
                role,
                path: if i == 0 {
                    plan.details().work_directory.clone()
                } else {
                    format!("/example/jobs/one/role-{i}")
                },
                device: 1,
                inode: i as u64 + 10,
                owner_uid: 1000,
                mode: 0o700,
            })
            .collect(),
    }
}

#[test]
fn ordering_is_canonical_and_inputs_cannot_mutate_validated_context() {
    let plan = plan();
    let mut supplied = input(&plan);
    let first = ReviewContext::validate(&plan, supplied.clone()).unwrap();
    supplied.directories.reverse();
    let second = ReviewContext::validate(&plan, supplied.clone()).unwrap();
    assert!(first == second);
    assert!(first.sha256() == second.sha256());
    supplied.directories[0].path = "/changed".into();
    assert!(first == second);
    assert_eq!(
        first.details().directories[0].path,
        plan.details().work_directory
    );
}

#[test]
fn invalid_paths_roles_aliases_permissions_and_privileged_accounts_are_rejected() {
    let plan = plan();
    for invalid in [
        "/",
        "relative",
        "/a/../b",
        "/a/./b",
        "/a//b",
        "/a/",
        "/a\n",
        &format!("/{}", "x".repeat(4096)),
    ] {
        let mut data = input(&plan);
        data.rustc.path = invalid.into();
        assert_eq!(
            ReviewContext::validate(&plan, data).err(),
            Some(Error::Executable)
        );
        let mut data = input(&plan);
        data.directories[1].path = invalid.into();
        assert_eq!(
            ReviewContext::validate(&plan, data).err(),
            Some(Error::Directories)
        );
    }
    for case in 0..9 {
        let mut data = input(&plan);
        match case {
            0 => {
                data.directories.pop();
            }
            1 => data.directories.push(data.directories[0].clone()),
            2 => data.directories[1].role = DirectoryRole::Work,
            3 => data.directories[1].path = data.directories[0].path.clone(),
            4 => data.directories[1].inode = data.directories[0].inode,
            5 => data.directories[1].inode = 0,
            6 => data.directories[1].owner_uid = 0,
            7 => data.directories[1].mode = 0o755,
            _ => data.directories[1].mode = 0o4700,
        }
        assert_eq!(
            ReviewContext::validate(&plan, data).err(),
            Some(Error::Directories)
        );
    }
    let mut data = input(&plan);
    data.execution_uid = 0;
    assert_eq!(
        ReviewContext::validate(&plan, data).err(),
        Some(Error::Identity)
    );
}

#[test]
fn context_must_match_plan_toolchain_work_source_and_dependencies() {
    let plan = plan();
    for case in 0..5 {
        let mut data = input(&plan);
        match case {
            0 => data.rust_version = "1.0.0".into(),
            1 => data.cargo.path = "/different/cargo".into(),
            2 => data.directories[0].path = "/different/work".into(),
            3 => data.source_snapshot = Sha256::digest(b"different source"),
            _ => data.dependency_bundle = Sha256::digest(b"different dependencies"),
        }
        assert_eq!(
            ReviewContext::validate(&plan, data).err(),
            Some(Error::PlanMismatch)
        );
    }
}

#[test]
fn every_external_field_changes_the_binding() {
    let plan = plan();
    let original = ReviewContext::validate(&plan, input(&plan)).unwrap();
    for case in 0..8 {
        let mut data = input(&plan);
        match case {
            0 => data.cargo.sha256 = Sha256::digest(b"changed"),
            1 => data.rustc.path = "/another/rustc".into(),
            2 => data.rustc.sha256 = Sha256::digest(b"changed"),
            3 => data.toolchain_manifest = Sha256::digest(b"changed"),
            4 => data.catalog_binding = Sha256::digest(b"changed"),
            5 => data.configuration = Sha256::digest(b"changed"),
            6 => data.execution_uid += 1,
            _ => data.directories[1].path.push_str("-changed"),
        }
        let mut candidate = plan.clone();
        if case == 1 {
            candidate
                .0
                .environment
                .iter_mut()
                .find(|e| e.name == "RUSTC")
                .unwrap()
                .value = data.rustc.path.clone();
            candidate
                .0
                .environment
                .iter_mut()
                .find(|e| e.name == "PATH")
                .unwrap()
                .value
                .push_str(":/another");
        }
        let changed = ReviewContext::validate(&candidate, data).unwrap();
        assert!(changed != original);
        assert!(changed.sha256() != original.sha256());
    }
    for role in 0..ROLES.len() {
        for field in 0..4 {
            let mut data = input(&plan);
            match field {
                0 => data.directories[role].device += 1,
                1 => data.directories[role].inode += 100,
                2 => data.directories[role].owner_uid += 1,
                _ => data.directories[role].mode = 0o500,
            }
            assert!(ReviewContext::validate(&plan, data).unwrap().sha256() != original.sha256());
        }
    }
}

#[test]
fn encoding_is_domain_separated_length_delimited_and_stable() {
    // Synthetic encoder-only vector independent of source fixture layout.
    // Validation/plan binding is tested separately above.
    let plan = plan();
    let mut data = input(&plan);
    data.source_snapshot = Sha256::digest(b"source");
    data.dependency_bundle = Sha256::digest(b"bundle");
    let context = ReviewContext(data);
    let bytes = context.encode();
    assert!(bytes.starts_with(b"devoption.review-context\0\x01"));
    let hex: String = context
        .sha256()
        .bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        hex,
        "14476c542c423164edc0956cb8ba3f2c026874ac38c4f982b5cf5bb02bde511b"
    );
    let (mut left, mut right) = (Vec::new(), Vec::new());
    field(&mut left, 1, b"ab");
    field(&mut left, 2, b"c");
    field(&mut right, 1, b"a");
    field(&mut right, 2, b"bc");
    assert_ne!(left, right);
    let mut retagged = Vec::new();
    field(&mut retagged, 2, b"ab");
    assert_ne!(&left[..11], retagged.as_slice());
}
