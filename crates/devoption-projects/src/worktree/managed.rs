//! Explicit authority to create new empty repositories, separate from inspection.
use super::{InspectionError, PermittedRoot};
use crate::Error;
use rustix::{
    fs::{Mode, OFlags, mkdirat, openat},
    io::Errno,
};
use std::{
    fs::File,
    io::Write,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Selected only by trusted local startup configuration, never browser input.
/// Existing entries are never overwritten, repaired or removed.
pub struct ManagedRoot {
    root: Arc<PermittedRoot>,
}
impl ManagedRoot {
    pub fn open_absolute(path: &Path) -> Result<Self, Error> {
        Self::from_permitted(Arc::new(PermittedRoot::open_absolute(path)?))
    }
    /// Explicitly grant creation authority for an already selected root.
    pub fn from_permitted(root: Arc<PermittedRoot>) -> Result<Self, Error> {
        let managed = Self { root };
        managed.directory()?;
        Ok(managed)
    }
    pub fn permitted(&self) -> &PermittedRoot {
        &self.root
    }

    fn directory(&self) -> Result<File, Error> {
        self.root.check_binding()?;
        let directory = self.root.directory(Path::new(""))?;
        if directory.metadata()?.mode() & 0o077 != 0 {
            return Err(InspectionError::UnsafeEntry.into());
        }
        Ok(directory)
    }

    /// Validate before filesystem work. A name is a single portable component.
    pub(crate) fn validate_name(name: &str) -> Result<(), Error> {
        Self::validate_component(name, 64)
    }

    fn validate_component(name: &str, limit: usize) -> Result<(), Error> {
        if name.is_empty()
            || name.len() > limit
            || !name.as_bytes()[0].is_ascii_alphanumeric()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(Error::InvalidName);
        }
        Ok(())
    }

    /// Initialize only a newly created directory. Errors can leave unpublished
    /// private files; preserve them for explicit inspection, never delete on error.
    pub(crate) fn initialize(&self, name: &str) -> Result<PathBuf, Error> {
        // Internal storage names include a stable project ID. Browser names are
        // separately limited to 64 bytes before this filesystem operation.
        Self::validate_component(name, crate::MAX_NAME_BYTES)?;
        let longest = self.root.path.join(name).join(".git/refs/heads");
        super::filesystem::bounded(longest.to_str().ok_or(InspectionError::InvalidPath)?)?;
        let root = self.directory()?;
        match mkdirat(&root, name, Mode::RWXU) {
            Ok(()) => {}
            Err(Errno::EXIST) => return Err(Error::RepositoryExists),
            Err(error) => return Err(error.into()),
        }
        let checkout = directory(&root, name)?;
        mkdirat(&checkout, ".git", Mode::RWXU)?;
        let git = directory(&checkout, ".git")?;
        for name in ["objects", "refs"] {
            mkdirat(&git, name, Mode::RWXU)?;
        }
        let objects = directory(&git, "objects")?;
        let refs = directory(&git, "refs")?;
        for name in ["heads", "tags"] {
            mkdirat(&refs, name, Mode::RWXU)?;
            directory(&refs, name)?.sync_all()?;
        }
        write(&git, "HEAD", b"ref: refs/heads/main\n")?;
        write(
            &git,
            "config",
            b"[core]\nrepositoryformatversion = 0\nfilemode = true\nbare = false\n",
        )?;
        for directory in [&objects, &refs, &git, &checkout, &root] {
            directory.sync_all()?;
        }
        self.directory()?;
        let path = PathBuf::from(name);
        self.root.inspect(&path)?;
        Ok(path)
    }
}
fn directory(parent: &File, name: &str) -> Result<File, Error> {
    Ok(File::from(openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?))
}
fn write(parent: &File, name: &str, bytes: &[u8]) -> Result<(), Error> {
    let mut file = File::from(openat(
        parent,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )?);
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
