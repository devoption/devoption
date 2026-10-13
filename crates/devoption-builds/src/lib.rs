//! Immutable Unix Rust build plans bound to verified root Cargo source metadata.
//! Validation reads supplied values only, with no filesystem, process or network I/O.
//! A valid plan is neither reviewed nor approved and grants no execution rights.
//! See `docs/build-plans.md` for the checks still required before any executor.

#![cfg(unix)]

pub mod cargo_source;
pub mod jobs;

use devoption_projects::{Catalog, ProjectId, RepositoryId};
use devoption_registry::CargoMaterialization;
use std::{collections::HashSet, fmt};

pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_TARGET_BYTES: usize = 128;
pub const MAX_ENVIRONMENT_ENTRIES: usize = 32;
pub const MAX_ENVIRONMENT_NAME_BYTES: usize = 64;
pub const MAX_ENVIRONMENT_VALUE_BYTES: usize = 1024;

/// Caller-supplied metadata, never collected from the service environment.
/// Values must be non-secret. Syntax validation cannot detect secrets or decide
/// which variables a future executor may allow. Intentionally has no Debug impl.
#[derive(Clone, PartialEq, Eq)]
pub struct EnvironmentEntry {
    pub name: String,
    pub value: String,
}

/// Proposed limits, not enforced limits or an approved resource policy.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub elapsed_seconds: u32,
    pub log_bytes: u32,
}

/// Inputs to source-binding and structural validation. Paths are candidate Unix paths;
/// no existence, ownership, toolchain identity or source cleanliness is implied.
/// Owned strings ensure later edits to a caller's copy cannot change a plan.
#[derive(Clone, PartialEq, Eq)]
pub struct PlanInput {
    pub project: ProjectId,
    pub repository: RepositoryId,
    pub commit: String,
    /// Verified Cargo membership relative to the selected commit, not repository
    /// authentication, a complete source snapshot or execution authorization.
    pub source: cargo_source::RootCargoSource,
    /// Registry-supplied metadata, not proof of lockfile or bundle verification.
    pub dependencies: CargoMaterialization,
    pub rust_version: String,
    pub cargo_executable: String,
    pub work_directory: String,
    pub package: String,
    pub binary: String,
    pub environment: Vec<EnvironmentEntry>,
    pub limits: Limits,
}

/// An immutable candidate for later review, with no approval flag or run method.
/// Equality includes every input field. It does not prove catalog freshness or
/// external state equality and must not substitute for an approval lifecycle.
#[derive(Clone, PartialEq, Eq)]
pub struct BuildPlan(PlanInput);

impl BuildPlan {
    pub fn validate(input: PlanInput, catalog: &Catalog) -> Result<Self, Error> {
        let project = catalog
            .projects()
            .iter()
            .find(|p| p.id() == input.project)
            .ok_or(Error::UnknownProject)?;
        if !catalog
            .repositories()
            .iter()
            .any(|r| r.id() == input.repository)
        {
            return Err(Error::UnknownRepository);
        }
        if !project.repositories().contains(&input.repository) {
            return Err(Error::NotAssociated);
        }
        if !catalog
            .repository_bindings()
            .iter()
            .any(|b| b.repository() == input.repository)
            || !catalog
                .default_checkouts()
                .iter()
                .any(|c| c.project() == input.project && c.repository() == input.repository)
        {
            return Err(Error::NotRegistered);
        }
        // Only the currently supported Git SHA-1 format, in canonical lowercase.
        if input.commit.len() != 40
            || !input
                .commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || input.commit.bytes().all(|b| b == b'0')
        {
            return Err(Error::InvalidCommit);
        }
        if input.commit != input.source.commit()
            || input.dependencies.request() != input.source.request()
        {
            return Err(Error::SourceMismatch);
        }
        if !exact_version(&input.rust_version) {
            return Err(Error::InvalidRustVersion);
        }
        if !absolute_path(&input.cargo_executable) || !input.cargo_executable.ends_with("/cargo") {
            return Err(Error::InvalidExecutable);
        }
        if !absolute_path(&input.work_directory) {
            return Err(Error::InvalidWorkDirectory);
        }
        if !target_name(&input.package) || !target_name(&input.binary) {
            return Err(Error::InvalidTarget);
        }
        if input.environment.len() > MAX_ENVIRONMENT_ENTRIES {
            return Err(Error::InvalidEnvironment);
        }
        let mut names = HashSet::new();
        for entry in &input.environment {
            if entry.name.is_empty()
                || entry.name.len() > MAX_ENVIRONMENT_NAME_BYTES
                || !entry.name.bytes().enumerate().all(|(i, b)| {
                    b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit())
                })
                || entry.value.len() > MAX_ENVIRONMENT_VALUE_BYTES
                || entry.value.chars().any(char::is_control)
                || !names.insert(&entry.name)
            {
                return Err(Error::InvalidEnvironment);
            }
        }
        if input.limits.elapsed_seconds == 0 || input.limits.log_bytes == 0 {
            return Err(Error::InvalidLimits);
        }
        Ok(Self(input))
    }

    /// Read-only review metadata, without HTML/shell encoding or secret redaction.
    pub fn details(&self) -> &PlanInput {
        &self.0
    }

    /// Fixed argument vector for review. These flags constrain Cargo dependency
    /// resolution; they do not sandbox build scripts or prevent network access.
    pub fn cargo_arguments(&self) -> [&str; 8] {
        [
            "build",
            "--release",
            "--locked",
            "--offline",
            "--package",
            &self.0.package,
            "--bin",
            &self.0.binary,
        ]
    }
}

/// Fixed diagnostics deliberately exclude caller values and private paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    UnknownProject,
    UnknownRepository,
    NotAssociated,
    NotRegistered,
    InvalidCommit,
    SourceMismatch,
    InvalidRustVersion,
    InvalidExecutable,
    InvalidWorkDirectory,
    InvalidTarget,
    InvalidEnvironment,
    InvalidLimits,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownProject => "project is absent from the catalog snapshot",
            Self::UnknownRepository => "repository is absent from the catalog snapshot",
            Self::NotAssociated => "repository is not associated with this project",
            Self::NotRegistered => "association has no registered default checkout",
            Self::InvalidCommit => "commit must be a nonzero full lowercase SHA-1 object ID",
            Self::SourceMismatch => {
                "commit and dependency request must match verified Cargo source"
            }
            Self::InvalidRustVersion => "Rust version must be an exact stable numeric version",
            Self::InvalidExecutable => "executable must be a bounded absolute Unix cargo path",
            Self::InvalidWorkDirectory => "work directory must be a bounded absolute Unix path",
            Self::InvalidTarget => "package and binary must be bounded ASCII target names",
            Self::InvalidEnvironment => "environment metadata is malformed, duplicate or oversized",
            Self::InvalidLimits => "elapsed time and log limits must be nonzero",
        })
    }
}

impl std::error::Error for Error {}

fn exact_version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').take(4).collect();
    value.len() <= 17
        && parts.len() == 3
        && parts[0] != "0"
        && parts.iter().all(|part| {
            !part.is_empty()
                && (part.len() == 1 || !part.starts_with('0'))
                && part.bytes().all(|b| b.is_ascii_digit())
                && part.parse::<u16>().is_ok()
        })
}

fn absolute_path(value: &str) -> bool {
    value.len() <= MAX_PATH_BYTES
        && !value.chars().any(char::is_control)
        && value.strip_prefix('/').is_some_and(|relative| {
            relative
                .split('/')
                .all(|part| !matches!(part, "" | "." | ".."))
        })
}

fn target_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TARGET_BYTES
        && value.bytes().enumerate().all(|(i, b)| {
            b.is_ascii_alphabetic() || b == b'_' || (i > 0 && (b.is_ascii_digit() || b == b'-'))
        })
}

#[cfg(test)]
mod tests;
