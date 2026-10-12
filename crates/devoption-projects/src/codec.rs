use crate::{
    Catalog, Error, MAX_NAME_BYTES, MAX_PROJECTS, MAX_REPOSITORIES, Project, ProjectId, Repository,
    RepositoryId,
};

const MAGIC: &[u8; 8] = b"DVPROJS\0";
pub(crate) const MAX_BYTES: usize = 512 * 1024;

pub(crate) fn encode(catalog: &Catalog) -> Result<Vec<u8>, Error> {
    catalog.validate()?;
    let mut out = MAGIC.to_vec();
    out.push(if catalog.root.is_some() { 2 } else { 1 });
    number(&mut out, catalog.repositories.len());
    for repository in &catalog.repositories {
        out.extend_from_slice(&repository.id.0);
        name(&mut out, &repository.name);
    }
    number(&mut out, catalog.projects.len());
    for project in &catalog.projects {
        out.extend_from_slice(&project.id.0);
        name(&mut out, &project.name);
        number(&mut out, project.repositories.len());
        for repository in &project.repositories {
            out.extend_from_slice(&repository.0);
        }
    }
    if let Some(root) = &catalog.root {
        name(&mut out, &root.path);
        identity(&mut out, root.identity);
        number(&mut out, catalog.bindings.len());
        for binding in &catalog.bindings {
            out.extend_from_slice(&binding.repository.0);
            identity(&mut out, binding.common_directory);
        }
        number(&mut out, catalog.checkouts.len());
        for checkout in &catalog.checkouts {
            // Reject before growing a snapshot beyond the on-disk bound.
            if out.len() + 66 + checkout.path.len() > MAX_BYTES {
                return Err(Error::Capacity);
            }
            out.extend_from_slice(&checkout.project.0);
            out.extend_from_slice(&checkout.repository.0);
            name(&mut out, &checkout.path);
            identity(&mut out, checkout.worktree);
            identity(&mut out, checkout.git_directory);
        }
    }
    if out.len() > MAX_BYTES {
        return Err(Error::Capacity);
    }
    Ok(out)
}

fn identity(out: &mut Vec<u8>, identity: crate::worktree::FilesystemIdentity) {
    out.extend_from_slice(&identity.device.to_le_bytes());
    out.extend_from_slice(&identity.inode.to_le_bytes());
}

fn number(out: &mut Vec<u8>, value: usize) {
    out.extend_from_slice(&(value as u16).to_le_bytes());
}

fn name(out: &mut Vec<u8>, name: &str) {
    number(out, name.len());
    out.extend_from_slice(name.as_bytes());
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Catalog, Error> {
    if bytes.len() > MAX_BYTES {
        return Err(Error::InvalidCatalog);
    }
    let mut input = Input(bytes);
    if input.take(8)? != MAGIC {
        return Err(Error::InvalidCatalog);
    }
    let version = input.take(1)?[0];
    if !matches!(version, 1 | 2) {
        return Err(Error::UnsupportedVersion);
    }
    let mut catalog = Catalog::default();
    for _ in 0..input.count(MAX_REPOSITORIES)? {
        catalog.repositories.push(Repository {
            id: RepositoryId(input.id()?),
            name: input.name()?,
        });
    }
    for _ in 0..input.count(MAX_PROJECTS)? {
        let id = ProjectId(input.id()?);
        let name = input.name()?;
        let mut repositories = Vec::new();
        for _ in 0..input.count(MAX_REPOSITORIES)? {
            repositories.push(RepositoryId(input.id()?));
        }
        catalog.projects.push(Project {
            id,
            name,
            repositories,
        });
    }
    if version == 2 {
        use crate::registration::{DefaultCheckout, RepositoryBinding, RootBinding};
        catalog.root = Some(RootBinding {
            path: input.string(4096)?,
            identity: input.identity()?,
        });
        for _ in 0..input.count(MAX_REPOSITORIES)? {
            catalog.bindings.push(RepositoryBinding {
                repository: RepositoryId(input.id()?),
                common_directory: input.identity()?,
            });
        }
        for _ in 0..input.count(MAX_PROJECTS * MAX_REPOSITORIES)? {
            catalog.checkouts.push(DefaultCheckout {
                project: ProjectId(input.id()?),
                repository: RepositoryId(input.id()?),
                path: input.string(4096)?,
                worktree: input.identity()?,
                git_directory: input.identity()?,
            });
        }
    }
    if !input.0.is_empty() {
        return Err(Error::InvalidCatalog);
    }
    catalog.validate()?;
    Ok(catalog)
}

struct Input<'a>(&'a [u8]);

impl<'a> Input<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let (head, tail) = self
            .0
            .split_at_checked(count)
            .ok_or(Error::InvalidCatalog)?;
        self.0 = tail;
        Ok(head)
    }
    fn count(&mut self, max: usize) -> Result<usize, Error> {
        let bytes = self.take(2)?;
        let count = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
        if count > max {
            return Err(Error::InvalidCatalog);
        }
        Ok(count)
    }
    fn id(&mut self) -> Result<[u8; 16], Error> {
        Ok(self.take(16)?.try_into().expect("fixed input length"))
    }
    fn name(&mut self) -> Result<String, Error> {
        self.string(MAX_NAME_BYTES)
    }
    fn identity(&mut self) -> Result<crate::worktree::FilesystemIdentity, Error> {
        Ok(crate::worktree::FilesystemIdentity {
            device: u64::from_le_bytes(self.take(8)?.try_into().expect("fixed input length")),
            inode: u64::from_le_bytes(self.take(8)?.try_into().expect("fixed input length")),
        })
    }
    fn string(&mut self, max: usize) -> Result<String, Error> {
        let count = self.count(max)?;
        std::str::from_utf8(self.take(count)?)
            .map(str::to_owned)
            .map_err(|_| Error::InvalidCatalog)
    }
}
