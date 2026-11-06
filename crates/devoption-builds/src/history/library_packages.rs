//! Bounded, descriptor-anchored reads of retained library source and provenance.
use super::*;
use crate::{jobs::ArtifactMetadata, library::MAX_PACKAGE_BYTES};
use devoption_registry::Sha256;
use std::path::Path;

/// Owned download bytes, not publication or execution authority.
pub struct LibraryPackage {
    pub source: Vec<u8>,
    pub receipt: Vec<u8>,
}

impl History {
    /// The caller must authorize project/attempt access and serialize with
    /// retention/promotion. Unknown, unsuccessful, legacy and app attempts have
    /// no library package; unsafe or mismatched retained state fails closed.
    pub fn library_package(&self, id: JobId, work: &Path) -> Result<Option<LibraryPackage>, Error> {
        let records = self.load()?;
        let Some(record) = records.iter().find(|r| r.id == id) else {
            return Ok(None);
        };
        let State::Succeeded(metadata) = &record.state else {
            return Ok(None);
        };
        let owned_name = super::retention::key("owned", id);
        let Some(owned) = self.read_snapshot(&owned_name, 64)? else {
            return Ok(None);
        };
        if !record.cleanup_confirmed {
            return Err(Error::Transition);
        }
        if owned.len() != 64 {
            return Err(Error::Corrupt);
        }
        let name = std::str::from_utf8(&owned[..32]).map_err(|_| Error::Corrupt)?;
        if !super::retention::token(name) {
            return Err(Error::Corrupt);
        }
        let n = |at| u64::from_be_bytes(owned[at..at + 8].try_into().unwrap());
        let root = directory(work.to_str().ok_or(Error::Unsafe)?)?;
        check_directory(&root, self.uid)?;
        if identity(&root)? != (n(32), n(40)) {
            return Err(Error::Changed);
        }
        let job =
            File::from(openat(&root, name, DIRECTORY, Mode::empty()).map_err(|_| Error::Unsafe)?);
        check_directory(&job, self.uid)?;
        if identity(&job)? != (n(48), n(56)) {
            return Err(Error::Changed);
        }
        let artifact = File::from(
            openat(&job, "artifact", DIRECTORY, Mode::empty()).map_err(|_| Error::Unsafe)?,
        );
        check_directory(&artifact, self.uid)?;
        if identity(&artifact)?.0 != identity(&root)?.0 {
            return Err(Error::Unsafe);
        }
        let receipt = read_child(&artifact, "receipt.toml", self.uid, 16384)?;
        let table: toml::Table = std::str::from_utf8(&receipt)
            .map_err(|_| Error::Corrupt)?
            .parse()
            .map_err(|_| Error::Corrupt)?;
        if table.get("build_policy").and_then(toml::Value::as_str) != Some("LibraryBuildV1") {
            return Ok(None);
        }
        validate_receipt(&table, record, metadata)?;
        let source = read_child(&artifact, "source.tar", self.uid, MAX_PACKAGE_BYTES)?;
        if source.len() as u64 != metadata.bytes() || Sha256::digest(&source) != *metadata.sha256()
        {
            return Err(Error::Corrupt);
        }
        super::retention::same_child(&job, "artifact", &artifact)?;
        super::retention::same_child(&root, name, &job)?;
        if self.read_snapshot(&owned_name, 64)?.as_deref() != Some(owned.as_slice()) {
            return Err(Error::Changed);
        }
        self.check_storage()?;
        Ok(Some(LibraryPackage { source, receipt }))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn validate_receipt(
    table: &toml::Table,
    record: &Record,
    metadata: &ArtifactMetadata,
) -> Result<(), Error> {
    let keys = [
        "version",
        "job",
        "project",
        "repository",
        "commit",
        "source_sha256",
        "review_sha256",
        "artifact_sha256",
        "artifact_bytes",
        "artifact",
        "toolchain_sha256",
        "dependency_sha256",
        "configuration_sha256",
        "build_policy",
        "target",
        "package",
        "package_version",
        "package_format",
    ];
    if table.len() != keys.len()
        || keys.iter().any(|k| !table.contains_key(*k))
        || table["version"].as_integer() != Some(2)
        || table["artifact_bytes"]
            .as_integer()
            .and_then(|n| u64::try_from(n).ok())
            != Some(metadata.bytes())
    {
        return Err(Error::Corrupt);
    }
    for (key, expected) in [
        ("job", hex(&record.id.to_bytes())),
        ("project", hex(&record.project)),
        ("repository", hex(&record.repository)),
        ("commit", record.commit.clone()),
        ("source_sha256", hex(&record.source)),
        ("artifact_sha256", hex(metadata.sha256().bytes())),
        ("artifact", "source.tar".into()),
        ("target", "native+wasm32-wasip2".into()),
        ("package_format", "ustar-source-v1".into()),
    ] {
        if table[key].as_str() != Some(expected.as_str()) {
            return Err(Error::Corrupt);
        }
    }
    for key in [
        "review_sha256",
        "toolchain_sha256",
        "dependency_sha256",
        "configuration_sha256",
    ] {
        Sha256::from_hex(table[key].as_str().ok_or(Error::Corrupt)?).map_err(|_| Error::Corrupt)?;
    }
    for key in ["package", "package_version"] {
        let value = table[key].as_str().ok_or(Error::Corrupt)?;
        if value.is_empty()
            || value.len() > 128
            || !value.is_ascii()
            || value.bytes().any(|b| b.is_ascii_control())
        {
            return Err(Error::Corrupt);
        }
    }
    Ok(())
}

fn read_child(parent: &File, name: &str, uid: u32, max: usize) -> Result<Vec<u8>, Error> {
    let mut file =
        File::from(openat(parent, name, READ, Mode::empty()).map_err(|_| Error::Unsafe)?);
    check_file(&file, uid, max as u64)?;
    let before = observation(&file)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Unavailable)?;
    if before != observation(&file)? || bytes.len() as u64 != before.2 {
        return Err(Error::Changed);
    }
    super::retention::same_child(parent, name, &file)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::RunnerEvent;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
        path::PathBuf,
    };

    struct Fixture {
        _temp: tempfile::TempDir,
        history: History,
        work: PathBuf,
        artifact: PathBuf,
        id: JobId,
        receipt: Vec<u8>,
        source: Vec<u8>,
    }
    fn private(path: &Path) {
        fs::create_dir(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fn write(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let base = temp.path().canonicalize().unwrap();
            private(&base.join("history"));
            private(&base.join("work"));
            let mut history = History::open(base.join("history").to_str().unwrap()).unwrap();
            let mut job = history.queue(crate::review_context::tests::plan()).unwrap();
            let work = base.join("work");
            let directory = work.join("ab".repeat(16));
            private(&directory);
            private(&directory.join("artifact"));
            history.bind_work(job.id(), &work, &directory).unwrap();
            let source = crate::library::source_package(
                &crate::source_snapshot::tests::ProofFixture::library("rlib")
                    .verify()
                    .unwrap(),
            )
            .unwrap();
            let metadata =
                ArtifactMetadata::new(Sha256::digest(&source), source.len() as u64).unwrap();
            job.apply(RunnerEvent::observed(&job, State::Running))
                .unwrap();
            history.record(&job).unwrap();
            job.apply(RunnerEvent::observed(
                &job,
                State::Succeeded(metadata.clone()),
            ))
            .unwrap();
            history.record(&job).unwrap();
            let record = history.load().unwrap().remove(0);
            let mut receipt = toml::Table::new();
            for (key, value) in [
                ("job", hex(&record.id.to_bytes())),
                ("project", hex(&record.project)),
                ("repository", hex(&record.repository)),
                ("commit", record.commit.clone()),
                ("source_sha256", hex(&record.source)),
                ("artifact_sha256", hex(metadata.sha256().bytes())),
                ("artifact", "source.tar".into()),
                ("target", "native+wasm32-wasip2".into()),
                ("package_format", "ustar-source-v1".into()),
                ("package", "fixture".into()),
                ("package_version", "0.1.0".into()),
                ("build_policy", "LibraryBuildV1".into()),
                ("toolchain_sha256", "12".repeat(32)),
                ("dependency_sha256", "34".repeat(32)),
                ("configuration_sha256", "56".repeat(32)),
                ("review_sha256", "78".repeat(32)),
            ] {
                receipt.insert(key.into(), toml::Value::String(value));
            }
            receipt.insert("version".into(), toml::Value::Integer(2));
            receipt.insert(
                "artifact_bytes".into(),
                toml::Value::Integer(source.len() as i64),
            );
            // This crate's TOML feature set intentionally supports parsing only.
            let receipt = receipt
                .iter()
                .map(|(k, v)| match v {
                    toml::Value::String(v) => format!("{k} = {v:?}\n"),
                    toml::Value::Integer(v) => format!("{k} = {v}\n"),
                    _ => unreachable!(),
                })
                .collect::<String>()
                .into_bytes();
            let artifact = directory.join("artifact");
            write(&artifact.join("source.tar"), &source);
            write(&artifact.join("receipt.toml"), &receipt);
            Self {
                _temp: temp,
                history,
                work,
                artifact,
                id: job.id(),
                receipt,
                source,
            }
        }
        fn read(&self) -> Result<Option<LibraryPackage>, Error> {
            self.history.library_package(self.id, &self.work)
        }
    }
    #[test]
    fn retained_package_is_owned_and_bound_to_success_and_exact_receipt() {
        let f = Fixture::new();
        let package = f.read().unwrap().unwrap();
        assert_eq!(package.source, f.source);
        assert_eq!(package.receipt, f.receipt);
        write(&f.artifact.join("source.tar"), b"changed");
        assert_eq!(package.source, f.source);
        assert!(f.read().is_err());
        assert!(
            f.history
                .library_package(JobId::from_bytes([0; 16]), &f.work)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn substituted_provenance_and_unsafe_archive_files_are_refused() {
        let f = Fixture::new();
        let text = std::str::from_utf8(&f.receipt).unwrap();
        for key in [
            "job",
            "project",
            "repository",
            "commit",
            "source_sha256",
            "artifact_sha256",
            "target",
            "package_format",
        ] {
            let changed = text
                .lines()
                .map(|line| {
                    if line.starts_with(&format!("{key} =")) {
                        format!("{key} = \"wrong\"\n")
                    } else {
                        format!("{line}\n")
                    }
                })
                .collect::<String>();
            write(&f.artifact.join("receipt.toml"), changed.as_bytes());
            assert!(f.read().is_err(), "{key}");
        }
        write(&f.artifact.join("receipt.toml"), &f.receipt);
        let original = f.artifact.join("source.tar");
        let outside = f.work.join("outside");
        fs::rename(&original, &outside).unwrap();
        symlink(&outside, &original).unwrap();
        assert!(f.read().is_err());
        fs::remove_file(&original).unwrap();
        fs::hard_link(&outside, &original).unwrap();
        assert!(f.read().is_err());
        fs::remove_file(&original).unwrap();
        fs::rename(&outside, &original).unwrap();
        fs::set_permissions(&original, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(f.read().is_err());
    }
    #[test]
    fn renamed_owned_work_and_app_receipts_cannot_select_other_packages() {
        let f = Fixture::new();

        let app = std::str::from_utf8(&f.receipt)
            .unwrap()
            .replace("LibraryBuildV1", "WasmBuildV1");
        write(&f.artifact.join("receipt.toml"), app.as_bytes());
        assert!(f.read().unwrap().is_none());
        write(&f.artifact.join("receipt.toml"), &f.receipt);
        let wrong = f.work.with_file_name("other-work");
        private(&wrong);
        assert!(f.history.library_package(f.id, &wrong).is_err());
        let job = f.artifact.parent().unwrap();
        let saved = job.with_file_name("saved");
        fs::rename(job, &saved).unwrap();
        private(job);
        private(&job.join("artifact"));
        assert!(f.read().is_err());
    }
    #[test]
    fn pruned_attempt_cannot_reuse_its_old_package_binding() {
        let mut f = Fixture::new();
        let plan = crate::review_context::tests::plan();
        // Keep the same persisted catalog identity as the original record.
        let mut plan = plan;
        plan.0.project =
            devoption_projects::ProjectId::from_bytes(f.history.load().unwrap()[0].project);
        let project = plan.details().project;
        let mut newer = f.history.queue(plan).unwrap();
        newer.cancel_queued().unwrap();
        f.history.record(&newer).unwrap();
        f.history
            .set_retention_policy(project, crate::retention::Policy::new(1).unwrap())
            .unwrap();
        let status = f
            .history
            .reconcile_retention(project, Some(&f.work), || Ok(vec![]))
            .unwrap();
        assert!(!status.failed && status.removed == 1);
        assert!(f.read().unwrap().is_none());
        assert!(!f.artifact.exists());
    }
}
