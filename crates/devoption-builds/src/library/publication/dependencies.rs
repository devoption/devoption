//! Exact locked archive acquisition, never extraction, vendor proof or execution.
use super::*;
use crate::source_snapshot::SourceSnapshot;
use devoption_registry::{CargoDependencyRequest, CargoOrigin, LockedPackage, cargo_inputs};

pub const MAX_DEPENDENCY_ARCHIVE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_LOCKED_ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
const PUBLIC_INDEX: &str = "https://github.com/rust-lang/crates.io-index";

/// Operator-supplied public bytes only. Metadata must exactly match a public
/// package in the verified source request. Never a fallback for hosted origins.
pub struct PublicArchive<'a> {
    pub package: &'a LockedPackage,
    pub bytes: &'a [u8],
}
/// One unchanged locked archive and optional approved hosted index line.
/// No Debug implementation exposes private metadata or code.
#[derive(PartialEq, Eq)]
pub struct LockedArchive {
    package: LockedPackage,
    bytes: Vec<u8>,
    index: Option<Vec<u8>>,
}
impl LockedArchive {
    pub fn package(&self) -> &LockedPackage {
        &self.package
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Current approved exact-version metadata for hosted packages; public
    /// archives are bound to the source lock checksum, not an invented index.
    pub fn index_line(&self) -> Option<&[u8]> {
        self.index.as_deref()
    }
}
/// Owns the complete admitted lock archive set. No Debug implementation exposes
/// private metadata/code. This is captured availability, not a transferable read
/// grant, build approval, verified vendor tree or proof of public package manifests.
pub struct LockedArchives {
    project: ProjectId,
    source: Sha256,
    request: CargoDependencyRequest,
    head: Sha256,
    catalog: Catalog,
    archives: Vec<LockedArchive>,
    fingerprint: Sha256,
}
impl LockedArchives {
    pub fn project(&self) -> ProjectId {
        self.project
    }
    pub fn source_sha256(&self) -> Sha256 {
        self.source
    }
    pub fn request(&self) -> &CargoDependencyRequest {
        &self.request
    }
    pub fn archives(&self) -> &[LockedArchive] {
        &self.archives
    }
    /// Binds project/source/request/registry mappings, policy head and all exact
    /// archive/index bytes. Current catalog equality is checked separately.
    pub fn fingerprint(&self) -> Sha256 {
        self.fingerprint
    }
}

impl Publications {
    /// Caller must authenticate the configured owner and capture this consumer's
    /// fresh registered source under catalog/registry serialization. Every hosted
    /// node needs its own current grant; public bytes must be supplied explicitly.
    /// No filesystem writes, network, decompression, Cargo or workloads occur.
    pub fn locked_archives(
        &self,
        actor: OwnerId,
        project: ProjectId,
        catalog: &Catalog,
        candidates: &PackageStore,
        source: &SourceSnapshot,
        public: &[PublicArchive<'_>],
    ) -> Result<LockedArchives, Error> {
        self.actor(actor)?;
        if !catalog.projects().iter().any(|p| p.id() == project) {
            return Err(Error::Denied);
        }
        let request = source.request();
        let state = self.state()?;
        if state.events.len() >= MAX_POLICY_EVENTS {
            return Err(Error::Capacity);
        }
        let source_file = |name| {
            source
                .files()
                .iter()
                .find(|f| f.path() == name)
                .map(|f| f.bytes())
                .ok_or(Error::Integrity)
        };
        let inspected = cargo_inputs::inspect_with_registries(
            source_file("Cargo.toml")?,
            source_file("Cargo.lock")?,
            request.hosted_registries(),
        )
        .map_err(|_| Error::Integrity)?;
        if inspected.hashes().manifest != request.manifest_sha256()
            || inspected.hashes().lockfile != request.lockfile_sha256()
        {
            return Err(Error::Integrity);
        }
        let mut acquired: Vec<Option<LockedArchive>> =
            (0..request.packages().len()).map(|_| None).collect();
        let mut total = 0;
        if public.len() > request.packages().len() {
            return Err(Error::Invalid);
        }
        for input in public {
            if input.package.origin != CargoOrigin::CratesIo {
                return Err(Error::Invalid);
            }
            let at = request
                .packages()
                .iter()
                .position(|p| p == input.package)
                .ok_or(Error::Invalid)?;
            if acquired[at].is_some() {
                return Err(Error::Invalid);
            }
            verify_archive(input.bytes, input.package.archive_sha256, &mut total)?;
            acquired[at] = Some(LockedArchive {
                package: input.package.clone(),
                bytes: input.bytes.to_vec(),
                index: None,
            });
        }
        let mut hosted = Vec::new();
        for (at, package) in request.packages().iter().enumerate() {
            let CargoOrigin::DevOption(id) = &package.origin else {
                if acquired[at].is_none() {
                    return Err(Error::Unavailable);
                }
                continue;
            };
            let binding = request
                .hosted_registries()
                .bindings()
                .iter()
                .find(|b| b.id() == id)
                .ok_or(Error::Denied)?;
            let c = Coordinates::new(binding.clone(), &package.name, &package.version)
                .map_err(|_| Error::Invalid)?;
            let v = state
                .find(&c)
                .filter(|v| v.coordinates == c && v.readers.contains(&project))
                .ok_or(Error::Denied)?;
            check_source(catalog, v.source, v.repository)?;
            hosted.push((at, v));
        }
        if !hosted.is_empty() {
            candidates.visit(|candidate| -> Result<(), Error> {
                let Some(&(at, version)) = hosted
                    .iter()
                    .find(|(_, v)| v.coordinates == *candidate.source().coordinates())
                else {
                    return Ok(());
                };
                if acquired[at].is_some() {
                    return Err(Error::Integrity);
                }
                verify_candidate(&candidate, version)?;
                let package = &request.packages()[at];
                verify_archive(candidate.archive(), package.archive_sha256, &mut total)?;
                let edges = inspected
                    .package_dependencies()
                    .iter()
                    .find(|e| e.package() == package)
                    .ok_or(Error::Integrity)?;
                check_edges(
                    candidate.index_line(),
                    edges.dependencies(),
                    request,
                    version.coordinates.registry(),
                )?;
                let index = super::index::line(&candidate, version.yanked)?;
                budget(index.len(), &mut total)?;
                acquired[at] = Some(LockedArchive {
                    package: package.clone(),
                    bytes: candidate.archive().to_vec(),
                    index: Some(index),
                });
                Ok(())
            })?;
        }
        let archives = acquired
            .into_iter()
            .map(|p| p.ok_or(Error::Unavailable))
            .collect::<Result<Vec<_>, _>>()?;
        let mut result = LockedArchives {
            project,
            source: source.sha256(),
            request: request.clone(),
            head: state.head(),
            catalog: catalog.clone(),
            archives,
            fingerprint: Sha256::digest(b""),
        };
        result.fingerprint = fingerprint(&result);
        Ok(result)
    }
    /// Freshly reacquire the exact set before a later review/execution boundary.
    /// Any catalog/policy/source change invalidates the captured view. Already
    /// copied bytes cannot be retracted, and this grants no execution authority.
    pub fn revalidate_archives(
        &self,
        actor: OwnerId,
        catalog: &Catalog,
        candidates: &PackageStore,
        source: &SourceSnapshot,
        captured: &LockedArchives,
    ) -> Result<(), Error> {
        self.actor(actor)?;
        if *catalog != captured.catalog
            || source.sha256() != captured.source
            || source.request() != &captured.request
        {
            return Err(Error::Changed);
        }
        let public: Vec<_> = captured
            .archives
            .iter()
            .filter(|a| a.package.origin == CargoOrigin::CratesIo)
            .map(|a| PublicArchive {
                package: &a.package,
                bytes: &a.bytes,
            })
            .collect();
        let current = self.locked_archives(
            actor,
            captured.project,
            catalog,
            candidates,
            source,
            &public,
        )?;
        if current.head != captured.head
            || current.fingerprint != captured.fingerprint
            || current.archives != captured.archives
        {
            return Err(Error::Changed);
        }
        Ok(())
    }
}
fn budget(bytes: usize, total: &mut usize) -> Result<(), Error> {
    if bytes > MAX_LOCKED_ARCHIVE_BYTES - *total {
        return Err(Error::Capacity);
    }
    *total += bytes;
    Ok(())
}
fn verify_archive(bytes: &[u8], expected: Sha256, total: &mut usize) -> Result<(), Error> {
    if bytes.is_empty() {
        return Err(Error::Unavailable);
    }
    if bytes.len() > MAX_DEPENDENCY_ARCHIVE_BYTES {
        return Err(Error::Capacity);
    }
    if Sha256::digest(bytes) != expected {
        return Err(Error::Integrity);
    }
    budget(bytes.len(), total)
}
fn check_edges(
    index: &[u8],
    expected: &[LockedPackage],
    request: &CargoDependencyRequest,
    registry: &HostedRegistry,
) -> Result<(), Error> {
    let value: Value = serde_json::from_slice(index).map_err(|_| Error::Integrity)?;
    let deps = value["deps"].as_array().ok_or(Error::Integrity)?;
    if deps.len() != expected.len() {
        return Err(Error::Integrity);
    }
    let mut found = Vec::new();
    for dep in deps {
        if dep["kind"] != "normal"
            || dep["optional"] != false
            || !dep["target"].is_null()
            || dep["features"] != json!([])
            || !dep["default_features"].is_boolean()
        {
            return Err(Error::Integrity);
        }
        let name = dep["name"].as_str().ok_or(Error::Integrity)?;
        let version = dep["req"]
            .as_str()
            .and_then(|s| s.strip_prefix('='))
            .ok_or(Error::Integrity)?;
        let origin = match &dep["registry"] {
            Value::Null => CargoOrigin::DevOption(registry.id().clone()),
            Value::String(index) if index == PUBLIC_INDEX => CargoOrigin::CratesIo,
            Value::String(index) => CargoOrigin::DevOption(
                request
                    .hosted_registries()
                    .bindings()
                    .iter()
                    .find(|b| b.index() == index)
                    .ok_or(Error::Denied)?
                    .id()
                    .clone(),
            ),
            _ => return Err(Error::Integrity),
        };
        let at = expected
            .iter()
            .position(|p| {
                p.origin == origin && p.name == name && release(&p.version) == release(version)
            })
            .ok_or(Error::Integrity)?;
        if found.contains(&at) {
            return Err(Error::Integrity);
        }
        found.push(at);
    }
    Ok(())
}
fn release(version: &str) -> &str {
    version.split('+').next().unwrap_or(version)
}
fn fingerprint(value: &LockedArchives) -> Sha256 {
    fn frame(out: &mut Vec<u8>, bytes: &[u8]) {
        out.extend((bytes.len() as u64).to_be_bytes());
        out.extend(bytes);
    }
    let mut bytes = b"devoption.locked-archives.v1\0".to_vec();
    for part in [
        value.project.to_bytes().as_slice(),
        value.source.bytes(),
        value.head.bytes(),
        value.request.registry().as_str().as_bytes(),
        value.request.manifest_sha256().bytes(),
        value.request.lockfile_sha256().bytes(),
    ] {
        frame(&mut bytes, part);
    }
    frame(
        &mut bytes,
        &(value.request.hosted_registries().bindings().len() as u64).to_be_bytes(),
    );
    for b in value.request.hosted_registries().bindings() {
        frame(&mut bytes, b.id().as_str().as_bytes());
        frame(&mut bytes, b.index().as_bytes());
    }
    frame(&mut bytes, &(value.archives.len() as u64).to_be_bytes());
    for a in &value.archives {
        match &a.package.origin {
            CargoOrigin::CratesIo => frame(&mut bytes, b"public"),
            CargoOrigin::DevOption(id) => {
                frame(&mut bytes, b"hosted");
                frame(&mut bytes, id.as_str().as_bytes());
            }
        }
        for part in [
            a.package.name.as_bytes(),
            a.package.version.as_bytes(),
            a.package.archive_sha256.bytes(),
            &(a.bytes.len() as u64).to_be_bytes(),
        ] {
            frame(&mut bytes, part);
        }
        if let Some(index) = &a.index {
            frame(&mut bytes, b"index");
            frame(&mut bytes, Sha256::digest(index).bytes());
        } else {
            frame(&mut bytes, b"no-index");
        }
    }
    Sha256::digest(&bytes)
}

#[cfg(test)]
pub(in crate::library) mod tests;
