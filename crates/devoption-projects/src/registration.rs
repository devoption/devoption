//! Durable repository identity and each project's selected default checkout.
//! Stored filesystem observations require revalidation before later operations.

use crate::{
    Catalog, Error, ProjectId, RepositoryId,
    worktree::{FilesystemIdentity, PermittedRoot, WorktreeObservation},
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootBinding {
    pub(crate) path: String,
    pub(crate) identity: FilesystemIdentity,
}

impl RootBinding {
    pub fn path(&self) -> &Path {
        Path::new(&self.path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryBinding {
    pub(crate) repository: RepositoryId,
    pub(crate) common_directory: FilesystemIdentity,
}

impl RepositoryBinding {
    pub fn repository(&self) -> RepositoryId {
        self.repository
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultCheckout {
    pub(crate) project: ProjectId,
    pub(crate) repository: RepositoryId,
    pub(crate) path: String,
    pub(crate) worktree: FilesystemIdentity,
    pub(crate) git_directory: FilesystemIdentity,
}

impl DefaultCheckout {
    pub fn project(&self) -> ProjectId {
        self.project
    }
    pub fn repository(&self) -> RepositoryId {
        self.repository
    }
    pub fn relative_path(&self) -> &Path {
        Path::new(&self.path)
    }
}

impl Catalog {
    pub fn repository_root(&self) -> Option<&RootBinding> {
        self.root.as_ref()
    }
    pub fn repository_bindings(&self) -> &[RepositoryBinding] {
        &self.bindings
    }
    pub fn default_checkouts(&self) -> &[DefaultCheckout] {
        &self.checkouts
    }

    /// Check an explicitly configured capability against the saved root without
    /// opening any saved checkout or modifying the catalog.
    pub fn check_repository_root(&self, root: &PermittedRoot) -> Result<(), Error> {
        let (path, identity) = root.binding()?;
        if self
            .root
            .as_ref()
            .is_some_and(|saved| saved.path != path || saved.identity != identity)
        {
            return Err(Error::RootMismatch);
        }
        Ok(())
    }

    /// Read status only for a saved, revalidated project/repository checkout.
    pub fn repository_status(
        &self,
        project: ProjectId,
        repository: RepositoryId,
        root: &PermittedRoot,
    ) -> Result<crate::worktree::GitStatus, Error> {
        self.check_repository_root(root)?;
        let selected = self
            .projects
            .iter()
            .find(|p| p.id == project)
            .ok_or(Error::UnknownProject)?;
        if !selected.repositories.contains(&repository) {
            return Err(Error::UnknownRepository);
        }
        let checkout = self
            .checkouts
            .iter()
            .find(|c| c.project == project && c.repository == repository)
            .ok_or(Error::CheckoutRequired)?;
        let path = Path::new(&checkout.path);
        let before = root.inspect(path)?;
        self.check_checkout(&checkout.path, &before)?;
        let status = root.git_status(path)?;
        revalidate_checkout(root, path, &before)?;
        self.check_repository_root(root)?;
        Ok(status)
    }

    /// Browse only a saved, revalidated association. Selected paths are tracked
    /// working-tree files, never arbitrary root-relative filesystem reads.
    pub fn repository_files(
        &self,
        project: ProjectId,
        repository: RepositoryId,
        root: &PermittedRoot,
        selected: Option<&str>,
    ) -> Result<crate::worktree::RepositoryFiles, Error> {
        self.check_repository_root(root)?;
        let project_record = self
            .projects
            .iter()
            .find(|p| p.id == project)
            .ok_or(Error::UnknownProject)?;
        if !project_record.repositories.contains(&repository) {
            return Err(Error::UnknownRepository);
        }
        let checkout = self
            .checkouts
            .iter()
            .find(|c| c.project == project && c.repository == repository)
            .ok_or(Error::CheckoutRequired)?;
        let path = Path::new(&checkout.path);
        let before = root.inspect(path)?;
        self.check_checkout(&checkout.path, &before)?;
        let files = root.repository_files(path, selected)?;
        revalidate_checkout(root, path, &before)?;
        self.check_repository_root(root)?;
        Ok(files)
    }

    pub(crate) fn attach(
        &mut self,
        project: ProjectId,
        root: &PermittedRoot,
        path: &Path,
    ) -> Result<RepositoryId, Error> {
        if !self.projects.iter().any(|p| p.id == project) {
            return Err(Error::UnknownProject);
        }
        self.check_repository_root(root)?;
        let observed = root.inspect(path)?;
        let relative = path.to_str().ok_or(Error::InvalidCatalog)?;
        self.check_checkout(relative, &observed)?;
        let repository = if let Some(binding) = self
            .bindings
            .iter()
            .find(|b| b.common_directory == observed.common_directory)
        {
            binding.repository
        } else {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or(Error::InvalidName)?;
            let repository = self.create_repository(name)?;
            self.bindings.push(RepositoryBinding {
                repository,
                common_directory: observed.common_directory,
            });
            repository
        };
        let target = self
            .projects
            .iter_mut()
            .find(|p| p.id == project)
            .ok_or(Error::UnknownProject)?;
        if target.repositories.contains(&repository) {
            return Err(Error::AlreadyAssociated);
        }
        target.repositories.push(repository);
        self.checkouts.push(DefaultCheckout {
            project,
            repository,
            path: relative.into(),
            worktree: observed.worktree,
            git_directory: observed.git_directory,
        });
        let (path, identity) = root.binding()?;
        self.root = Some(RootBinding { path, identity });
        // No publication if the selected layout changed across preparation.
        revalidate_checkout(root, Path::new(relative), &observed)?;
        self.check_repository_root(root)?;
        self.validate()?;
        Ok(repository)
    }

    fn check_checkout(&self, path: &str, observed: &WorktreeObservation) -> Result<(), Error> {
        for saved in &self.checkouts {
            if saved.path == path
                || saved.worktree == observed.worktree
                || saved.git_directory == observed.git_directory
            {
                let binding = self
                    .bindings
                    .iter()
                    .find(|b| b.repository == saved.repository)
                    .ok_or(Error::InvalidCatalog)?;
                if saved.worktree != observed.worktree
                    || saved.git_directory != observed.git_directory
                    || binding.common_directory != observed.common_directory
                {
                    return Err(Error::StaleCheckout);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn validate_bindings(&self) -> Result<(), Error> {
        let invalid = || Error::InvalidCatalog;
        let Some(root) = &self.root else {
            return if self.bindings.is_empty() && self.checkouts.is_empty() {
                Ok(())
            } else {
                Err(invalid())
            };
        };
        let relative = root.path.strip_prefix('/').ok_or_else(invalid)?;
        valid_relative(relative)?;
        if root.path.len() > 4096 || root.path.split('/').count() > 64 || self.bindings.is_empty() {
            return Err(invalid());
        }
        let mut registered = HashSet::new();
        let mut common = HashSet::new();
        for binding in &self.bindings {
            if !self.repositories.iter().any(|r| r.id == binding.repository)
                || !registered.insert(binding.repository)
                || !common.insert(binding.common_directory)
            {
                return Err(invalid());
            }
        }
        let mut associations = HashSet::new();
        for checkout in &self.checkouts {
            valid_relative(&checkout.path)?;
            if root.path.len() + 1 + checkout.path.len() > 4096
                || root.path.split('/').count() + checkout.path.split('/').count() > 64
                || !registered.contains(&checkout.repository)
                || !associations.insert((checkout.project, checkout.repository))
                || !self.projects.iter().any(|p| {
                    p.id == checkout.project && p.repositories.contains(&checkout.repository)
                })
            {
                return Err(invalid());
            }
        }
        for project in &self.projects {
            for repository in &project.repositories {
                if registered.contains(repository)
                    && !associations.contains(&(project.id, *repository))
                {
                    return Err(invalid());
                }
            }
        }
        // Every registered repository originates in a saved association.
        if self
            .bindings
            .iter()
            .any(|b| !self.checkouts.iter().any(|c| c.repository == b.repository))
        {
            return Err(invalid());
        }
        let mut paths = HashMap::new();
        let mut worktrees = HashMap::new();
        let mut git_directories = HashMap::new();
        for checkout in &self.checkouts {
            let binding = self
                .bindings
                .iter()
                .find(|b| b.repository == checkout.repository)
                .ok_or_else(invalid)?;
            let value = (
                checkout.worktree,
                checkout.git_directory,
                binding.common_directory,
            );
            if paths
                .insert(&checkout.path, value)
                .is_some_and(|previous| previous != value)
                || worktrees
                    .insert(checkout.worktree, value)
                    .is_some_and(|previous| previous != value)
                || git_directories
                    .insert(checkout.git_directory, value)
                    .is_some_and(|previous| previous != value)
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

pub(crate) fn revalidate_checkout(
    root: &PermittedRoot,
    candidate: &Path,
    observed: &WorktreeObservation,
) -> Result<(), Error> {
    // This candidate already passed inspection. Losing its supported layout is
    // a change during preparation, not an invalid initial request.
    if root.inspect(candidate).map_err(|_| Error::StaleCheckout)? != *observed {
        return Err(Error::StaleCheckout);
    }
    Ok(())
}

fn valid_relative(path: &str) -> Result<(), Error> {
    if path.is_empty()
        || path.len() > 4096
        || path.split('/').count() > 64
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(Error::InvalidCatalog);
    }
    Ok(())
}
