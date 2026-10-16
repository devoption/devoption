//! Single-writer catalog snapshots on a local Unix filesystem.
//!
//! The caller supplies an existing private data directory, never a checkout path.
//! An advisory lock is held for the store's lifetime; all cooperating writers must
//! use it. The OS owner remains trusted. No protection from that owner or network
//! filesystem locking/durability guarantee is claimed. Never remove the lock file.

use crate::{Catalog, Error, codec};
use rustix::{
    fs::{AtFlags, FlockOperation, Mode, OFlags, flock, open, openat, renameat, unlinkat},
    io::Errno,
    process::geteuid,
};
use std::{
    fs::{File, Metadata},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path},
    sync::atomic::{AtomicU64, Ordering},
};

const RECORD: &str = "projects.bin";
const LOCK: &str = ".projects.lock";
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Read one validated, bounded snapshot without creating files or taking a writer
/// lock. Only an absent catalog means None. The same private directory/file rules
/// apply as for writers; any unsafe or interrupted read fails closed.
pub fn read_catalog(path: &Path) -> Result<Option<Catalog>, Error> {
    load_snapshot(&open_directory(path)?)
}

/// An unlocked catalog directory capability. Reads load fresh snapshots; writers
/// acquire their own short-lived locks. Retaining this handle never holds a lock.
pub struct CatalogDirectory {
    directory: File,
}

impl CatalogDirectory {
    /// Open an existing private directory without creating files or taking a
    /// writer lock. Reject relative paths, parent traversal and symlink components.
    pub fn open_absolute(path: &Path) -> Result<Self, Error> {
        Self::from_directory(open_directory(path)?)
    }

    /// Adopt an already selected directory descriptor after checking its type,
    /// ownership and private mode. The caller selects the directory; no pathname
    /// is resolved here. This permits shared identity with another storage layer.
    pub fn from_directory(directory: File) -> Result<Self, Error> {
        directory_valid(&directory)?;
        Ok(Self { directory })
    }

    pub fn load(&self) -> Result<Option<Catalog>, Error> {
        load_snapshot(&self.directory)
    }

    pub fn writer(&self) -> Result<CatalogStore, Error> {
        CatalogStore::open_directory(self.directory.try_clone()?)
    }

    /// Check actual ancestry against the retained data directory, not a reopened
    /// data pathname. The repository root retains its own binding checks.
    pub fn is_disjoint(&self, root: &crate::worktree::PermittedRoot) -> Result<bool, Error> {
        directory_valid(&self.directory)?;
        Ok(root.disjoint_directory(&self.directory)?)
    }

    /// Read-only review; creates no lock, backup or catalog file.
    pub fn review_device_rebind(
        &self,
        root: &crate::worktree::PermittedRoot,
    ) -> Result<crate::recovery::RecoveryReview, Error> {
        if !self.is_disjoint(root)? {
            return Err(Error::RootMismatch);
        }
        let metadata = self.directory.metadata()?;
        crate::recovery::review(
            self.load()?.ok_or(Error::InvalidCatalog)?,
            root,
            (metadata.dev(), metadata.ino()),
        )
    }
}

/// An exclusive catalog writer anchored to an existing private directory.
/// Opening creates only a private lock file. Drop releases the lock, not its file.
/// Share this handle through application serialization; do not cache competing
/// snapshots across unrelated read/modify/save operations on the same handle.
pub struct CatalogStore {
    directory: File,
    lock: File,
}

impl Drop for CatalogStore {
    fn drop(&mut self) {
        // A concurrent spawn can retain a duplicate until exec. Release this
        // writer's lock now instead of waiting for every duplicate to close.
        let _ = flock(&self.lock, FlockOperation::Unlock);
    }
}

impl CatalogStore {
    /// Explicit local trust grant after review. Holds the writer lock, checks a
    /// fresh complete review and retains a durable private backup before publish.
    /// An error after publication is uncertain; inspect current state before retry.
    pub fn approve_device_rebind(
        &mut self,
        root: &crate::worktree::PermittedRoot,
        fingerprint: [u8; 32],
    ) -> Result<String, Error> {
        self.approve_device_rebind_using(root, fingerprint, || Ok(()))
    }

    pub(crate) fn approve_device_rebind_using(
        &mut self,
        root: &crate::worktree::PermittedRoot,
        fingerprint: [u8; 32],
        before_publish: impl FnOnce() -> Result<(), Error>,
    ) -> Result<String, Error> {
        self.check()?;
        let reader = CatalogDirectory::from_directory(self.directory.try_clone()?)?;
        let reviewed = reader.review_device_rebind(root)?;
        if reviewed.fingerprint() != fingerprint {
            return Err(Error::StaleCheckout);
        }
        let bytes = codec::encode(&reviewed.original)?;
        let backup = reviewed.backup_name();
        let backup_file = match openat(
            &self.directory,
            backup.as_str(),
            OFlags::WRONLY
                | OFlags::CREATE
                | OFlags::EXCL
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(fd) => {
                let mut file = File::from(fd);
                file_valid(&file)?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                file
            }
            Err(Errno::EXIST) => {
                // An uncertain earlier attempt may already have made this exact
                // backup. Never overwrite it or accept unsafe/different contents.
                let mut file = File::from(openat(
                    &self.directory,
                    backup.as_str(),
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    Mode::empty(),
                )?);
                file_valid(&file)?;
                let mut existing = Vec::new();
                Read::by_ref(&mut file)
                    .take((codec::MAX_BYTES + 1) as u64)
                    .read_to_end(&mut existing)?;
                file.sync_all()?;
                if existing != bytes {
                    return Err(Error::InvalidCatalog);
                }
                file
            }
            Err(error) => return Err(error.into()),
        };
        self.directory.sync_all()?;
        before_publish()?;
        file_valid(&backup_file)?;
        let mut current_backup = File::from(openat(
            &self.directory,
            backup.as_str(),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        file_valid(&current_backup)?;
        let retained = backup_file.metadata()?;
        let current = current_backup.metadata()?;
        if (retained.dev(), retained.ino()) != (current.dev(), current.ino()) {
            return Err(Error::UnsafeFile);
        }
        let mut final_bytes = Vec::new();
        Read::by_ref(&mut current_backup)
            .take((codec::MAX_BYTES + 1) as u64)
            .read_to_end(&mut final_bytes)?;
        file_valid(&current_backup)?;
        if final_bytes != bytes {
            return Err(Error::InvalidCatalog);
        }
        current_backup.sync_all()?;
        // Recheck every saved observation after backup and before atomic save.
        if reader.review_device_rebind(root)?.fingerprint() != fingerprint {
            return Err(Error::StaleCheckout);
        }
        self.save(&reviewed.replacement)?;
        Ok(backup)
    }
    /// Create a new empty checkout and publish its identity/association under the
    /// catalog lock. Never reuse an existing path or remove files on failure.
    /// An interrupted initialization/save may leave an unregistered checkout;
    /// inspect saved state before retrying or explicitly attaching it.
    pub fn create_managed_repository(
        &mut self,
        project: crate::ProjectId,
        root: &crate::worktree::ManagedRoot,
        name: &str,
    ) -> Result<crate::RepositoryId, Error> {
        crate::worktree::ManagedRoot::validate_name(name)?;
        if !root.permitted().disjoint_directory(&self.directory)? {
            return Err(Error::RootMismatch);
        }
        let mut catalog = self.load()?.unwrap_or_default();
        let selected = catalog
            .projects()
            .iter()
            .find(|candidate| candidate.id() == project)
            .ok_or(Error::UnknownProject)?;
        // Include legacy/attached records so a new storage layout cannot create
        // a second same-named repository in this project. Existing data is unchanged.
        if catalog.repositories().iter().any(|repository| {
            selected.repositories().contains(&repository.id())
                && repository.name().eq_ignore_ascii_case(name)
        }) {
            return Err(Error::RepositoryExists);
        }
        if catalog.repositories().len() >= crate::MAX_REPOSITORIES {
            return Err(Error::Capacity);
        }
        catalog.check_repository_root(root.permitted())?;
        let project_key: String = project
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let path = root.initialize(&format!("{project_key}--{name}"))?;
        // Initialization has already published filesystem state. Even a capacity
        // failure during catalog encoding now requires inspection before retry.
        let repository = catalog
            .attach_named(project, root.permitted(), &path, Some(name))
            .map_err(|_| Error::CreationUncertain)?;
        self.save(&catalog).map_err(|_| Error::CreationUncertain)?;
        Ok(repository)
    }

    /// Validate and attach one selected checkout under this writer's lock.
    /// Shared Git common directories reuse a repository identity across projects.
    /// This grants no execution permission and never invokes Git.
    pub fn attach_repository(
        &mut self,
        project: crate::ProjectId,
        root: &crate::worktree::PermittedRoot,
        candidate: &Path,
    ) -> Result<crate::RepositoryId, Error> {
        if !root.disjoint_directory(&self.directory)? {
            return Err(Error::RootMismatch);
        }
        let mut catalog = self.load()?.unwrap_or_default();
        let repository = catalog.attach(project, root, candidate)?;
        self.save(&catalog)?;
        Ok(repository)
    }

    /// Load, add an empty project and durably save under this writer's lock.
    /// Never reuse a caller's potentially stale snapshot for this operation.
    /// As with save, an error after rename requires inspection before retrying.
    pub fn create_project(&mut self, name: &str) -> Result<crate::ProjectId, Error> {
        let mut catalog = self.load()?.unwrap_or_default();
        let id = catalog.create_project(name)?;
        self.save(&catalog)?;
        Ok(id)
    }

    /// Rename in a fresh snapshot under this writer's lock, preserving bindings.
    /// A post-publication sync failure requires inspection before retrying.
    pub fn rename_project(&mut self, id: crate::ProjectId, name: &str) -> Result<(), Error> {
        let mut catalog = self.load()?.unwrap_or_default();
        catalog.rename_project(id, name)?;
        self.save(&catalog)
    }

    /// Reject relative paths, `..`, and symlinks in every directory component.
    /// Final directory must belong to the current OS user with no group/other
    /// permissions. Opening neither creates directories nor repairs permissions.
    pub fn open_absolute(path: &Path) -> Result<Self, Error> {
        Self::open_directory(open_directory(path)?)
    }

    fn open_directory(directory: File) -> Result<Self, Error> {
        directory_valid(&directory)?;
        let lock = File::from(openat(
            &directory,
            LOCK,
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )?);
        file_valid(&lock)?;
        if lock.metadata()?.len() != 0 {
            return Err(Error::UnsafeFile);
        }
        match flock(&lock, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {}
            Err(Errno::WOULDBLOCK) => return Err(Error::Busy),
            Err(error) => return Err(error.into()),
        }
        let store = Self { directory, lock };
        // Corruption must be visible at open, not deferred until an overwrite.
        store.load()?;
        Ok(store)
    }

    /// Only an absent projects.bin is None. Invalid/private/unsupported files
    /// return errors. Temporary files are never loaded, even after interruption.
    pub fn load(&self) -> Result<Option<Catalog>, Error> {
        self.check()?;
        load_snapshot(&self.directory)
    }

    /// Atomically replace a complete catalog while holding the writer lock.
    /// Validation or pre-publication failure preserves the previous snapshot.
    /// An error syncing the directory AFTER rename may mean the new snapshot is
    /// visible but durability is uncertain: reload/inspect; never reset/delete it.
    pub fn save(&mut self, catalog: &Catalog) -> Result<(), Error> {
        self.save_using(catalog, |file, bytes| file.write_all(bytes))
    }

    // Keep the publication path identical when tests inject a partial-write error.
    pub(crate) fn save_using(
        &mut self,
        catalog: &Catalog,
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<(), Error> {
        self.load()?;
        let bytes = codec::encode(catalog)?;
        let (mut file, pending) = self.temporary()?;
        write(&mut file, &bytes)?;
        file.sync_all()?;
        self.check()?;
        renameat(
            &self.directory,
            pending.name.as_str(),
            &self.directory,
            RECORD,
        )?;
        self.directory.sync_all()?;
        Ok(())
    }

    fn check(&self) -> Result<(), Error> {
        directory_valid(&self.directory)?;
        file_valid(&self.lock)
    }

    fn temporary(&self) -> Result<(File, Pending<'_>), Error> {
        for _ in 0..32 {
            let name = format!(
                ".projects-{}-{}.tmp",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            );
            match openat(
                &self.directory,
                name.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(fd) => {
                    return Ok((
                        File::from(fd),
                        Pending {
                            directory: &self.directory,
                            name,
                        },
                    ));
                }
                Err(Errno::EXIST) => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(Errno::EXIST.into())
    }
}

fn open_directory(path: &Path) -> Result<File, Error> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(Error::UnsafePath);
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = File::from(open("/", flags, Mode::empty())?);
    for component in path.components() {
        if let Component::Normal(name) = component {
            directory = File::from(openat(&directory, name, flags, Mode::empty())?);
        }
    }
    directory_valid(&directory)?;
    Ok(directory)
}

fn load_snapshot(directory: &File) -> Result<Option<Catalog>, Error> {
    directory_valid(directory)?;
    let file = match openat(
        directory,
        RECORD,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => File::from(fd),
        Err(Errno::NOENT) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    file_valid(&file)?;
    if file.metadata()?.len() > codec::MAX_BYTES as u64 {
        return Err(Error::InvalidCatalog);
    }
    let mut bytes = Vec::new();
    file.take(codec::MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    codec::decode(&bytes).map(Some)
}

fn private(metadata: &Metadata) -> bool {
    metadata.uid() == geteuid().as_raw() && metadata.mode() & 0o077 == 0
}

fn directory_valid(directory: &File) -> Result<(), Error> {
    let metadata = directory.metadata()?;
    if !metadata.is_dir() || metadata.nlink() == 0 || !private(&metadata) {
        return Err(Error::UnsafeDirectory);
    }
    Ok(())
}

fn file_valid(file: &File) -> Result<(), Error> {
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 || !private(&metadata) {
        return Err(Error::UnsafeFile);
    }
    Ok(())
}

struct Pending<'a> {
    directory: &'a File,
    name: String,
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        let _ = unlinkat(self.directory, self.name.as_str(), AtFlags::empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn dropping_writer_releases_lock_even_when_a_descriptor_was_duplicated() {
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let root = directory.path().canonicalize().unwrap();
        let store = CatalogStore::open_absolute(&root).unwrap();
        // A subprocess spawn can briefly inherit the same open-file description
        // before CLOEXEC closes it. Duplication reproduces that lifetime without
        // a timing-dependent fork or changes to the production descriptor flags.
        let inherited = store.lock.try_clone().unwrap();
        assert!(matches!(
            CatalogStore::open_absolute(&root),
            Err(Error::Busy)
        ));
        drop(store);
        let next = CatalogStore::open_absolute(&root).unwrap();
        assert!(matches!(
            CatalogStore::open_absolute(&root),
            Err(Error::Busy)
        ));
        drop(inherited);
        assert!(matches!(
            CatalogStore::open_absolute(&root),
            Err(Error::Busy)
        ));
        drop(next);
        assert!(CatalogStore::open_absolute(&root).is_ok());
    }
}
