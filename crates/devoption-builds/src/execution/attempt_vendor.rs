//! Private, newly allocated attempt inputs. No shared vendor store or replacement.
use crate::library::vendor::VendorInputs;
use devoption_registry::Sha256;
use rustix::fs::{self as rfs, Dir, Mode, OFlags};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
    path::Path,
    sync::Arc,
};
const DIR: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
pub(super) struct Staged {
    root: File,
    parent: File,
    vendor: Arc<VendorInputs>,
    config: Vec<u8>,
    directories: BTreeMap<String, (u64, u64)>,
}
fn error() -> io::Error {
    io::Error::other("attempt dependency inputs unavailable or changed")
}
fn id(file: &File) -> io::Result<(u64, u64)> {
    let m = file.metadata()?;
    Ok((m.dev(), m.ino()))
}
fn private(file: &File) -> io::Result<()> {
    let m = file.metadata()?;
    if !m.is_dir() || m.uid() != rustix::process::geteuid().as_raw() || m.mode() & 0o7777 != 0o700 {
        return Err(error());
    }
    Ok(())
}
fn child(parent: &File, name: &str) -> io::Result<File> {
    let file = File::from(rfs::openat(parent, name, DIR, Mode::empty())?);
    private(&file)?;
    Ok(file)
}
fn descend(root: &File, path: &str, create: bool) -> io::Result<File> {
    let mut file = root.try_clone()?;
    if !path.is_empty() {
        for part in path.split('/') {
            if part.is_empty() || part == "." || part == ".." {
                return Err(error());
            }
            if create {
                match rfs::mkdirat(&file, part, Mode::RUSR | Mode::WUSR | Mode::XUSR) {
                    Ok(()) => {}
                    Err(rustix::io::Errno::EXIST) => {}
                    Err(e) => return Err(e.into()),
                }
            }
            file = child(&file, part)?;
        }
    }
    Ok(file)
}
impl Staged {
    pub(super) fn create(job: &Path, vendor: Arc<VendorInputs>) -> io::Result<Self> {
        let parent =
            crate::history::directory(job.to_str().ok_or_else(error)?).map_err(io::Error::other)?;
        private(&parent)?;
        rfs::mkdirat(
            &parent,
            "dependencies",
            Mode::RUSR | Mode::WUSR | Mode::XUSR,
        )?;
        let root = child(&parent, "dependencies")?;
        let config = vendor
            .cargo_config(Path::new("/vendor"))
            .map_err(io::Error::other)?
            .into_bytes();
        if config.len() > 65536 {
            return Err(error());
        }
        let mut paths = BTreeSet::new();
        paths.insert(String::new());
        for source in vendor.sources() {
            paths.insert(source.directory().to_owned());
        }
        let files: Vec<_> = vendor
            .files()
            .map(|f| (f.path().to_owned(), f.bytes().to_vec()))
            .chain(std::iter::once((
                "cargo-config.toml".into(),
                config.clone(),
            )))
            .collect();
        for (path, _) in &files {
            let mut prefix = String::new();
            let parts: Vec<_> = path.split('/').collect();
            for part in &parts[..parts.len() - 1] {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                paths.insert(prefix.clone());
                if paths.len() > 16384 {
                    return Err(error());
                }
            }
        }
        let mut directories = BTreeMap::new();
        for path in &paths {
            let fd = descend(&root, path, true)?;
            directories.insert(path.clone(), id(&fd)?);
        }
        for (path, bytes) in &files {
            let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
            let fd = descend(&root, dir, false)?;
            let mut output = File::from(rfs::openat(
                &fd,
                name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )?);
            output.write_all(bytes)?;
            rfs::fchmod(&output, Mode::from_raw_mode(0o644))?;
            output.sync_all()?;
        }
        for path in paths.iter().rev() {
            descend(&root, path, false)?.sync_all()?;
        }
        parent.sync_all()?;
        let value = Self {
            root,
            parent,
            vendor,
            config,
            directories,
        };
        value.verify()?;
        Ok(value)
    }
    pub(super) fn binding(&self) -> io::Result<Sha256> {
        self.verify()?;
        let mut bytes = self
            .vendor
            .materialization()
            .bundle_sha256()
            .bytes()
            .to_vec();
        bytes.extend(self.vendor.archive_fingerprint().bytes());
        bytes.extend(Sha256::digest(&self.config).bytes());
        for (name, (dev, ino)) in &self.directories {
            bytes.extend((name.len() as u64).to_be_bytes());
            bytes.extend(name.as_bytes());
            bytes.extend(dev.to_be_bytes());
            bytes.extend(ino.to_be_bytes());
        }
        Ok(Sha256::digest(&bytes))
    }
    pub(super) fn verify(&self) -> io::Result<()> {
        private(&self.parent)?;
        if id(&child(&self.parent, "dependencies")?)? != id(&self.root)? {
            return Err(error());
        }
        let expected: BTreeMap<_, _> = self
            .vendor
            .files()
            .map(|f| (f.path().to_owned(), f.bytes().to_vec()))
            .chain(std::iter::once((
                "cargo-config.toml".into(),
                self.config.clone(),
            )))
            .collect();
        let mut seen = BTreeSet::new();
        let mut seen_dirs = BTreeSet::new();
        self.walk("", &expected, &mut seen, &mut seen_dirs)?;
        if seen.len() != expected.len() || seen_dirs.len() != self.directories.len() {
            return Err(error());
        }
        Ok(())
    }
    fn walk(
        &self,
        path: &str,
        expected: &BTreeMap<String, Vec<u8>>,
        seen: &mut BTreeSet<String>,
        seen_dirs: &mut BTreeSet<String>,
    ) -> io::Result<()> {
        let fd = descend(&self.root, path, false)?;
        if self.directories.get(path) != Some(&id(&fd)?) {
            return Err(error());
        }
        seen_dirs.insert(path.to_owned());
        for entry in Dir::read_from(&fd)? {
            let entry = entry?;
            let name = entry.file_name().to_str().map_err(|_| error())?;
            if matches!(name, "." | "..") {
                continue;
            }
            let full = if path.is_empty() {
                name.to_owned()
            } else {
                format!("{path}/{name}")
            };
            if self.directories.contains_key(&full) {
                self.walk(&full, expected, seen, seen_dirs)?;
                continue;
            }
            let bytes = expected.get(&full).ok_or_else(error)?;
            let mut input = File::from(rfs::openat(
                &fd,
                name,
                OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            let before = input.metadata()?;
            if !before.is_file()
                || before.uid() != rustix::process::geteuid().as_raw()
                || before.mode() & 0o7777 != 0o644
                || before.nlink() != 1
                || before.len() != bytes.len() as u64
            {
                return Err(error());
            }
            let mut actual = Vec::new();
            (&mut input)
                .take(bytes.len() as u64 + 1)
                .read_to_end(&mut actual)?;
            let after = input.metadata()?;
            if actual != *bytes
                || before.ino() != after.ino()
                || before.len() != after.len()
                || before.mtime() != after.mtime()
                || before.mtime_nsec() != after.mtime_nsec()
                || before.ctime() != after.ctime()
                || before.ctime_nsec() != after.ctime_nsec()
            {
                return Err(error());
            }
            seen.insert(full);
        }
        if id(&descend(&self.root, path, false)?)? != id(&fd)? {
            return Err(error());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    #[test]
    fn exact_inputs_are_exclusive_and_reverified_without_following_aliases() {
        let vendor = Arc::new(crate::library::vendor::tests::attempt_fixture());
        let root = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "devoption-attempt-vendor-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let staged = Staged::create(&root, vendor.clone()).unwrap();
        let binding = staged.binding().unwrap();
        assert!(Staged::create(&root, vendor.clone()).is_err());
        assert!(staged.binding().unwrap() == binding);
        let (relative, original) = vendor
            .files()
            .next()
            .map(|f| (f.path().to_owned(), f.bytes().to_vec()))
            .unwrap();
        let path = root.join("dependencies").join(relative);
        std::fs::write(&path, b"changed").unwrap();
        assert!(staged.verify().is_err());
        std::fs::write(&path, &original).unwrap();
        staged.verify().unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(staged.verify().is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(staged.verify().is_err());
        symlink("/dev/null", &path).unwrap();
        assert!(staged.verify().is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, &original).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        staged.verify().unwrap();
        let extra = root.join("dependencies/extra");
        std::fs::write(&extra, b"extra").unwrap();
        assert!(staged.verify().is_err());
        std::fs::remove_file(extra).unwrap();
        std::fs::rename(root.join("dependencies"), root.join("old")).unwrap();
        std::fs::create_dir(root.join("dependencies")).unwrap();
        std::fs::set_permissions(
            root.join("dependencies"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        assert!(staged.verify().is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
