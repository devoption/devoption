use super::{InspectionError as Error, MAX_COMPONENTS, MAX_PATH, Result};
use rustix::{
    fs::{Mode, OFlags, open, openat},
    io::Errno,
    process::geteuid,
};
use std::{
    fs::{File, Metadata},
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

/// Local device/inode identity, stable only while the filesystem object exists.
/// Linked worktrees share a common-directory identity but have distinct git dirs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FilesystemIdentity {
    pub(crate) device: u64,
    pub(crate) inode: u64,
}

/// An explicit trusted-local-root capability; never a default or browser input.
/// Opening neither creates nor repairs anything. Symlinks in any component are
/// rejected. Root and inspected descendants must belong to this OS user and
/// must not be writable by group/other. Ancestors need not belong to this user.
pub struct PermittedRoot {
    pub(super) path: PathBuf,
    directory: File,
}

const DIRECTORY: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const ENTRY: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

impl PermittedRoot {
    pub(crate) fn binding(&self) -> Result<(String, FilesystemIdentity)> {
        self.check_binding()?;
        Ok((
            self.path.to_str().ok_or(Error::InvalidPath)?.into(),
            identity(&self.directory)?,
        ))
    }

    /// Compare actual directory ancestry, including case aliases, without listing
    /// contents. Both capabilities must have been explicitly opened locally.
    pub fn is_disjoint(&self, other: &Self) -> Result<bool> {
        self.check_binding()?;
        other.check_binding()?;
        self.disjoint_directory(&other.directory)
    }

    pub(crate) fn disjoint_directory(&self, other: &File) -> Result<bool> {
        self.check_binding()?;
        Ok(!ancestor(&self.directory, identity(other)?)?
            && !ancestor(other, identity(&self.directory)?)?)
    }
    pub fn open_absolute(path: &Path) -> Result<Self> {
        let directory = absolute(path)?;
        Ok(Self {
            path: path.into(),
            directory,
        })
    }

    pub(super) fn check_binding(&self) -> Result<()> {
        if identity(&absolute(&self.path)?)? != identity(&self.directory)? {
            return Err(Error::Changed);
        }
        Ok(())
    }

    pub(super) fn directory(&self, path: &Path) -> Result<File> {
        bounded(self.path.join(path).to_str().ok_or(Error::InvalidPath)?)?;
        let mut directory = self.directory.try_clone().map_err(|_| Error::Unavailable)?;
        checked(&directory.metadata().map_err(|_| Error::Unavailable)?, true)?;
        if path.as_os_str().is_empty() {
            return Ok(directory);
        }
        relative(path)?;
        for part in path.components() {
            directory = File::from(
                openat(&directory, part.as_os_str(), DIRECTORY, Mode::empty()).map_err(io_error)?,
            );
            checked(&directory.metadata().map_err(|_| Error::Unavailable)?, true)?;
        }
        Ok(directory)
    }

    pub(super) fn entry(&self, path: &Path) -> Result<Option<File>> {
        bounded(self.path.join(path).to_str().ok_or(Error::InvalidPath)?)?;
        relative(path)?;
        let parent = self.directory(path.parent().ok_or(Error::InvalidPath)?)?;
        let file = match openat(
            &parent,
            path.file_name().ok_or(Error::InvalidPath)?,
            ENTRY,
            Mode::empty(),
        ) {
            Ok(file) => File::from(file),
            Err(Errno::NOENT) => return Ok(None),
            Err(error) => return Err(io_error(error)),
        };
        let metadata = file.metadata().map_err(|_| Error::Unavailable)?;
        checked(&metadata, metadata.is_dir())?;
        Ok(Some(file))
    }

    // Only leading parent components are accepted for metadata references.
    // Their base was already opened without symlinks. Never lexically erase an
    // uninspected `component/..`, which Git might resolve through a symlink.
    pub(super) fn reference(&self, base: &Path, value: &str) -> Result<PathBuf> {
        bounded(value)?;
        let path = Path::new(value);
        if path.is_absolute() {
            relative(Path::new(
                value.strip_prefix('/').ok_or(Error::InvalidPath)?,
            ))?;
            let relative = path
                .strip_prefix(&self.path)
                .map_err(|_| Error::OutsideRoot)?;
            if !relative.as_os_str().is_empty() {
                self::relative(relative)?;
            }
            return Ok(relative.into());
        }
        let mut result = base.to_path_buf();
        let mut parents = true;
        for part in value.split('/') {
            if part == ".." && parents {
                if !result.pop() {
                    return Err(Error::OutsideRoot);
                }
            } else {
                parents = false;
                if part.is_empty() || part == "." || part == ".." {
                    return Err(Error::InvalidPath);
                }
                result.push(part);
            }
        }
        if !result.as_os_str().is_empty() {
            relative(&result)?;
        }
        Ok(result)
    }
}

fn ancestor(leaf: &File, target: FilesystemIdentity) -> Result<bool> {
    let mut directory = leaf.try_clone().map_err(|_| Error::Unavailable)?;
    for _ in 0..MAX_COMPONENTS {
        let current = directory.metadata().map_err(|_| Error::Unavailable)?;
        let current = FilesystemIdentity {
            device: current.dev(),
            inode: current.ino(),
        };
        if current == target {
            return Ok(true);
        }
        let parent =
            File::from(openat(&directory, "..", DIRECTORY, Mode::empty()).map_err(io_error)?);
        let metadata = parent.metadata().map_err(|_| Error::Unavailable)?;
        if metadata.dev() == current.device && metadata.ino() == current.inode {
            return Ok(false);
        }
        directory = parent;
    }
    Err(Error::LimitExceeded)
}

fn absolute(path: &Path) -> Result<File> {
    let value = path.to_str().ok_or(Error::InvalidPath)?;
    let relative = value
        .strip_prefix('/')
        .filter(|value| !value.is_empty())
        .ok_or(Error::InvalidPath)?;
    bounded(value)?;
    self::relative(Path::new(relative))?;
    let mut directory = File::from(open("/", DIRECTORY, Mode::empty()).map_err(io_error)?);
    for part in relative.split('/') {
        directory =
            File::from(openat(&directory, part, DIRECTORY, Mode::empty()).map_err(io_error)?);
    }
    checked(&directory.metadata().map_err(|_| Error::Unavailable)?, true)?;
    Ok(directory)
}

pub(super) fn relative(path: &Path) -> Result<()> {
    let value = path.to_str().ok_or(Error::InvalidPath)?;
    bounded(value)?;
    if value
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::InvalidPath);
    }
    Ok(())
}

pub(super) fn bounded(value: &str) -> Result<()> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(Error::InvalidPath);
    }
    if value.len() > MAX_PATH || value.split('/').count() > MAX_COMPONENTS {
        return Err(Error::LimitExceeded);
    }
    Ok(())
}

fn checked(metadata: &Metadata, directory: bool) -> Result<()> {
    if metadata.uid() != geteuid().as_raw()
        || metadata.mode() & 0o022 != 0
        || metadata.nlink() == 0
        || (directory && !metadata.is_dir())
        || (!directory && (!metadata.is_file() || metadata.nlink() != 1))
    {
        return Err(Error::UnsafeEntry);
    }
    Ok(())
}

pub(super) fn identity(file: &File) -> Result<FilesystemIdentity> {
    let metadata = file.metadata().map_err(|_| Error::Unavailable)?;
    checked(&metadata, true)?;
    Ok(FilesystemIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

pub(super) fn text(file: File, limit: usize) -> Result<String> {
    String::from_utf8(bytes(file, limit)?).map_err(|_| Error::InvalidLayout)
}

pub(super) fn bytes(mut file: File, limit: usize) -> Result<Vec<u8>> {
    let before = file.metadata().map_err(|_| Error::Unavailable)?;
    checked(&before, false)?;
    if before.len() > limit as u64 {
        return Err(Error::LimitExceeded);
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Unavailable)?;
    if bytes.len() > limit {
        return Err(Error::LimitExceeded);
    }
    let after = file.metadata().map_err(|_| Error::Unavailable)?;
    checked(&after, false)?;
    if before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(Error::Changed);
    }
    Ok(bytes)
}

fn io_error(error: Errno) -> Error {
    match error {
        Errno::LOOP | Errno::NOTDIR => Error::UnsafeEntry,
        _ => Error::Unavailable,
    }
}
