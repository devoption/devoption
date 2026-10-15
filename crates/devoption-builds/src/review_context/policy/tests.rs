use super::*;
use crate::{
    EnvironmentEntry,
    approvals::{Approvals, Error as ApprovalError, OwnerId, Status},
    jobs::{BuildJob, JobId},
    review_context::{Error, ReviewContext},
};

fn fixture() -> (BuildPlan, Input) {
    let plan = crate::review_context::tests::plan();
    let context = crate::review_context::tests::input(&plan);
    (plan, context)
}
fn set(plan: &mut BuildPlan, name: &str, value: &str) {
    plan.0
        .environment
        .iter_mut()
        .find(|entry| entry.name == name)
        .unwrap()
        .value = value.into();
}
fn rejected(plan: &BuildPlan, context: Input) {
    assert_eq!(
        ReviewContext::validate(plan, context).err(),
        Some(Error::Policy)
    );
}

#[test]
fn explicit_environment_is_complete_order_independent_and_not_synthesized() {
    let (mut plan, input) = fixture();
    let context = ReviewContext::validate(&plan, input.clone()).unwrap();
    plan.0.environment.reverse();
    assert!(ReviewContext::validate(&plan, input.clone()).unwrap() == context);
    // Environment order is still part of exact BuildPlan equality, by design.
    for name in input_variables(&plan) {
        let mut missing = plan.clone();
        missing.0.environment.retain(|entry| entry.name != name);
        rejected(&missing, input.clone());
    }
    let mut empty = plan.clone();
    empty.0.environment.clear();
    rejected(&empty, input);
}
fn input_variables(plan: &BuildPlan) -> Vec<String> {
    plan.0.environment.iter().map(|e| e.name.clone()).collect()
}

#[test]
fn extra_duplicate_and_case_changed_names_cannot_smuggle_in_overrides() {
    let (plan, context) = fixture();
    for name in [
        "SSH_AUTH_SOCK",
        "RUSTFLAGS",
        "CARGO_HOME_OTHER",
        "HTTP_PROXY",
        "SECRET_TOKEN",
    ] {
        let mut extra = plan.clone();
        extra.0.environment.push(EnvironmentEntry {
            name: name.into(),
            value: "fixture".into(),
        });
        rejected(&extra, context.clone());
        extra.0.environment.remove(0); // Same count is not enough.
        rejected(&extra, context.clone());
    }
    let mut duplicate = plan.clone();
    duplicate.0.environment[0] = duplicate.0.environment[1].clone();
    rejected(&duplicate, context.clone());
    let mut lowercase = plan.clone();
    lowercase.0.environment[0].name.make_ascii_lowercase();
    rejected(&lowercase, context);
}

#[test]
fn fixed_values_wrappers_and_directory_bindings_are_exact() {
    let (plan, context) = fixture();
    for (name, wrong) in [
        ("LANG", "en_US.UTF-8"),
        ("CARGO_NET_OFFLINE", "false"),
        ("CARGO_INCREMENTAL", "1"),
        ("CARGO_BUILD_JOBS", "2"),
        ("RUSTC_WRAPPER", "/example/wrapper"),
        ("RUSTC_WORKSPACE_WRAPPER", " "),
        ("RUSTC", "/another/rustc"),
        ("HOME", "/another/home"),
        ("CARGO_HOME", "/another/cargo"),
        ("CARGO_TARGET_DIR", "/another/target"),
        ("TMPDIR", "/another/tmp"),
    ] {
        let mut changed = plan.clone();
        set(&mut changed, name, wrong);
        rejected(&changed, context.clone());
    }
    for (name, role) in [
        ("HOME", DirectoryRole::Home),
        ("CARGO_HOME", DirectoryRole::CargoHome),
        ("CARGO_TARGET_DIR", DirectoryRole::Target),
        ("TMPDIR", DirectoryRole::Temporary),
    ] {
        let mut changed = context.clone();
        changed
            .directories
            .iter_mut()
            .find(|d| d.role == role)
            .unwrap()
            .path
            .push_str("-changed");
        rejected(&plan, changed.clone());
        let mut matched = plan.clone();
        set(
            &mut matched,
            name,
            &changed
                .directories
                .iter()
                .find(|d| d.role == role)
                .unwrap()
                .path,
        );
        assert!(ReviewContext::validate(&matched, changed).is_ok());
    }
}

#[test]
fn path_rejects_implicit_search_ambiguity_and_job_overlap() {
    let (plan, context) = fixture();
    let compiler = "/example/toolchains/1.99.0/bin";
    for bad in [
        "".to_owned(),
        "/usr/bin".into(),
        format!("{compiler}:"),
        format!(":{compiler}"),
        format!("{compiler}::/usr/bin"),
        format!("{compiler}:relative"),
        format!("{compiler}:/usr/../bin"),
        format!("{compiler}:/usr//bin"),
        format!("{compiler}:{compiler}"),
        format!("{compiler}:/"),
        format!("{compiler}:/example"),
        format!("{compiler}:/example/jobs/one/bin"),
        format!("{compiler}:/example/jobs/one"),
        format!("{compiler}:/tmp/\n"),
        format!("{compiler}:/{}", "x".repeat(1024)),
    ] {
        let mut changed = plan.clone();
        set(&mut changed, "PATH", &bad);
        rejected(&changed, context.clone());
    }
    // Each role is excluded even if a later context locates it outside Work.
    for role in [
        DirectoryRole::Source,
        DirectoryRole::Home,
        DirectoryRole::CargoHome,
        DirectoryRole::Target,
        DirectoryRole::Temporary,
        DirectoryRole::Artifact,
    ] {
        let mut external = context.clone();
        external
            .directories
            .iter_mut()
            .find(|d| d.role == role)
            .unwrap()
            .path = "/external-owned".into();
        let mut changed = plan.clone();
        let variable = match role {
            DirectoryRole::Home => Some("HOME"),
            DirectoryRole::CargoHome => Some("CARGO_HOME"),
            DirectoryRole::Target => Some("CARGO_TARGET_DIR"),
            DirectoryRole::Temporary => Some("TMPDIR"),
            _ => None,
        };
        if let Some(name) = variable {
            set(&mut changed, name, "/external-owned");
        }
        assert!(permits(&changed, &external));
        set(
            &mut changed,
            "PATH",
            &format!("{compiler}:/external-owned/bin"),
        );
        assert!(!permits(&changed, &external));
    }
    let mut sibling = plan.clone();
    set(
        &mut sibling,
        "PATH",
        &format!("{compiler}:/example/jobs/one-tools"),
    );
    assert!(ReviewContext::validate(&sibling, context).is_ok());
}

#[test]
fn both_reviewed_tool_directories_must_be_present_in_path() {
    let (mut plan, mut context) = fixture();
    context.rustc.path = "/another/bin/rustc".into();
    set(&mut plan, "RUSTC", &context.rustc.path);
    rejected(&plan, context.clone());
    set(&mut plan, "PATH", "/another/bin");
    rejected(&plan, context.clone());
    set(
        &mut plan,
        "PATH",
        "/example/toolchains/1.99.0/bin:/another/bin",
    );
    assert!(ReviewContext::validate(&plan, context).is_ok());
}

#[test]
fn proposed_limits_have_inclusive_caps_and_nonzero_lower_bounds() {
    let (plan, context) = fixture();
    for (seconds, bytes, accepted) in [
        (1, 1, true),
        (60, 1_048_576, true),
        (0, 1, false),
        (1, 0, false),
        (61, 1, false),
        (1, 1_048_577, false),
        (u32::MAX, u32::MAX, false),
    ] {
        let mut candidate = plan.clone();
        candidate.0.limits.elapsed_seconds = seconds;
        candidate.0.limits.log_bytes = bytes;
        assert_eq!(
            ReviewContext::validate(&candidate, context.clone()).is_ok(),
            accepted
        );
    }
}

#[test]
fn existing_context_cannot_bypass_policy_at_issuance_or_revive_changed_approval() {
    let (plan, input) = fixture();
    let context = ReviewContext::validate(&plan, input).unwrap();
    let owner = OwnerId::from_bytes([1; 16]);
    for limit in [false, true] {
        let registry = Approvals::default();
        let mut invalid = plan.clone();
        if limit {
            invalid.0.limits.elapsed_seconds = 61;
        } else {
            set(&mut invalid, "RUSTC_WRAPPER", "/bad/wrapper");
        }
        let bad = BuildJob::queued(JobId::from_bytes([1; 16]), invalid);
        assert!(matches!(
            registry.approve(owner, &bad, context.clone()),
            Err(ApprovalError::ReviewChanged)
        ));
        let good = BuildJob::queued(bad.id(), plan.clone());
        let approval = registry.approve(owner, &good, context.clone()).unwrap();
        assert_eq!(
            registry.consume(&approval, owner, &bad, &context),
            Err(ApprovalError::ReviewChanged)
        );
        assert_eq!(
            registry.consume(&approval, owner, &good, &context),
            Err(ApprovalError::Inactive(Status::Invalidated))
        );
    }
}
