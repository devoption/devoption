//! Explicit registered-repository adapter; no approval, extraction or execution.
use super::{Object, ObjectKind, SourceSnapshot, verify};
use devoption_projects::{
    Catalog, ProjectId, RepositoryId,
    worktree::{GitSourceKind, PermittedRoot},
};
use devoption_registry::RegistryId;
use std::fmt;

/// Read a selected current HEAD through the saved association, verify all object
/// links, then compare every verified file against the anchored checkout bytes.
/// The caller supplies fresh catalog/registry/revision selection; this returns no
/// authorization and cannot guarantee an atomic observation against external writers.
pub fn capture(
    catalog: &Catalog,
    project: ProjectId,
    repository: RepositoryId,
    root: &PermittedRoot,
    selected_commit: &str,
    registry: RegistryId,
) -> Result<SourceSnapshot, Error> {
    let raw = catalog
        .source_objects(project, repository, root, selected_commit)
        .map_err(Error::Repository)?;
    let objects: Vec<_> = raw
        .objects()
        .iter()
        .map(|object| Object {
            kind: match object.kind() {
                GitSourceKind::Tree => ObjectKind::Tree,
                GitSourceKind::Blob => ObjectKind::Blob,
            },
            bytes: object.bytes(),
        })
        .collect();
    let source =
        verify(registry, selected_commit, raw.commit(), &objects).map_err(Error::Snapshot)?;
    let files: Vec<_> = source
        .files()
        .iter()
        .map(|file| (file.path(), file.bytes()))
        .collect();
    catalog
        .check_source_files(project, repository, root, selected_commit, &files)
        .map_err(Error::Repository)?;
    Ok(source)
}

/// No diagnostic includes repository names, paths, object bytes or OS errors.
pub enum Error {
    Repository(devoption_projects::Error),
    Snapshot(super::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(error) => {
                use devoption_projects::worktree::InspectionError;
                match error {
                    devoption_projects::Error::Inspection(InspectionError::UncommittedSource) => f
                        .write_str(
                            "source has uncommitted changes or differs from the selected commit",
                        ),
                    devoption_projects::Error::Inspection(
                        InspectionError::SelectedCommitMismatch,
                    ) => f.write_str("selected commit no longer matches the checkout HEAD"),
                    _ => error.fmt(f),
                }
            }
            Self::Snapshot(error) => error.fmt(f),
        }
    }
}
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for Error {}

#[cfg(test)]
mod tests;
