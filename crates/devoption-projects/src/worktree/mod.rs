//! Read-only, root-bounded inspection of a conservative Git worktree layout.
//!
//! Only a trusted local caller may select a PermittedRoot; never construct it
//! from a browser-submitted root. The registration adapter receives this capability
//! from explicit local startup configuration.
//! Inspection is not an execution grant or proof of repository/object integrity.

mod filesystem;
mod format;

pub use filesystem::{FilesystemIdentity, PermittedRoot};
use std::{
    fmt,
    path::{Path, PathBuf},
};

/// Fixed errors disclose neither paths nor file contents, including via Debug.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectionError {
    InvalidPath,
    OutsideRoot,
    UnsafeEntry,
    LimitExceeded,
    InvalidLayout,
    UnsupportedLayout,
    Changed,
    Unavailable,
}

impl fmt::Display for InspectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidPath => "worktree path is not a bounded normalized path",
            Self::OutsideRoot => "Git metadata leaves the permitted root",
            Self::UnsafeEntry => "worktree entry has an unsafe type, owner, link or permission",
            Self::LimitExceeded => "worktree inspection limit exceeded",
            Self::InvalidLayout => "Git worktree metadata is missing or invalid",
            Self::UnsupportedLayout => "Git worktree layout is not supported",
            Self::Changed => "worktree location or metadata changed during inspection",
            Self::Unavailable => "worktree metadata is unavailable",
        })
    }
}

impl std::error::Error for InspectionError {}

type Result<T> = std::result::Result<T, InspectionError>;
const MAX_PATH: usize = 4096;
const MAX_COMPONENTS: usize = 64;
const MAX_CONFIG: usize = 16 * 1024;

/// Maximum immediate root entries considered in one picker request.
pub const MAX_CHECKOUT_ENTRIES: usize = 64;

/// Display-only choices. Attachment must inspect the selected path again.
#[derive(Debug, PartialEq, Eq)]
pub struct CheckoutOptions {
    pub paths: Vec<String>,
    pub limited: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorktreeKind {
    Main,
    Linked,
}

/// A point-in-time observation. Paths are display data, never authority for a
/// later operation; re-inspect before use. Escape paths before rendering them.
/// Filesystem identities detect aliases while their inodes exist; they are not
/// persisted RepositoryId values, content hashes or approvals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeObservation {
    pub path: PathBuf,
    pub kind: WorktreeKind,
    pub worktree: FilesystemIdentity,
    pub git_directory: FilesystemIdentity,
    pub common_directory: FilesystemIdentity,
}

impl PermittedRoot {
    /// List supported immediate child checkouts without recursion or Git commands.
    /// Invalid, unsafe and unsupported entries are omitted. The limit counts all
    /// root entries, not just checkouts; a partial list makes no completeness claim.
    pub fn checkout_options(&self) -> Result<CheckoutOptions> {
        self.check_binding()?;
        let directory = self.directory(Path::new(""))?;
        let entries =
            rustix::fs::Dir::read_from(&directory).map_err(|_| InspectionError::Unavailable)?;
        let mut paths = Vec::new();
        let mut seen = 0;
        let mut limited = false;
        for entry in entries {
            let entry = entry.map_err(|_| InspectionError::Unavailable)?;
            let name = entry.file_name().to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            if seen == MAX_CHECKOUT_ENTRIES {
                limited = true;
                break;
            }
            seen += 1;
            let Ok(name) = std::str::from_utf8(name) else {
                continue;
            };
            if name.trim() == name
                && name.len() <= crate::MAX_NAME_BYTES
                && self.inspect(Path::new(name)).is_ok()
            {
                paths.push(name.to_owned());
            }
        }
        self.check_binding()?;
        paths.sort();
        Ok(CheckoutOptions { paths, limited })
    }

    /// Inspect one relative directory, without parent search or enumeration.
    /// Ordinary .git directories and linked gitfiles are supported only when
    /// all inspected metadata is contained in this explicitly selected root.
    pub fn inspect(&self, candidate: &Path) -> Result<WorktreeObservation> {
        filesystem::relative(candidate)?;
        self.check_binding()?;
        let worktree = self.directory(candidate)?;
        let marker = candidate.join(".git");
        let entry = self.entry(&marker)?.ok_or(InspectionError::InvalidLayout)?;
        let metadata = entry.metadata().map_err(|_| InspectionError::Unavailable)?;
        let (kind, git_path, common_path) = if metadata.is_dir() {
            if self.entry(&marker.join("commondir"))?.is_some() {
                return Err(InspectionError::UnsupportedLayout);
            }
            (WorktreeKind::Main, marker.clone(), marker)
        } else {
            let pointer = filesystem::text(entry, MAX_PATH)?;
            let pointer = format::line(&pointer)?
                .strip_prefix("gitdir: ")
                .ok_or(InspectionError::InvalidLayout)?;
            let git_path = self.reference(candidate, pointer)?;
            let _git = self.directory(&git_path)?;
            let common = self.required(&git_path.join("commondir"), MAX_PATH)?;
            let common_path = self.reference(&git_path, format::line(&common)?)?;
            // Support the documented linked-worktree layout, not arbitrary
            // gitfile redirection (separate git dirs and submodules are deferred).
            let parent = git_path.parent().ok_or(InspectionError::InvalidLayout)?;
            if parent != common_path.join("worktrees") {
                return Err(InspectionError::UnsupportedLayout);
            }
            let backlink = self.required(&git_path.join("gitdir"), MAX_PATH)?;
            if self.reference(&git_path, format::line(&backlink)?)? != marker {
                return Err(InspectionError::InvalidLayout);
            }
            if self.entry(&common_path.join("commondir"))?.is_some() {
                return Err(InspectionError::UnsupportedLayout);
            }
            (WorktreeKind::Linked, git_path, common_path)
        };
        let git = self.directory(&git_path)?;
        let common = self.directory(&common_path)?;
        self.directory(&common_path.join("objects"))?;
        self.directory(&common_path.join("refs"))?;
        // Never follow object-store alternates or per-worktree config. Other
        // files (hooks, index, objects, refs, working files) are never read.
        for path in [&git_path, &common_path] {
            if self.entry(&path.join("config.worktree"))?.is_some() {
                return Err(InspectionError::UnsupportedLayout);
            }
        }
        let info = common_path.join("objects/info");
        if self.entry(&info)?.is_some() {
            self.directory(&info)?;
            for name in ["alternates", "http-alternates"] {
                if self.entry(&info.join(name))?.is_some() {
                    return Err(InspectionError::UnsupportedLayout);
                }
            }
        }
        format::head(&self.required(&git_path.join("HEAD"), MAX_PATH)?)?;
        format::config(&self.required(&common_path.join("config"), MAX_CONFIG)?)?;
        let result = WorktreeObservation {
            path: self.path.join(candidate),
            kind,
            worktree: filesystem::identity(&worktree)?,
            git_directory: filesystem::identity(&git)?,
            common_directory: filesystem::identity(&common)?,
        };
        self.check_binding()?;
        for (path, expected) in [
            (candidate, result.worktree),
            (&git_path, result.git_directory),
            (&common_path, result.common_directory),
        ] {
            if filesystem::identity(&self.directory(path)?)? != expected {
                return Err(InspectionError::Changed);
            }
        }
        Ok(result)
    }

    fn required(&self, path: &Path, limit: usize) -> Result<String> {
        filesystem::text(
            self.entry(path)?.ok_or(InspectionError::InvalidLayout)?,
            limit,
        )
    }
}

#[cfg(test)]
mod tests;
