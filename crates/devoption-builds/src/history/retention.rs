//! Resumable retention under the history writer and the caller's admission gate.
use super::*;
use crate::retention::{self, Attempt, Policy};
use devoption_projects::ProjectId;
use rustix::fs::{AtFlags, Dir, unlinkat};
use std::{ffi::CStr, io, path::Path};

pub(super) fn key(prefix: &str, id: JobId) -> String {
    let hex: String = id.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
    format!("build-{prefix}-{hex}.bin")
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RetentionStatus {
    /// None means the retained snapshot could not be verified.
    pub retained: Option<usize>,
    /// None means reference/protection verification did not complete.
    pub protected: Option<usize>,
    pub removed: usize,
    /// Bytes from successful removals in this reconciliation only.
    pub reclaimed_bytes: u64,
    pub failed: bool,
}

impl History {
    // Content-addressed private pending files are retryable. A partial matching
    // write may be completed, but unrelated bytes or unsafe files are refused.
    fn publish_retention_snapshot(
        &mut self,
        name: &str,
        bytes: &[u8],
        max: usize,
        write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
    ) -> Result<(), Error> {
        self.check()?;
        self.read_snapshot(name, max)?;
        if bytes.len() > max {
            return Err(Error::Corrupt);
        }
        let digest = devoption_registry::Sha256::digest(&[name.as_bytes(), bytes].concat());
        let pending = format!(
            ".retention-{}.tmp",
            digest
                .bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let mut file = File::from(
            openat(
                &self.directory,
                pending.as_str(),
                OFlags::RDWR
                    | OFlags::CREATE
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|_| Error::Unavailable)?,
        );
        check_file(&file, self.uid, max as u64)?;
        let mut old = Vec::new();
        Read::by_ref(&mut file)
            .take((max + 1) as u64)
            .read_to_end(&mut old)
            .map_err(|_| Error::Unavailable)?;
        if !bytes.starts_with(&old) {
            return Err(Error::Changed);
        }
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(0))
            .map_err(|_| Error::Unavailable)?;
        write(&mut file, bytes).map_err(|_| Error::Unavailable)?;
        file.sync_all().map_err(|_| Error::Unavailable)?;
        self.check_storage()?;
        same_child(&self.directory, pending.as_str(), &file)?;
        renameat(&self.directory, pending.as_str(), &self.directory, name)
            .map_err(|_| Error::Unavailable)?;
        self.directory.sync_all().map_err(|_| Error::Unavailable)
    }

    pub(super) fn audit_exists(&self, id: JobId) -> Result<bool, Error> {
        // Any unsafe/corrupt audit refuses allocation instead of permitting reuse.
        Ok(self
            .read_snapshot(&key("audit", id), codec::MAX_BYTES)?
            .is_some())
    }

    pub fn retention_policy(&self, project: ProjectId) -> Result<Policy, Error> {
        match self.read_snapshot(&key("policy", JobId::from_bytes(project.to_bytes())), 2)? {
            None => Ok(Policy::default()),
            Some(bytes) if bytes.len() == 2 => {
                Policy::new(u16::from_be_bytes(bytes.try_into().unwrap()))
                    .map_err(|_| Error::Corrupt)
            }
            _ => Err(Error::Corrupt),
        }
    }

    /// Caller must first validate project existence/authorization. Saving a
    /// policy does not claim successful cleanup; reconcile and report separately.
    pub fn set_retention_policy(
        &mut self,
        project: ProjectId,
        policy: Policy,
    ) -> Result<(), Error> {
        self.retention_policy(project)?;
        self.publish_retention_snapshot(
            &key("policy", JobId::from_bytes(project.to_bytes())),
            &policy.count().to_be_bytes(),
            2,
            |f, b| f.write_all(b),
        )
    }

    pub fn retention_status(&self, project: ProjectId) -> Result<Option<RetentionStatus>, Error> {
        if self.retention_status_failures.contains(&project.to_bytes()) {
            return Err(Error::Unavailable);
        }
        let Some(bytes) =
            self.read_snapshot(&key("retention", JobId::from_bytes(project.to_bytes())), 33)?
        else {
            return Ok(None);
        };
        if bytes.len() != 33 || bytes[32] > 1 {
            return Err(Error::Corrupt);
        }
        let n = |offset| u64::from_be_bytes(bytes[offset..offset + 8].try_into().unwrap());
        Ok(Some(RetentionStatus {
            retained: if n(0) == u64::MAX {
                None
            } else {
                Some(usize::try_from(n(0)).map_err(|_| Error::Corrupt)?)
            },
            protected: if n(8) == u64::MAX {
                None
            } else {
                Some(usize::try_from(n(8)).map_err(|_| Error::Corrupt)?)
            },
            removed: usize::try_from(n(16)).map_err(|_| Error::Corrupt)?,
            reclaimed_bytes: n(24),
            failed: bytes[32] == 1,
        }))
    }

    pub fn save_retention_status(
        &mut self,
        project: ProjectId,
        status: &RetentionStatus,
    ) -> Result<(), Error> {
        let mut bytes = Vec::new();
        for n in [
            status.retained.map(|n| n as u64).unwrap_or(u64::MAX),
            status.protected.map(|n| n as u64).unwrap_or(u64::MAX),
            status.removed as u64,
            status.reclaimed_bytes,
        ] {
            bytes.extend(n.to_be_bytes());
        }
        bytes.push(u8::from(status.failed));
        let result = self.publish_retention_snapshot(
            &key("retention", JobId::from_bytes(project.to_bytes())),
            &bytes,
            33,
            |f, b| f.write_all(b),
        );
        if result.is_err() {
            self.retention_status_failures.insert(project.to_bytes());
        } else {
            self.retention_status_failures.remove(&project.to_bytes());
        }
        result
    }

    /// Bind a known attempt to its exact private staging directory before it can
    /// execute. Only one direct 32-hex child under the selected root is accepted.
    pub fn bind_work(&mut self, id: JobId, root: &Path, path: &Path) -> Result<(), Error> {
        if !self
            .load()?
            .iter()
            .any(|r| r.id == id && !terminal(&r.state))
        {
            return Err(Error::UnknownAttempt);
        }
        self.bind_work_path(id, root, path)
    }

    pub(super) fn work_binding_exists(&self, id: JobId) -> Result<bool, Error> {
        Ok(self.read_snapshot(&key("owned", id), 64)?.is_some())
    }

    pub(super) fn bind_work_path(
        &mut self,
        id: JobId,
        root: &Path,
        path: &Path,
    ) -> Result<(), Error> {
        if path.parent() != Some(root) {
            return Err(Error::Unsafe);
        }
        let root = directory(root.to_str().ok_or(Error::Unsafe)?)?;
        check_directory(&root, self.uid)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(Error::Unsafe)?;
        if !token(name) {
            return Err(Error::Unsafe);
        }
        let job =
            File::from(openat(&root, name, DIRECTORY, Mode::empty()).map_err(|_| Error::Unsafe)?);
        check_directory(&job, self.uid)?;
        let mut bytes = name.as_bytes().to_vec();
        for value in [
            identity(&root)?.0,
            identity(&root)?.1,
            identity(&job)?.0,
            identity(&job)?.1,
        ] {
            bytes.extend(value.to_be_bytes());
        }
        if self.read_snapshot(&key("owned", id), 64)?.is_some() {
            return Err(Error::Changed);
        }
        self.publish_retention_snapshot(&key("owned", id), &bytes, 64, |f, b| f.write_all(b))
    }

    /// After the installed broker has stopped and verified the entire old unit,
    /// startup may mark interrupted records reconciled. No workload is resumed.
    pub fn confirm_interrupted_cleanup(&mut self) -> Result<(), Error> {
        let mut records = self.load()?;
        for record in &mut records {
            if matches!(record.state, State::Interrupted) {
                record.cleanup_confirmed = true;
            }
        }
        self.publish(&records)
    }

    /// Owns no release store. `references` is observed once per reconciliation
    /// and must freshly observe every saved release under a cross-process
    /// promotion/cleanup lock held by the caller for the entire call. Pending
    /// work stays in history until removed candidates' owned files are deleted
    /// and the compacted snapshot is published once, so restart retries it.
    pub fn reconcile_retention(
        &mut self,
        project: ProjectId,
        work: Option<&Path>,
        references: impl FnOnce() -> Result<Vec<JobId>, Error>,
    ) -> Result<RetentionStatus, Error> {
        self.reconcile_retention_using(project, work, references, |f, b| f.write_all(b))
    }

    fn reconcile_retention_using(
        &mut self,
        project: ProjectId,
        work: Option<&Path>,
        references: impl FnOnce() -> Result<Vec<JobId>, Error>,
        publish_history: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
    ) -> Result<RetentionStatus, Error> {
        let (mut records, watermark) = match self.read_snapshot(RECORD, codec::MAX_BYTES)? {
            Some(bytes) => codec::decode_with_watermark(&bytes)?,
            None => (Vec::new(), 0),
        };
        let attempts: Vec<_> = records
            .iter()
            .map(|r| Attempt {
                id: r.id,
                project: ProjectId::from_bytes(r.project),
                state: &r.state,
                completion: r.completion,
                cleanup_confirmed: r.cleanup_confirmed,
                cancelled_before_run: r.cancelled_before_run,
            })
            .collect();
        let refs = references()?;
        let plan = retention::plan(project, self.retention_policy(project)?, &attempts, &refs)
            .map_err(|_| Error::Corrupt)?;
        let mut status = RetentionStatus {
            retained: Some(plan.retained.len()),
            protected: Some(
                plan.retained
                    .iter()
                    .filter(|(_, reason)| *reason != retention::KeepReason::Newest)
                    .count(),
            ),
            ..Default::default()
        };
        if plan.candidates.is_empty() {
            return Ok(status);
        }
        let index: std::collections::HashMap<JobId, usize> = records
            .iter()
            .enumerate()
            .map(|(i, record)| (record.id, i))
            .collect();
        let mut pruned = std::collections::HashSet::new();
        let mut reclaimed_bytes: u64 = 0;
        let mut unlinked_root: Option<File> = None;
        let mut unlinked_output = false;
        for id in plan.candidates {
            let result = index.get(&id).ok_or(Error::UnknownAttempt).and_then(|&i| {
                self.prune_files(&records[i], work, &mut unlinked_root, &mut unlinked_output)
            });
            match result {
                Ok(bytes) => {
                    pruned.insert(id);
                    reclaimed_bytes = reclaimed_bytes.saturating_add(bytes);
                }
                Err(_) => {
                    status.failed = true;
                    status.retained = status.retained.map(|count| count + 1);
                }
            }
        }
        if !pruned.is_empty() {
            records.retain(|r| !pruned.contains(&r.id));
            let commit = (|| -> Result<(), Error> {
                if let Some(root) = unlinked_root {
                    root.sync_all().map_err(|_| Error::Unavailable)?;
                }
                if unlinked_output {
                    self.directory.sync_all().map_err(|_| Error::Unavailable)?;
                }
                // Publish removal once after all pruned files are unlinked.
                // Audit and owned-path bindings remain as compact durable
                // metadata, preserving ID non-reuse and interruption evidence.
                let snapshot = codec::encode_with_watermark(&records, watermark)?;
                self.publish_retention_snapshot(
                    RECORD,
                    &snapshot,
                    codec::MAX_BYTES,
                    publish_history,
                )
            })();
            match commit {
                Ok(()) => {
                    status.removed = pruned.len();
                    status.reclaimed_bytes = reclaimed_bytes;
                }
                Err(_) => {
                    status.failed = true;
                    status.retained = status.retained.map(|count| count + pruned.len());
                }
            }
        }
        Ok(status)
    }

    fn prune_files(
        &mut self,
        record: &Record,
        work: Option<&Path>,
        unlinked_root: &mut Option<File>,
        unlinked_output: &mut bool,
    ) -> Result<u64, Error> {
        let id = record.id;
        if !terminal(&record.state) || !record.cleanup_confirmed {
            return Err(Error::Transition);
        }
        // Verify the output's embedded attempt identity and checksum before
        // removing any staged files. A private filename alone is not evidence
        // that its contents belong to this attempt.
        let output = logs::filename(id);
        let output_bytes = self.read_snapshot(&output, crate::output::persistence::MAX_SNAPSHOT)?;
        if let Some(data) = &output_bytes {
            crate::output::BuildOutput::restore(data, id)?;
        }
        let audit = codec::encode_with_watermark(std::slice::from_ref(record), 0)?;
        let audit_name = key("audit", id);
        if let Some(old) = self.read_snapshot(&audit_name, codec::MAX_BYTES)? {
            // Watermark can advance while a previously interrupted prune retries.
            if codec::decode(&old)? != vec![record.clone()] {
                return Err(Error::Changed);
            }
        } else {
            self.publish_retention_snapshot(&audit_name, &audit, codec::MAX_BYTES, |f, b| {
                f.write_all(b)
            })?;
        }
        let mut bytes = 0;
        if let Some(owned) = self.read_snapshot(&key("owned", id), 64)? {
            if owned.len() != 64 {
                return Err(Error::Corrupt);
            }
            let root = directory(work.and_then(Path::to_str).ok_or(Error::Unavailable)?)?;
            check_directory(&root, self.uid)?;
            let n = |offset| u64::from_be_bytes(owned[offset..offset + 8].try_into().unwrap());
            if identity(&root)? != (n(32), n(40)) {
                return Err(Error::Changed);
            }
            let name = std::str::from_utf8(&owned[..32]).map_err(|_| Error::Corrupt)?;
            if !token(name) {
                return Err(Error::Corrupt);
            }
            match openat(&root, name, DIRECTORY, Mode::empty()) {
                Ok(fd) => {
                    let job = File::from(fd);
                    check_directory(&job, self.uid)?;
                    if identity(&job)? != (n(48), n(56)) {
                        return Err(Error::Changed);
                    }
                    let mut count = 0;
                    validate_tree(&job, self.uid, identity(&job)?.0, 0, &mut count)?;
                    bytes += remove_contents(&job, self.uid, identity(&job)?.0, 0)?;
                    same_child(&root, name, &job)?;
                    unlinkat(&root, name, AtFlags::REMOVEDIR).map_err(|_| Error::Unavailable)?;
                    *unlinked_root = Some(root);
                }
                Err(rustix::io::Errno::NOENT) => {} // prior interrupted removal
                Err(_) => return Err(Error::Unsafe),
            }
        } else {
            // Legacy attempts lack a verified path binding. Preserve their
            // files and evidence rather than guessing a directory.
            return Err(Error::Unavailable);
        }
        let current_output =
            self.read_snapshot(&output, crate::output::persistence::MAX_SNAPSHOT)?;
        if current_output != output_bytes {
            return Err(Error::Changed);
        }
        if let Some(data) = current_output {
            bytes += data.len() as u64;
            unlinkat(&self.directory, output.as_str(), AtFlags::empty())
                .map_err(|_| Error::Unavailable)?;
            *unlinked_output = true;
        }
        Ok(bytes)
    }
}

pub(super) fn token(name: &str) -> bool {
    name.len() == 32
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(super) fn same_child(
    parent: &File,
    name: impl rustix::path::Arg,
    child: &File,
) -> Result<(), Error> {
    let now = File::from(openat(parent, name, READ, Mode::empty()).map_err(|_| Error::Changed)?);
    if identity(&now)? != identity(child)? {
        return Err(Error::Changed);
    }
    Ok(())
}
fn entries(directory: &File) -> Result<Vec<std::ffi::CString>, Error> {
    let entries = Dir::read_from(directory)
        .map_err(|_| Error::Unavailable)?
        .map(|entry| {
            entry
                .map(|e| e.file_name().to_owned())
                .map_err(|_| Error::Unavailable)
        })
        .filter(|entry| {
            !entry
                .as_ref()
                .is_ok_and(|n| matches!(n.to_bytes(), b"." | b".."))
        })
        .take(100_001)
        .collect::<Result<Vec<_>, _>>()?;
    if entries.len() > 100_000 {
        return Err(Error::Unsafe);
    }
    Ok(entries)
}
fn child(parent: &File, name: &CStr, uid: u32, device: u64) -> Result<File, Error> {
    let file = File::from(openat(parent, name, READ, Mode::empty()).map_err(|_| Error::Unsafe)?);
    let m = file.metadata().map_err(|_| Error::Unavailable)?;
    if m.uid() != uid
        || m.dev() != device
        || m.mode() & 0o022 != 0
        || (!m.is_dir() && (!m.is_file() || m.nlink() != 1))
    {
        return Err(Error::Unsafe);
    }
    Ok(file)
}
fn validate_tree(
    dir: &File,
    uid: u32,
    device: u64,
    depth: usize,
    count: &mut usize,
) -> Result<(), Error> {
    if depth > 64 {
        return Err(Error::Unsafe);
    }
    for name in entries(dir)? {
        *count += 1;
        if *count > 100_000 {
            return Err(Error::Unsafe);
        }
        let f = child(dir, &name, uid, device)?;
        if f.metadata().map_err(|_| Error::Unavailable)?.is_dir() {
            validate_tree(&f, uid, device, depth + 1, count)?;
        }
    }
    Ok(())
}
fn remove_contents(dir: &File, uid: u32, device: u64, depth: usize) -> Result<u64, Error> {
    if depth > 64 {
        return Err(Error::Unsafe);
    }
    let mut bytes: u64 = 0;
    for name in entries(dir)? {
        let f = child(dir, &name, uid, device)?;
        let m = f.metadata().map_err(|_| Error::Unavailable)?;
        if m.is_dir() {
            bytes = bytes.saturating_add(remove_contents(&f, uid, device, depth + 1)?);
            same_child(dir, &name, &f)?;
            unlinkat(dir, &name, AtFlags::REMOVEDIR).map_err(|_| Error::Unavailable)?;
        } else {
            same_child(dir, &name, &f)?;
            unlinkat(dir, &name, AtFlags::empty()).map_err(|_| Error::Unavailable)?;
            bytes = bytes.saturating_add(m.len());
        }
    }
    dir.sync_all().map_err(|_| Error::Unavailable)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests;
