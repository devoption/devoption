use crate::Error;
use std::collections::HashSet;

pub const MAX_PROJECTS: usize = 64;
pub const MAX_REPOSITORIES: usize = 256;
pub const MAX_NAME_BYTES: usize = 128;

/// Stable project identity, independent of its name or repository membership.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProjectId(pub(crate) [u8; 16]);

impl ProjectId {
    pub const fn to_bytes(self) -> [u8; 16] {
        self.0
    }
}

/// Stable repository identity; not a path, Git object ID or authorization grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RepositoryId(pub(crate) [u8; 16]);

impl RepositoryId {
    pub const fn to_bytes(self) -> [u8; 16] {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub(crate) id: ProjectId,
    pub(crate) name: String,
    pub(crate) repositories: Vec<RepositoryId>,
}

impl Project {
    pub fn id(&self) -> ProjectId {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn repositories(&self) -> &[RepositoryId] {
        &self.repositories
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repository {
    pub(crate) id: RepositoryId,
    pub(crate) name: String,
}

impl Repository {
    pub fn id(&self) -> RepositoryId {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Bounded identity metadata. Names need not be unique and are never filenames.
/// Future renderers must escape names; this model does not produce HTML.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub(crate) projects: Vec<Project>,
    pub(crate) repositories: Vec<Repository>,
}

impl Catalog {
    pub fn projects(&self) -> &[Project] {
        &self.projects
    }
    pub fn repositories(&self) -> &[Repository] {
        &self.repositories
    }

    pub fn create_project(&mut self, name: &str) -> Result<ProjectId, Error> {
        name_valid(name)?;
        if self.projects.len() == MAX_PROJECTS {
            return Err(Error::Capacity);
        }
        let id = ProjectId(fresh_id(self.projects.iter().map(|p| p.id.0))?);
        self.projects.push(Project {
            id,
            name: name.into(),
            repositories: Vec::new(),
        });
        Ok(id)
    }

    /// Create identity/display metadata only; no Git repository is registered.
    pub fn create_repository(&mut self, name: &str) -> Result<RepositoryId, Error> {
        name_valid(name)?;
        if self.repositories.len() == MAX_REPOSITORIES {
            return Err(Error::Capacity);
        }
        let id = RepositoryId(fresh_id(self.repositories.iter().map(|r| r.id.0))?);
        self.repositories.push(Repository {
            id,
            name: name.into(),
        });
        Ok(id)
    }

    pub fn associate(&mut self, project: ProjectId, repository: RepositoryId) -> Result<(), Error> {
        if !self.repositories.iter().any(|r| r.id == repository) {
            return Err(Error::UnknownRepository);
        }
        let project = self
            .projects
            .iter_mut()
            .find(|p| p.id == project)
            .ok_or(Error::UnknownProject)?;
        if project.repositories.contains(&repository) {
            return Err(Error::AlreadyAssociated);
        }
        project.repositories.push(repository);
        Ok(())
    }

    #[cfg(unix)]
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.projects.len() > MAX_PROJECTS || self.repositories.len() > MAX_REPOSITORIES {
            return Err(Error::InvalidCatalog);
        }
        let mut repositories = HashSet::new();
        for repository in &self.repositories {
            name_valid(&repository.name).map_err(|_| Error::InvalidCatalog)?;
            if !repositories.insert(repository.id) {
                return Err(Error::InvalidCatalog);
            }
        }
        let mut projects = HashSet::new();
        for project in &self.projects {
            name_valid(&project.name).map_err(|_| Error::InvalidCatalog)?;
            let mut associations = HashSet::new();
            if !projects.insert(project.id)
                || project.repositories.len() > MAX_REPOSITORIES
                || project
                    .repositories
                    .iter()
                    .any(|id| !repositories.contains(id) || !associations.insert(id))
            {
                return Err(Error::InvalidCatalog);
            }
        }
        Ok(())
    }
}

fn name_valid(name: &str) -> Result<(), Error> {
    if name.is_empty()
        || name.len() > MAX_NAME_BYTES
        || name.trim() != name
        || name.chars().any(char::is_control)
    {
        return Err(Error::InvalidName);
    }
    Ok(())
}

fn fresh_id(existing: impl Iterator<Item = [u8; 16]>) -> Result<[u8; 16], Error> {
    let existing: HashSet<_> = existing.collect();
    for _ in 0..8 {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(|_| Error::Randomness)?;
        if !existing.contains(&bytes) {
            return Ok(bytes);
        }
    }
    Err(Error::Randomness)
}
