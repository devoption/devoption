//! Captured successful-library-build evidence, never publication authority.
use super::staging::StagedSource;
use crate::jobs::JobId;
use devoption_registry::Sha256;

pub(crate) const MAX_RECEIPT_BYTES: usize = 16384;

/// Exact receipt captured only after retained history/source verification.
/// This is local provenance, not a signature or independently trusted attestation.
/// No Debug implementation exposes private receipt data.
#[derive(Clone, PartialEq, Eq)]
pub struct BuildProvenance {
    attempt: JobId,
    receipt: Vec<u8>,
}
impl BuildProvenance {
    pub(crate) fn new(attempt: JobId, receipt: Vec<u8>, source: &StagedSource) -> Result<Self, ()> {
        if receipt.len() > MAX_RECEIPT_BYTES {
            return Err(());
        }
        let table = std::str::from_utf8(&receipt)
            .map_err(|_| ())?
            .parse()
            .map_err(|_| ())?;
        validate(
            &table,
            Binding {
                attempt,
                project: source.project(),
                repository: source.repository(),
                commit: source.commit(),
                source: source.source_sha256().bytes(),
                artifact: source.archive_sha256(),
                artifact_bytes: source.archive().len() as u64,
                package: Some((source.coordinates().name(), source.coordinates().version())),
            },
        )?;
        Ok(Self { attempt, receipt })
    }
    pub fn attempt(&self) -> JobId {
        self.attempt
    }
    pub fn receipt(&self) -> &[u8] {
        &self.receipt
    }
}

pub(crate) struct Binding<'a> {
    pub attempt: JobId,
    pub project: [u8; 16],
    pub repository: [u8; 16],
    pub commit: &'a str,
    pub source: &'a [u8; 32],
    pub artifact: Sha256,
    pub artifact_bytes: u64,
    pub package: Option<(&'a str, &'a str)>,
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub(crate) fn validate(table: &toml::Table, binding: Binding<'_>) -> Result<(), ()> {
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
            != Some(binding.artifact_bytes)
    {
        return Err(());
    }
    for (key, expected) in [
        ("job", hex(&binding.attempt.to_bytes())),
        ("project", hex(&binding.project)),
        ("repository", hex(&binding.repository)),
        ("commit", binding.commit.to_owned()),
        ("source_sha256", hex(binding.source)),
        ("artifact_sha256", hex(binding.artifact.bytes())),
        ("artifact", "source.tar".into()),
        ("target", "native+wasm32-wasip2".into()),
        ("package_format", "ustar-source-v1".into()),
    ] {
        if table[key].as_str() != Some(expected.as_str()) {
            return Err(());
        }
    }
    for key in [
        "review_sha256",
        "toolchain_sha256",
        "dependency_sha256",
        "configuration_sha256",
    ] {
        Sha256::from_hex(table[key].as_str().ok_or(())?).map_err(|_| ())?;
    }
    for key in ["package", "package_version"] {
        let value = table[key].as_str().ok_or(())?;
        if value.is_empty()
            || value.len() > 128
            || !value.is_ascii()
            || value.bytes().any(|b| b.is_ascii_control())
        {
            return Err(());
        }
    }
    if let Some((name, version)) = binding.package
        && (table["package"].as_str() != Some(name)
            || table["package_version"].as_str() != Some(version))
    {
        return Err(());
    }
    if table["build_policy"].as_str() != Some("LibraryBuildV1") {
        return Err(());
    }
    Ok(())
}
