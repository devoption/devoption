//! Existing operator-selected root only. No credentials, directories or policy
//! decisions are created here. Native stores may create their own private locks.
use super::*;
use devoption_projects::worktree::PermittedRoot;
use rustix::{
    fs::{Mode, OFlags, open, openat},
    process::geteuid,
};
use std::{
    fs::File,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Component, Path},
};
const DIR: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema: u32,
    registry: String,
    index: String,
}
pub(super) struct Root {
    pub binding: HostedRegistry,
    pub candidates: String,
    pub policy: String,
    path: String,
    directory: File,
    children: [File; 2],
    config: Vec<u8>,
}
fn identity(file: &File) -> Result<(u64, u64), ()> {
    let m = file.metadata().map_err(|_| ())?;
    Ok((m.dev(), m.ino()))
}
fn raw_directory(path: &Path) -> Result<File, ()> {
    if !path.is_absolute()
        || path.to_str().is_none()
        || path
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        || path
            .to_string_lossy()
            .split('/')
            .skip(1)
            .any(|s| s.is_empty() || s == "." || s == "..")
    {
        return Err(());
    }
    let mut fd = File::from(open("/", DIR, Mode::empty()).map_err(|_| ())?);
    for component in path.components().skip(1) {
        fd = File::from(openat(&fd, component.as_os_str(), DIR, Mode::empty()).map_err(|_| ())?);
    }
    Ok(fd)
}
fn directory(path: &Path) -> Result<File, ()> {
    let fd = raw_directory(path)?;
    private_directory(&fd)?;
    Ok(fd)
}
fn ancestor(ancestor: &File, child: &File) -> Result<bool, ()> {
    let wanted = identity(ancestor)?;
    let mut current = child.try_clone().map_err(|_| ())?;
    for _ in 0..128 {
        let id = identity(&current)?;
        if id == wanted {
            return Ok(true);
        }
        let parent = File::from(openat(&current, "..", DIR, Mode::empty()).map_err(|_| ())?);
        if identity(&parent)? == id {
            return Ok(false);
        }
        current = parent;
    }
    Err(())
}
fn private_directory(file: &File) -> Result<(), ()> {
    let m = file.metadata().map_err(|_| ())?;
    if geteuid().as_raw() == 0
        || !m.is_dir()
        || m.uid() != geteuid().as_raw()
        || m.mode() & 0o7777 != 0o700
        || m.nlink() == 0
    {
        return Err(());
    }
    Ok(())
}
fn config(directory: &File) -> Result<Vec<u8>, ()> {
    let mut file = File::from(
        openat(
            directory,
            "config.toml",
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| ())?,
    );
    let before = file.metadata().map_err(|_| ())?;
    if !before.is_file()
        || before.uid() != geteuid().as_raw()
        || before.mode() & 0o7777 != 0o600
        || before.nlink() != 1
        || before.len() > 4096
    {
        return Err(());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    let after = file.metadata().map_err(|_| ())?;
    if before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
        || bytes.len() as u64 != after.len()
    {
        return Err(());
    }
    Ok(bytes)
}
impl Root {
    pub fn open(
        path: &Path,
        owner: &CatalogDirectory,
        forbidden: &[std::path::PathBuf],
    ) -> Result<Self, ()> {
        let directory = directory(path)?;
        let selected = PermittedRoot::open_absolute(path).map_err(|_| ())?;
        if !owner.is_disjoint(&selected).map_err(|_| ())? {
            return Err(());
        }
        for path in forbidden {
            let other = raw_directory(path)?;
            if ancestor(&directory, &other)? || ancestor(&other, &directory)? {
                return Err(());
            }
        }
        let bytes = config(&directory)?;
        let packet: Config =
            toml::from_str(std::str::from_utf8(&bytes).map_err(|_| ())?).map_err(|_| ())?;
        if packet.schema != 1 {
            return Err(());
        }
        let binding = HostedRegistry::new(
            RegistryId::new(&packet.registry).map_err(|_| ())?,
            &packet.index,
        )
        .map_err(|_| ())?;
        let children = ["candidates", "policy"].map(|name| {
            openat(&directory, name, DIR, Mode::empty())
                .map(File::from)
                .map_err(|_| ())
        });
        let [candidates, policy] = children;
        let children = [candidates?, policy?];
        for child in &children {
            private_directory(child)?;
        }
        let path = path.to_str().ok_or(())?.to_owned();
        let root = Self {
            binding,
            candidates: format!("{path}/candidates"),
            policy: format!("{path}/policy"),
            path,
            directory,
            children,
            config: bytes,
        };
        root.verify()?;
        Ok(root)
    }
    pub fn verify(&self) -> Result<(), ()> {
        private_directory(&self.directory)?;
        if identity(&directory(Path::new(&self.path))?)? != identity(&self.directory)?
            || config(&self.directory)? != self.config
        {
            return Err(());
        }
        for entry in rustix::fs::Dir::read_from(&self.directory).map_err(|_| ())? {
            let entry = entry.map_err(|_| ())?;
            if !matches!(
                entry.file_name().to_str().map_err(|_| ())?,
                "." | ".." | "config.toml" | "candidates" | "policy"
            ) {
                return Err(());
            }
        }
        for (name, retained) in ["candidates", "policy"].into_iter().zip(&self.children) {
            let current =
                File::from(openat(&self.directory, name, DIR, Mode::empty()).map_err(|_| ())?);
            private_directory(&current)?;
            if identity(&current)? != identity(retained)? {
                return Err(());
            }
        }
        Ok(())
    }
}
