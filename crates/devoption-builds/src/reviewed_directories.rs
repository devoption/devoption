//! Read-only, retained directory observations for one private same-account work tree.
//! No creation, repair, cleanup, runner capability or complete-context verification.

use crate::review_context::{Directory, DirectoryRole, ReviewContext};
use rustix::{
    fs::{Mode, OFlags, open, openat},
    process::geteuid,
};
use std::{fmt, fs::File, os::unix::fs::MetadataExt};

const MAX_COMPONENTS: usize = 64;
const FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// Private pinned handles, never exposed as writable directory or runner authority.
/// A successful check is an observation, not isolation from a same-user writer.
/// Drop closes all descriptors; it never removes any directory.
pub struct ReviewedDirectories {
    expected: Vec<Directory>,
    handles: Vec<File>,
    uid: u32,
}

impl ReviewedDirectories {
    /// Inspect supplied review paths without changing them. Supports only seven
    /// distinct roles beneath Work, owned by the current non-root effective UID.
    /// Multi-account and separate-root layouts require a separately reviewed verifier.
    pub fn open(context: &ReviewContext) -> Result<Self, Error> {
        let uid = geteuid().as_raw();
        if uid == 0 || context.details().execution_uid != uid {
            return Err(Error::UnsupportedOwner);
        }
        let expected = &context.details().directories;
        let work = expected.first().ok_or(Error::UnsupportedLayout)?;
        if work.role != DirectoryRole::Work {
            return Err(Error::UnsupportedLayout);
        }
        for (index, directory) in expected.iter().enumerate() {
            if directory.owner_uid != uid {
                return Err(Error::UnsupportedOwner);
            }
            if directory.path.split('/').skip(1).count() > MAX_COMPONENTS {
                return Err(Error::UnsupportedLayout);
            }
            if index > 0 {
                relative(&work.path, &directory.path)?;
                for other in &expected[1..index] {
                    if nested(&other.path, &directory.path) || nested(&directory.path, &other.path)
                    {
                        return Err(Error::UnsupportedLayout);
                    }
                }
            }
        }
        let root = absolute(&work.path)?;
        check(&root, work, uid)?;
        let mut handles = vec![root];
        for directory in &expected[1..] {
            let file = descendant(
                &handles[0],
                relative(&work.path, &directory.path)?,
                work.device,
                uid,
            )?;
            check(&file, directory, uid)?;
            handles.push(file);
        }
        let result = Self {
            expected: expected.clone(),
            handles,
            uid,
        };
        // Recheck names after all handles have been pinned; never accept a partly
        // opened snapshot solely because each name matched at different times.
        result.revalidate()?;
        Ok(result)
    }

    /// Compare both retained objects and current no-follow path bindings against
    /// the original observations. A later rename or permission change fails closed.
    /// This is not an atomic snapshot against arbitrary concurrent filesystem writes.
    pub fn revalidate(&self) -> Result<(), Error> {
        if geteuid().as_raw() != self.uid {
            return Err(Error::UnsupportedOwner);
        }
        for (file, expected) in self.handles.iter().zip(&self.expected) {
            check(file, expected, self.uid)?;
        }
        self.root_binding()?;
        let work = &self.expected[0];
        for expected in &self.expected[1..] {
            let current = descendant(
                &self.handles[0],
                relative(&work.path, &expected.path)?,
                work.device,
                self.uid,
            )?;
            check(&current, expected, self.uid)?;
        }
        self.root_binding()?;
        for (file, expected) in self.handles.iter().zip(&self.expected) {
            check(file, expected, self.uid)?;
        }
        Ok(())
    }

    fn root_binding(&self) -> Result<(), Error> {
        let root = absolute(&self.expected[0].path)?;
        check(&root, &self.expected[0], self.uid)
    }
}

fn nested(parent: &str, child: &str) -> bool {
    child == parent
        || child
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
}
fn relative<'a>(root: &str, path: &'a str) -> Result<&'a str, Error> {
    path.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .filter(|rest| !rest.is_empty())
        .ok_or(Error::UnsupportedLayout)
}
fn absolute(path: &str) -> Result<File, Error> {
    // ReviewContext already checks lexical normalization, but keep this I/O
    // boundary defensive and bounded independently of its caller.
    if !crate::absolute_path(path) || path.split('/').skip(1).count() > MAX_COMPONENTS {
        return Err(Error::UnsupportedLayout);
    }
    let mut directory =
        File::from(open("/", FLAGS, Mode::empty()).map_err(|_| Error::Unavailable)?);
    for component in path[1..].split('/') {
        directory = File::from(
            openat(&directory, component, FLAGS, Mode::empty()).map_err(|_| Error::Unavailable)?,
        );
    }
    Ok(directory)
}
fn descendant(root: &File, relative: &str, device: u64, uid: u32) -> Result<File, Error> {
    let mut directory = root.try_clone().map_err(|_| Error::Unavailable)?;
    for component in relative.split('/') {
        directory = File::from(
            openat(&directory, component, FLAGS, Mode::empty()).map_err(|_| Error::Unavailable)?,
        );
        let metadata = directory.metadata().map_err(|_| Error::Unavailable)?;
        if !metadata.is_dir()
            || metadata.dev() != device
            || metadata.uid() != uid
            || !matches!(metadata.mode() & 0o7777, 0o500 | 0o700)
            || metadata.nlink() == 0
        {
            return Err(Error::Changed);
        }
    }
    Ok(directory)
}
fn check(file: &File, expected: &Directory, uid: u32) -> Result<(), Error> {
    let metadata = file.metadata().map_err(|_| Error::Unavailable)?;
    if !metadata.is_dir()
        || metadata.nlink() == 0
        || metadata.dev() != expected.device
        || metadata.ino() != expected.inode
        || metadata.uid() != uid
        || metadata.uid() != expected.owner_uid
        || metadata.mode() & 0o7777 != expected.mode
    {
        return Err(Error::Changed);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    UnsupportedOwner,
    UnsupportedLayout,
    Changed,
    Unavailable,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnsupportedOwner => {
                "directory verification requires the current unprivileged account"
            }
            Self::UnsupportedLayout => "reviewed directory layout is unsupported",
            Self::Changed => "reviewed directory metadata changed",
            Self::Unavailable => "reviewed directory is unavailable",
        })
    }
}
impl std::error::Error for Error {}

#[cfg(test)]
mod tests;
