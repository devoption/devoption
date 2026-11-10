//! Checked immutable vendor bytes. No disk publication, network or execution.
use super::publication::{Error, Publications, dependencies::LockedArchives};
use crate::{approvals::OwnerId, source_snapshot::SourceSnapshot};
use devoption_projects::Catalog;
use devoption_registry::{
    CargoDependencyRequest, CargoMaterialization, CargoOrigin, CargoRegistryProvider,
    HostedRegistry, LockedPackage, Sha256,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path},
};
mod archive;
mod manifest;
pub const MAX_PACKAGE_TAR_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PACKAGE_ENTRIES: usize = 8192;
pub const MAX_VENDOR_TAR_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_VENDOR_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_VENDOR_FILES: usize = 16384;
pub const MAX_VENDOR_BUNDLE_BYTES: usize = MAX_VENDOR_BYTES + MAX_VENDOR_FILES * (512 + 14) + 13;
const MODE: u16 = 0o644;

/// Source replacement metadata, not permission to acquire a registry package.
/// No Debug implementation exposes private registry identities.
pub struct VendorSource {
    origin: CargoOrigin,
    binding: Option<HostedRegistry>,
    directory: String,
}
impl VendorSource {
    pub fn origin(&self) -> &CargoOrigin {
        &self.origin
    }
    pub fn directory(&self) -> &str {
        &self.directory
    }
}
struct Entry {
    path: String,
    start: usize,
    end: usize,
}
/// A regular read-only view of one verified output file. OS publication must
/// separately use exclusive/no-follow writes and revalidate the completed tree.
pub struct VendorFile<'a> {
    path: &'a str,
    bytes: &'a [u8],
}
impl VendorFile<'_> {
    pub fn path(&self) -> &str {
        self.path
    }
    pub fn bytes(&self) -> &[u8] {
        self.bytes
    }
    pub fn mode(&self) -> u16 {
        MODE
    }
}
/// Owned canonical DVVENDOR1 bytes and matching descriptor; not a build approval
/// or an already installed vendor directory. No Debug implementation exposes code.
pub struct VendorInputs {
    materialization: CargoMaterialization,
    captured: Sha256,
    sources: Vec<VendorSource>,
    entries: Vec<Entry>,
    bundle: Vec<u8>,
}
impl VendorInputs {
    pub fn materialization(&self) -> &CargoMaterialization {
        &self.materialization
    }
    pub fn archive_fingerprint(&self) -> Sha256 {
        self.captured
    }
    pub fn bundle_bytes(&self) -> &[u8] {
        &self.bundle
    }
    pub fn sources(&self) -> &[VendorSource] {
        &self.sources
    }
    pub fn files(&self) -> impl Iterator<Item = VendorFile<'_>> {
        self.entries.iter().map(|e| VendorFile {
            path: &e.path,
            bytes: &self.bundle[e.start..e.end],
        })
    }
    /// Deterministic Cargo source replacement for a trusted absolute mount. The
    /// chosen runtime path/configuration must be bound by a later build review.
    pub fn cargo_config(&self, root: &Path) -> Result<String, Error> {
        let path = root.to_str().ok_or(Error::Invalid)?;
        if !root.is_absolute()
            || path.len() > 4096
            || !path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
            || root
                .components()
                .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        {
            return Err(Error::Invalid);
        }
        let quote = |s: &str| serde_json::to_string(s).map_err(|_| Error::Invalid);
        let mut reserved: BTreeSet<_> = self
            .sources
            .iter()
            .filter_map(|s| s.binding.as_ref().map(|b| b.id().as_str().to_owned()))
            .collect();
        reserved.insert("crates-io".into());
        let mut out = String::new();
        for (n, source) in self.sources.iter().enumerate() {
            let mut replacement = format!("devoption-vendor-{n}");
            while !reserved.insert(replacement.clone()) {
                replacement.push_str("-x");
            }
            match &source.binding {
                None => {
                    out += &format!(
                        "[source.crates-io]\nreplace-with={}\n",
                        quote(&replacement)?
                    )
                }
                Some(b) => {
                    out += &format!(
                        "[registries.{}]\nindex={}\n[source.{}]\nregistry={}\nreplace-with={}\n",
                        quote(b.id().as_str())?,
                        quote(b.index())?,
                        quote(b.id().as_str())?,
                        quote(b.index())?,
                        quote(&replacement)?
                    );
                }
            }
            let directory = root.join(&source.directory);
            out += &format!(
                "[source.{}]\ndirectory={}\n",
                quote(&replacement)?,
                quote(directory.to_str().ok_or(Error::Invalid)?)?
            );
        }
        Ok(out)
    }
    /// Recheck policy/source before a later handoff. A matching descriptor alone
    /// cannot establish current permission or the integrity of a published tree.
    pub fn revalidate(
        &self,
        policy: &Publications,
        actor: OwnerId,
        catalog: &Catalog,
        candidates: &super::package_store::PackageStore,
        source: &SourceSnapshot,
        captured: &LockedArchives,
    ) -> Result<(), Error> {
        if self.captured != captured.fingerprint()
            || self.materialization.request() != captured.request()
        {
            return Err(Error::Changed);
        }
        policy.revalidate_archives(actor, catalog, candidates, source, captured)
    }
}
impl CargoRegistryProvider for VendorInputs {
    fn materialize(
        &self,
        request: &CargoDependencyRequest,
    ) -> Result<CargoMaterialization, devoption_registry::Error> {
        if request != self.materialization.request() {
            return Err(devoption_registry::Error::MismatchedRequest);
        }
        Ok(self.materialization.clone())
    }
}
impl Publications {
    /// Authenticate/serialize fresh source and catalog selection as for locked
    /// acquisition. This verifies archive content and a bounded normal-dependency
    /// manifest profile, then constructs owned vendor bytes without disk writes.
    pub fn vendor_inputs(
        &self,
        actor: OwnerId,
        catalog: &Catalog,
        candidates: &super::package_store::PackageStore,
        source: &SourceSnapshot,
        captured: &LockedArchives,
    ) -> Result<VendorInputs, Error> {
        self.revalidate_archives(actor, catalog, candidates, source, captured)?;
        verify(source, captured)
    }
}
fn verify(source: &SourceSnapshot, captured: &LockedArchives) -> Result<VendorInputs, Error> {
    let request = captured.request();
    let source_file = |name| {
        source
            .files()
            .iter()
            .find(|f| f.path() == name)
            .map(|f| f.bytes())
            .ok_or(Error::Integrity)
    };
    let graph = devoption_registry::cargo_inputs::inspect_with_registries(
        source_file("Cargo.toml")?,
        source_file("Cargo.lock")?,
        request.hosted_registries(),
    )
    .map_err(|_| Error::Integrity)?;
    let mut sources = vec![VendorSource {
        origin: CargoOrigin::CratesIo,
        binding: None,
        directory: "public".into(),
    }];
    for b in request.hosted_registries().bindings() {
        let mut identity = b.id().as_str().as_bytes().to_vec();
        identity.push(0);
        identity.extend(b.index().as_bytes());
        sources.push(VendorSource {
            origin: CargoOrigin::DevOption(b.id().clone()),
            binding: Some(b.clone()),
            directory: format!("hosted-{}", hex(Sha256::digest(&identity))),
        });
    }
    let mut all = BTreeMap::new();
    let mut total = 0;
    let mut remaining_tar = MAX_VENDOR_TAR_BYTES;
    for archive in captured.archives() {
        let package = archive.package();
        let mut files = archive::read(archive.bytes(), package, &mut remaining_tar)?;
        let edges = graph
            .package_dependencies()
            .iter()
            .find(|e| e.package() == package)
            .ok_or(Error::Integrity)?;
        manifest::check(&files, package, edges.dependencies(), request)?;
        let hashes: BTreeMap<_, _> = files
            .iter()
            .map(|(p, b)| (p.clone(), hex(Sha256::digest(b))))
            .collect();
        let checksums = serde_json::to_vec(
            &serde_json::json!({"package":hex(package.archive_sha256),"files":hashes}),
        )
        .map_err(|_| Error::Invalid)?;
        files.insert(".cargo-checksum.json".into(), checksums);
        let directory = &sources
            .iter()
            .find(|s| s.origin == package.origin)
            .ok_or(Error::Invalid)?
            .directory;
        for (path, bytes) in files {
            if bytes.len() > MAX_VENDOR_BYTES - total || all.len() >= MAX_VENDOR_FILES {
                return Err(Error::Capacity);
            }
            total += bytes.len();
            let path = format!("{directory}/{}-{}/{path}", package.name, package.version);
            if path.len() > 512 || all.insert(path, bytes).is_some() {
                return Err(Error::Invalid);
            }
        }
    }
    let mut bundle = b"DVVENDOR1".to_vec();
    bundle.extend((all.len() as u32).to_be_bytes());
    let mut entries = Vec::new();
    for (path, bytes) in all {
        bundle.extend((path.len() as u32).to_be_bytes());
        bundle.extend(path.as_bytes());
        bundle.extend(MODE.to_be_bytes());
        bundle.extend((bytes.len() as u64).to_be_bytes());
        let start = bundle.len();
        bundle.extend(bytes);
        entries.push(Entry {
            path,
            start,
            end: bundle.len(),
        });
    }
    if bundle.len() > MAX_VENDOR_BUNDLE_BYTES {
        return Err(Error::Capacity);
    }
    let materialization = CargoMaterialization::new(
        request.clone(),
        Sha256::digest(&bundle),
        bundle.len() as u64,
    );
    Ok(VendorInputs {
        materialization,
        captured: captured.fingerprint(),
        sources,
        entries,
        bundle,
    })
}
fn hex(value: Sha256) -> String {
    value.bytes().iter().map(|b| format!("{b:02x}")).collect()
}
fn safe_path(path: &str) -> Result<(), Error> {
    if path.is_empty()
        || path.len() > 240
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-+/".contains(&b))
        || path.split('/').count() > 24
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.len() > 255)
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
#[cfg(test)]
pub(crate) mod tests;
