//! Project/repository identity and catalog persistence, independent of HTTP and auth.
//!
//! Repository metadata and validated location bindings remain separate.
//! The Unix worktree inspector accepts only paths under an explicit permitted root,
//! runs no commands and grants no execution permissions.
//! Unix storage is explicit and private. The server exposes authenticated project
//! creation, catalog reads and opt-in validated repository attachment.

mod model;
pub use model::{
    Catalog, MAX_NAME_BYTES, MAX_PROJECTS, MAX_REPOSITORIES, Project, ProjectId, Repository,
    RepositoryId,
};

#[cfg(unix)]
mod codec;
#[cfg(unix)]
mod store;
#[cfg(unix)]
pub use store::{CatalogDirectory, CatalogStore, read_catalog};

#[cfg(unix)]
pub mod registration;
#[cfg(unix)]
pub mod worktree;

use std::{fmt, io};

/// Errors never include catalog names, filesystem paths or serialized content.
#[derive(Debug)]
pub enum Error {
    InvalidName,
    Capacity,
    UnknownProject,
    UnknownRepository,
    AlreadyAssociated,
    CheckoutRequired,
    RootMismatch,
    StaleCheckout,
    #[cfg(unix)]
    Inspection(worktree::InspectionError),
    Randomness,
    InvalidCatalog,
    UnsupportedVersion,
    UnsafePath,
    UnsafeDirectory,
    UnsafeFile,
    Busy,
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidName => {
                "name must be trimmed, nonempty, bounded and free of control characters"
            }
            Self::Capacity => "catalog capacity reached",
            Self::UnknownProject => "project does not exist in this catalog",
            Self::UnknownRepository => "repository does not exist in this catalog",
            Self::AlreadyAssociated => "repository is already associated with this project",
            Self::CheckoutRequired => {
                "registered repositories require a validated default checkout"
            }
            Self::RootMismatch => {
                "repository root does not match the saved catalog or overlaps application data"
            }
            Self::StaleCheckout => {
                "saved repository checkout has changed; explicit reattachment is required"
            }
            #[cfg(unix)]
            Self::Inspection(_) => "repository checkout could not be validated",
            Self::Randomness => "identity generation failed",
            Self::InvalidCatalog => "project catalog is invalid",
            Self::UnsupportedVersion => "project catalog version is unsupported",
            Self::UnsafePath => {
                "catalog directory requires an absolute path without parent traversal"
            }
            Self::UnsafeDirectory => "catalog directory must be private and owned by this OS user",
            Self::UnsafeFile => "catalog file must be a private regular file with one link",
            Self::Busy => "project catalog already has a writer",
            Self::Io(_) => "project catalog I/O failed",
        })
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(unix)]
impl From<worktree::InspectionError> for Error {
    fn from(error: worktree::InspectionError) -> Self {
        Self::Inspection(error)
    }
}

#[cfg(unix)]
impl From<rustix::io::Errno> for Error {
    fn from(error: rustix::io::Errno) -> Self {
        Self::Io(error.into())
    }
}

#[cfg(test)]
mod tests;
