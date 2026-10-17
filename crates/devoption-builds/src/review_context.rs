//! Structurally checked external review metadata. No filesystem or process I/O.
//! Digests and inode observations are supplied claims, not verification evidence.

mod policy;

use crate::{BuildPlan, absolute_path};
use devoption_registry::Sha256;
use std::{collections::HashSet, fmt};

/// Fixed initial policy from docs/execution-trust.md. Changes require a new version.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    InitialBuildV1,
    WasmBuildV1,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum DirectoryRole {
    Work = 1,
    Source,
    Home,
    CargoHome,
    Target,
    Temporary,
    Artifact,
}
const ROLES: [DirectoryRole; 7] = [
    DirectoryRole::Work,
    DirectoryRole::Source,
    DirectoryRole::Home,
    DirectoryRole::CargoHome,
    DirectoryRole::Target,
    DirectoryRole::Temporary,
    DirectoryRole::Artifact,
];

#[derive(Clone, PartialEq, Eq)]
pub struct Executable {
    pub path: String,
    pub sha256: Sha256,
}

/// Private directory observation, not a retained descriptor or a race-free check.
#[derive(Clone, PartialEq, Eq)]
pub struct Directory {
    pub role: DirectoryRole,
    pub path: String,
    pub device: u64,
    pub inode: u64,
    pub owner_uid: u32,
    pub mode: u32,
}

/// Caller-supplied values for review. No Debug prevents accidental path disclosure.
#[derive(Clone, PartialEq, Eq)]
pub struct Input {
    pub policy: Policy,
    pub rust_version: String,
    pub cargo: Executable,
    pub rustc: Executable,
    /// Digest of the complete compiler/linker/support-file manifest, not just rustc.
    pub toolchain_manifest: Sha256,
    /// Fresh selected catalog binding, including repository/root filesystem identity.
    pub catalog_binding: Sha256,
    pub source_snapshot: Sha256,
    pub dependency_bundle: Sha256,
    /// Controlled Cargo config, dependency layout and effective environment/config.
    pub configuration: Sha256,
    pub execution_uid: u32,
    pub directories: Vec<Directory>,
}

/// Immutable, normalized review data, distinct from verified execution authority.
#[derive(Clone, PartialEq, Eq)]
pub struct ReviewContext(Input);

impl ReviewContext {
    pub fn validate(plan: &BuildPlan, mut input: Input) -> Result<Self, Error> {
        if !absolute_path(&input.cargo.path)
            || !absolute_path(&input.rustc.path)
            || !input.rustc.path.ends_with("/rustc")
            || input.cargo.path == input.rustc.path
        {
            return Err(Error::Executable);
        }
        if input.execution_uid == 0 {
            return Err(Error::Identity);
        }
        if input.directories.len() != ROLES.len() {
            return Err(Error::Directories);
        }
        input.directories.sort_by_key(|d| d.role);
        let mut paths = HashSet::new();
        let mut identities = HashSet::new();
        for (directory, expected) in input.directories.iter().zip(ROLES) {
            if directory.role != expected
                || !absolute_path(&directory.path)
                || directory.inode == 0
                || directory.owner_uid == 0
                || !matches!(directory.mode, 0o500 | 0o700)
                || !paths.insert(&directory.path)
                || !identities.insert((directory.device, directory.inode))
            {
                return Err(Error::Directories);
            }
        }
        let context = Self(input);
        if !context.matches_plan_metadata(plan) {
            return Err(Error::PlanMismatch);
        }
        if !policy::permits(plan, &context.0) {
            return Err(Error::Policy);
        }
        Ok(context)
    }

    pub fn details(&self) -> &Input {
        &self.0
    }

    /// Canonical metadata fingerprint; never proof that any supplied claim is true.
    pub fn sha256(&self) -> Sha256 {
        Sha256::digest(&self.encode())
    }

    pub(crate) fn matches_plan(&self, plan: &BuildPlan) -> bool {
        self.matches_plan_metadata(plan) && policy::permits(plan, &self.0)
    }

    fn matches_plan_metadata(&self, plan: &BuildPlan) -> bool {
        let p = plan.details();
        self.0.rust_version == p.rust_version
            && self.0.cargo.path == p.cargo_executable
            && self.0.directories[0].path == p.work_directory
            && self.0.source_snapshot == p.source.sha256()
            && self.0.dependency_bundle == p.dependencies.bundle_sha256()
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = b"devoption.review-context\0\x01".to_vec();
        let input = &self.0;
        field(
            &mut out,
            1,
            &[match input.policy {
                Policy::InitialBuildV1 => 1,
                Policy::WasmBuildV1 => 2,
            }],
        );
        field(&mut out, 2, input.rust_version.as_bytes());
        field(&mut out, 3, input.cargo.path.as_bytes());
        field(&mut out, 4, input.cargo.sha256.bytes());
        field(&mut out, 5, input.rustc.path.as_bytes());
        field(&mut out, 6, input.rustc.sha256.bytes());
        field(&mut out, 7, input.toolchain_manifest.bytes());
        field(&mut out, 8, input.catalog_binding.bytes());
        field(&mut out, 9, input.source_snapshot.bytes());
        field(&mut out, 10, input.dependency_bundle.bytes());
        field(&mut out, 11, input.configuration.bytes());
        field(&mut out, 12, &input.execution_uid.to_be_bytes());
        for directory in &input.directories {
            let mut encoded = Vec::new();
            field(&mut encoded, 1, directory.path.as_bytes());
            field(&mut encoded, 2, &directory.device.to_be_bytes());
            field(&mut encoded, 3, &directory.inode.to_be_bytes());
            field(&mut encoded, 4, &directory.owner_uid.to_be_bytes());
            field(&mut encoded, 5, &directory.mode.to_be_bytes());
            field(&mut out, 12 + directory.role as u8, &encoded);
        }
        out
    }
}

// TLV: one byte tag, unsigned 64-bit big-endian byte length, exact payload.
fn field(out: &mut Vec<u8>, tag: u8, bytes: &[u8]) {
    out.push(tag);
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Executable,
    Identity,
    Directories,
    PlanMismatch,
    Policy,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Executable => "invalid review executable metadata",
            Self::Identity => "review requires an unprivileged execution identity",
            Self::Directories => "invalid review directory metadata",
            Self::PlanMismatch => "review context does not match the plan",
            Self::Policy => "proposed build settings violate review policy",
        })
    }
}
impl std::error::Error for Error {}

#[cfg(test)]
pub(crate) mod tests;
