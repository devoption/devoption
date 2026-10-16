//! Explicit local review for device-number recovery, never automatic continuity proof.
#[cfg(test)]
mod tests;
use crate::{
    Catalog, Error, codec,
    worktree::{FilesystemIdentity, PermittedRoot},
};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::Path};

/// Display-only observations. Approval binds private copies, not these public fields.
pub struct IdentityChange {
    pub location: String,
    pub previous: (u64, u64),
    pub current: (u64, u64),
}

/// No Debug: this local review contains private paths and catalog bytes.
pub struct RecoveryReview {
    pub(crate) original: Catalog,
    pub(crate) replacement: Catalog,
    fingerprint: [u8; 32],
    changes: Vec<IdentityChange>,
}

impl RecoveryReview {
    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }
    pub fn changes(&self) -> &[IdentityChange] {
        &self.changes
    }
    pub fn backup_name(&self) -> String {
        format!(
            "projects.before-rebind-{}.bin",
            self.fingerprint
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
    }
}

/// Require the same saved path and every inode, but do not treat these as proof
/// of the same filesystem. A human must separately approve the reviewed new grant.
pub(crate) fn review(
    catalog: Catalog,
    root: &PermittedRoot,
    data: (u64, u64),
) -> Result<RecoveryReview, Error> {
    catalog.validate()?;
    let saved = catalog.root.as_ref().ok_or(Error::RootMismatch)?;
    let (path, current) = root.binding()?;
    if path != saved.path || current == saved.identity {
        return Err(Error::RootMismatch);
    }
    let mut mapping = Mapping::default();
    mapping.check(saved.identity, current)?;
    let mut changes = vec![change(path, saved.identity, current)];
    let mut replacement = catalog.clone();
    replacement
        .root
        .as_mut()
        .ok_or(Error::InvalidCatalog)?
        .identity = current;
    let mut observations = Vec::new();
    let mut repositories = HashMap::new();
    for (index, checkout) in catalog.checkouts.iter().enumerate() {
        let observed = root.inspect(Path::new(&checkout.path))?;
        let binding = catalog
            .bindings
            .iter()
            .find(|b| b.repository == checkout.repository)
            .ok_or(Error::InvalidCatalog)?;
        mapping.check(checkout.worktree, observed.worktree)?;
        mapping.check(checkout.git_directory, observed.git_directory)?;
        mapping.check(binding.common_directory, observed.common_directory)?;
        if repositories
            .insert(checkout.repository, observed.common_directory)
            .is_some_and(|old| old != observed.common_directory)
        {
            return Err(Error::StaleCheckout);
        }
        changes.push(change(
            format!("{} (worktree)", checkout.path),
            checkout.worktree,
            observed.worktree,
        ));
        changes.push(change(
            format!("{} (git directory)", checkout.path),
            checkout.git_directory,
            observed.git_directory,
        ));
        changes.push(change(
            format!("{} (common directory)", checkout.path),
            binding.common_directory,
            observed.common_directory,
        ));
        replacement.checkouts[index].worktree = observed.worktree;
        replacement.checkouts[index].git_directory = observed.git_directory;
        observations.push((checkout.path.clone(), observed));
    }
    for binding in &mut replacement.bindings {
        binding.common_directory = *repositories
            .get(&binding.repository)
            .ok_or(Error::InvalidCatalog)?;
    }
    replacement.validate()?;
    replacement.check_repository_root(root)?;
    for (path, observed) in &observations {
        crate::registration::revalidate_checkout(root, Path::new(path), observed)?;
    }
    let mut digest = Sha256::new();
    digest.update(b"devoption.device-rebind.v1\0");
    digest.update(data.0.to_le_bytes());
    digest.update(data.1.to_le_bytes());
    for bytes in [codec::encode(&catalog)?, codec::encode(&replacement)?] {
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    Ok(RecoveryReview {
        original: catalog,
        replacement,
        fingerprint: digest.finalize().into(),
        changes,
    })
}

fn change(
    location: String,
    previous: FilesystemIdentity,
    current: FilesystemIdentity,
) -> IdentityChange {
    IdentityChange {
        location,
        previous: (previous.device, previous.inode),
        current: (current.device, current.inode),
    }
}

#[derive(Default)]
struct Mapping {
    forward: HashMap<u64, u64>,
    reverse: HashMap<u64, u64>,
}
impl Mapping {
    fn check(
        &mut self,
        previous: FilesystemIdentity,
        current: FilesystemIdentity,
    ) -> Result<(), Error> {
        if previous.inode != current.inode
            || self
                .forward
                .insert(previous.device, current.device)
                .is_some_and(|old| old != current.device)
            || self
                .reverse
                .insert(current.device, previous.device)
                .is_some_and(|old| old != previous.device)
        {
            return Err(Error::StaleCheckout);
        }
        Ok(())
    }
}
