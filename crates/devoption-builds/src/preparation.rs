//! Initial source preparation observations. Never an approval, BuildPlan or job.
use devoption_projects::{
    Catalog, Error, ProjectId, RepositoryId,
    worktree::{ChangeCounts, FileContent, GitStatus, PermittedRoot},
};
use devoption_registry::cargo_inputs::{self, InspectedInputs};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    ChangesNeeded,
    Unavailable,
}

pub struct Check {
    pub label: &'static str,
    pub outcome: Outcome,
    pub detail: String,
}

pub struct Preparation {
    pub status: GitStatus,
    pub checks: Vec<Check>,
    pub cargo: Option<InspectedInputs>,
}

impl Preparation {
    pub fn outcome(&self) -> Outcome {
        if self
            .checks
            .iter()
            .any(|check| check.outcome == Outcome::Unavailable)
        {
            Outcome::Unavailable
        } else if self
            .checks
            .iter()
            .any(|check| check.outcome == Outcome::ChangesNeeded)
        {
            Outcome::ChangesNeeded
        } else {
            Outcome::Passed
        }
    }
}

/// Reads the registered checkout using existing bounded Git and file inspection.
/// External writers remain trusted; passing observations do not prove committed
/// bytes, compiler validity, dependency availability or execution readiness.
pub fn inspect(
    catalog: &Catalog,
    project: ProjectId,
    repository: RepositoryId,
    root: &PermittedRoot,
) -> Result<Preparation, Error> {
    let source = catalog.source_inputs(project, repository, root)?;
    let mut checks = vec![Check {
        label: "Commit",
        outcome: if source.status.commit.is_some() {
            Outcome::Passed
        } else {
            Outcome::ChangesNeeded
        },
        detail: if source.status.commit.is_some() {
            "HEAD identifies a commit."
        } else {
            "Create the first commit with Git, then check again."
        }
        .into(),
    }];
    let changes = &source.status.changes;
    checks.push(Check {
        label: "Working tree",
        outcome: if *changes == ChangeCounts::default() { Outcome::Passed } else { Outcome::ChangesNeeded },
        detail: if *changes == ChangeCounts::default() { "Git reports no staged, modified, untracked or conflicted entries.".into() } else {
            format!("{} staged · {} modified · {} untracked · {} conflicted. Review and commit or resolve these changes with Git, then check again.", changes.staged, changes.modified, changes.untracked, changes.conflicted)
        },
    });
    let mut text = Vec::new();
    for (path, content) in &source.files {
        let (outcome, detail, value) = match content {
            Some(FileContent::Text(value)) if !value.trim().is_empty() => (
                Outcome::Passed,
                "Tracked text file is readable.",
                Some(value.as_bytes()),
            ),
            Some(FileContent::Text(_)) => (
                Outcome::ChangesNeeded,
                "This tracked file is empty. Add its contents and commit with Git.",
                None,
            ),
            None => (
                Outcome::ChangesNeeded,
                "This file is not tracked. Add it and commit with Git.",
                None,
            ),
            Some(FileContent::TooLarge) => (
                Outcome::Unavailable,
                "This file exceeds the 64 KiB inspection limit.",
                None,
            ),
            Some(FileContent::Binary) => (
                Outcome::Unavailable,
                "This file is not supported UTF-8 text.",
                None,
            ),
            Some(FileContent::Unsupported | FileContent::Unavailable) => (
                Outcome::Unavailable,
                "This tracked file is missing, changed or unsafe to read. Inspect it with Git.",
                None,
            ),
        };
        checks.push(Check {
            label: path,
            outcome,
            detail: detail.into(),
        });
        text.push(value);
    }
    let cargo = match (text[0], text[1]) {
        (Some(manifest), Some(lockfile)) => match cargo_inputs::inspect(manifest, lockfile) {
            Ok(cargo) => {
                checks.push(Check { label: "Cargo inputs", outcome: Outcome::Passed, detail: "The standalone manifest and version-4 lockfile are internally consistent. Dependency archives have not been acquired or verified.".into() });
                Some(cargo)
            }
            Err(error) => {
                checks.push(Check {
                    label: "Cargo inputs",
                    outcome: Outcome::ChangesNeeded,
                    detail: format!("{} See the supported source format below.", error),
                });
                None
            }
        },
        _ => {
            checks.push(Check { label: "Cargo inputs", outcome: Outcome::ChangesNeeded, detail: "Both Cargo.toml and Cargo.lock must be readable before their consistency can be checked.".into() });
            None
        }
    };
    Ok(Preparation {
        status: source.status,
        checks,
        cargo,
    })
}

#[cfg(test)]
mod tests;
