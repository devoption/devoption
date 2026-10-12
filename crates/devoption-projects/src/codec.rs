use crate::{
    Catalog, Error, MAX_NAME_BYTES, MAX_PROJECTS, MAX_REPOSITORIES, Project, ProjectId, Repository,
    RepositoryId,
};

const MAGIC: &[u8; 8] = b"DVPROJS\0";
const VERSION: u8 = 1;
pub(crate) const MAX_BYTES: usize = 512 * 1024;

pub(crate) fn encode(catalog: &Catalog) -> Result<Vec<u8>, Error> {
    catalog.validate()?;
    let mut out = MAGIC.to_vec();
    out.push(VERSION);
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
    if out.len() > MAX_BYTES {
        return Err(Error::InvalidCatalog);
    }
    Ok(out)
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
    if input.take(1)? != [VERSION] {
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
        let count = self.count(MAX_NAME_BYTES)?;
        std::str::from_utf8(self.take(count)?)
            .map(str::to_owned)
            .map_err(|_| Error::InvalidCatalog)
    }
}
