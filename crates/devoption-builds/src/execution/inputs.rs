//! Operator-prepared input identity. Does not download dependencies or execute code.
use devoption_registry::Sha256;
use std::{
    fs,
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

const MAX_FILES: usize = 16384;
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub(super) fn digest_tree(root: &Path) -> io::Result<Sha256> {
    Ok(describe_tree(root)?.0)
}

pub(super) fn describe_tree(root: &Path) -> io::Result<(Sha256, u64)> {
    if !fs::symlink_metadata(root)?.is_dir() || fs::canonicalize(root)? != root {
        return Err(io::Error::other(
            "prepared root must be a canonical directory",
        ));
    }
    let mut files = Vec::new();
    collect(root, root, &mut files)?;
    files.sort();
    let mut manifest = Vec::new();
    let mut total = 0;
    for path in files {
        let metadata = fs::symlink_metadata(&path)?;
        total += metadata.len();
        if total > MAX_BYTES || metadata.len() > 256 * 1024 * 1024 {
            return Err(io::Error::other("prepared inputs exceed byte limit"));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(io::Error::other)?
            .to_str()
            .ok_or_else(|| io::Error::other("input path requires UTF-8"))?;
        let bytes = read_regular(&path, metadata.len())?;
        manifest.extend_from_slice(&(relative.len() as u64).to_be_bytes());
        manifest.extend_from_slice(relative.as_bytes());
        manifest.extend_from_slice(Sha256::digest(&bytes).bytes());
        manifest.extend_from_slice(&(metadata.mode() & 0o777).to_be_bytes());
    }
    Ok((Sha256::digest(&manifest), total))
}

fn collect(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    if directory
        .strip_prefix(root)
        .map_err(io::Error::other)?
        .components()
        .count()
        > 32
    {
        return Err(io::Error::other("prepared inputs exceed depth limit"));
    }
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            collect(root, &path, files)?;
        } else if metadata.is_file() {
            if files.len() >= MAX_FILES {
                return Err(io::Error::other("prepared inputs exceed file limit"));
            }
            files.push(path);
        } else {
            return Err(io::Error::other(
                "prepared inputs must contain regular files only",
            ));
        }
    }
    Ok(())
}

pub(super) fn read_regular(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    use rustix::fs::{Mode, OFlags, open};
    let file = fs::File::from(open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    let before = file.metadata()?;
    if !before.is_file() || before.len() > max {
        return Err(io::Error::other("input is not a bounded regular file"));
    }
    let mut bytes = Vec::new();
    (&file).take(max + 1).read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if bytes.len() as u64 != before.len()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(io::Error::other("input changed while reading"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn digest_binds_paths_bytes_modes_and_extra_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let source = root.join("input");
        fs::write(&source, b"first").unwrap();
        let original = digest_tree(&root).unwrap();
        fs::write(&source, b"other").unwrap();
        assert!(digest_tree(&root).unwrap() != original);
        fs::write(&source, b"first").unwrap();
        assert!(digest_tree(&root).unwrap() == original);
        fs::set_permissions(&source, fs::Permissions::from_mode(0o500)).unwrap();
        assert!(digest_tree(&root).unwrap() != original);
        let executable = digest_tree(&root).unwrap();
        fs::rename(&source, root.join("renamed")).unwrap();
        assert!(digest_tree(&root).unwrap() != executable);
        fs::write(root.join("extra"), b"").unwrap();
        assert!(digest_tree(&root).unwrap() != executable);
    }

    #[test]
    fn reject_links_and_oversized_reads() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::write(root.join("file"), b"bytes").unwrap();
        assert!(read_regular(&root.join("file"), 4).is_err());
        symlink(root.join("file"), root.join("link")).unwrap();
        assert!(read_regular(&root.join("link"), 5).is_err());
        assert!(digest_tree(&root).is_err());
        fs::remove_file(root.join("link")).unwrap();
        fs::create_dir(root.join("directory")).unwrap();
        symlink(root.join("directory"), root.join("link")).unwrap();
        assert!(digest_tree(&root.join("link")).is_err());
    }
}
