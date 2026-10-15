//! Pure checks of proposed InitialBuildV1 settings; never OS enforcement.
use super::{DirectoryRole, Input, Policy};
use crate::{BuildPlan, absolute_path};
use std::collections::{HashMap, HashSet};

pub(super) fn permits(plan: &BuildPlan, context: &Input) -> bool {
    match context.policy {
        Policy::InitialBuildV1 => initial(plan, context),
    }
}

fn initial(plan: &BuildPlan, context: &Input) -> bool {
    let plan = plan.details();
    if !(1..=60).contains(&plan.limits.elapsed_seconds)
        || !(1..=1_048_576).contains(&plan.limits.log_bytes)
        || plan.environment.len() != 12
    {
        return false;
    }
    let environment: HashMap<_, _> = plan
        .environment
        .iter()
        .map(|entry| (entry.name.as_str(), entry.value.as_str()))
        .collect();
    if environment.len() != plan.environment.len() {
        return false;
    }
    for (name, expected) in [
        ("LANG", "C.UTF-8"),
        ("CARGO_NET_OFFLINE", "true"),
        ("CARGO_INCREMENTAL", "0"),
        ("CARGO_BUILD_JOBS", "1"),
        ("RUSTC_WRAPPER", ""),
        ("RUSTC_WORKSPACE_WRAPPER", ""),
        ("RUSTC", context.rustc.path.as_str()),
    ] {
        if environment.get(name) != Some(&expected) {
            return false;
        }
    }
    for (name, role) in [
        ("HOME", DirectoryRole::Home),
        ("CARGO_HOME", DirectoryRole::CargoHome),
        ("CARGO_TARGET_DIR", DirectoryRole::Target),
        ("TMPDIR", DirectoryRole::Temporary),
    ] {
        let Some(directory) = context.directories.iter().find(|d| d.role == role) else {
            return false;
        };
        if environment.get(name) != Some(&directory.path.as_str()) {
            return false;
        }
    }
    let Some(path) = environment.get("PATH") else {
        return false;
    };
    if path.len() > crate::MAX_ENVIRONMENT_VALUE_BYTES {
        return false;
    }
    let mut components = HashSet::new();
    for component in path.split(':') {
        if !absolute_path(component)
            || !components.insert(component)
            || context
                .directories
                .iter()
                .any(|d| overlaps(component, &d.path))
        {
            return false;
        }
    }
    [&context.cargo.path, &context.rustc.path]
        .into_iter()
        .all(|executable| {
            executable
                .rsplit_once('/')
                .is_some_and(|(parent, _)| components.contains(parent))
        })
}

// Component boundaries matter: /job must not reject its disjoint sibling /job-tools.
fn overlaps(left: &str, right: &str) -> bool {
    fn contains(parent: &str, child: &str) -> bool {
        child == parent
            || child
                .strip_prefix(parent)
                .is_some_and(|rest| rest.starts_with('/'))
    }
    contains(left, right) || contains(right, left)
}

#[cfg(test)]
mod tests;
