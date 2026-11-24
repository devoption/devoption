//! Deterministic Cargo artifacts from verified library plans; no publication/I/O.
use super::{MAX_PACKAGE_BYTES, source_tar, staging::StagedSource};
use crate::BuildPlan;
use devoption_registry::{
    CargoOrigin, HostedRegistries, HostedRegistry, RegistryId, Sha256,
    cargo_inputs::{self, CRATES_IO_INDEX, DirectDependency},
};
use flate2::{Compression, GzBuilder};
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{self, Write},
};

const PUBLIC_INDEX: &str = CRATES_IO_INDEX;
pub const MAX_INDEX_LINE_BYTES: usize = 256 * 1024;

/// Owned archive/index bytes bound to source-stage provenance. `from_plan` does
/// not attest build success; History may attach verified retained-build evidence.
/// Neither path authorizes publication, serving or execution.
/// No Debug implementation exposes private metadata, code or registry endpoints.
#[derive(Clone, PartialEq, Eq)]
pub struct CargoPackage {
    source: StagedSource,
    archive: Vec<u8>,
    sha256: Sha256,
    index_path: String,
    index_line: Vec<u8>,
    provenance: Option<super::build_provenance::BuildProvenance>,
}
impl CargoPackage {
    pub fn from_plan(destination: HostedRegistry, plan: &BuildPlan) -> io::Result<Self> {
        let input = plan.details();
        Self::from_source(
            destination,
            input.project.to_bytes(),
            input.repository.to_bytes(),
            &input.source,
        )
    }
    pub(crate) fn from_source(
        destination: HostedRegistry,
        project: [u8; 16],
        repository: [u8; 16],
        source: &crate::source_snapshot::SourceSnapshot,
    ) -> io::Result<Self> {
        let staged = StagedSource::from_source(destination.clone(), project, repository, source)
            .map_err(|_| invalid())?;
        if source.files().iter().any(|file| {
            [
                "Cargo.toml.orig",
                ".cargo_vcs_info.json",
                ".cargo-checksum.json",
            ]
            .iter()
            .any(|reserved| {
                file.path()
                    .split('/')
                    .next()
                    .is_some_and(|part| part.eq_ignore_ascii_case(reserved))
            })
        }) {
            return Err(invalid());
        }
        let original = source
            .files()
            .iter()
            .find(|file| file.path() == "Cargo.toml")
            .ok_or_else(invalid)?
            .bytes();
        let registries = source.request().hosted_registries();
        let normalized = normalized_manifest(
            source.package(),
            source.version(),
            source.edition(),
            source.direct_dependencies(),
            registries,
        )?;
        let mut files: BTreeMap<&str, &[u8]> = source
            .files()
            .iter()
            .map(|file| (file.path(), file.bytes()))
            .collect();
        files.insert("Cargo.toml.orig", original);
        files.insert("Cargo.toml", normalized.as_bytes());
        let tar = source_tar(source.package(), source.version(), files)?;
        let mut encoder = GzBuilder::new()
            .mtime(0)
            .operating_system(255)
            .write(Vec::new(), Compression::new(6));
        encoder.write_all(&tar).map_err(|_| invalid())?;
        let archive = encoder.finish().map_err(|_| invalid())?;
        if archive.len() > MAX_PACKAGE_BYTES {
            return Err(invalid());
        }
        let sha256 = Sha256::digest(&archive);
        let index_line = index_metadata(&staged, sha256, source.direct_dependencies(), registries)?;
        let name = source.package().to_ascii_lowercase();
        let index_path = index_location(&name);
        Ok(Self {
            source: staged,
            archive,
            sha256,
            index_path,
            index_line,
            provenance: None,
        })
    }
    pub(super) fn encode(&self) -> Vec<u8> {
        let mut out = if self.provenance.is_some() {
            b"DVCARGO2"
        } else {
            b"DVCARGO1"
        }
        .to_vec();
        for bytes in [
            self.source.encode().as_slice(),
            self.archive.as_slice(),
            self.index_path.as_bytes(),
            self.index_line.as_slice(),
        ] {
            out.extend((bytes.len() as u32).to_be_bytes());
            out.extend(bytes);
        }
        if let Some(provenance) = &self.provenance {
            out.extend(provenance.attempt().to_bytes());
            out.extend((provenance.receipt().len() as u32).to_be_bytes());
            out.extend(provenance.receipt());
        }
        out.extend(Sha256::digest(&out).bytes());
        out
    }
    pub(super) fn decode(bytes: &[u8]) -> Result<Self, super::staging::Error> {
        use super::{record_store::Record, staging::Error};
        if bytes.len() < 40 || bytes.len() > <Self as Record>::MAX_BYTES {
            return Err(Error::Invalid);
        }
        let (body, digest) = bytes.split_at(bytes.len() - 32);
        if Sha256::digest(body).bytes() != digest {
            return Err(Error::Integrity);
        }
        let built = &body[..8] == b"DVCARGO2";
        if !built && &body[..8] != b"DVCARGO1" {
            return Err(Error::Invalid);
        }
        let mut rest = &body[8..];
        fn field<'a>(rest: &mut &'a [u8], max: usize) -> Result<&'a [u8], Error> {
            if rest.len() < 4 {
                return Err(Error::Invalid);
            }
            let n = u32::from_be_bytes(rest[..4].try_into().map_err(|_| Error::Invalid)?) as usize;
            if n == 0 || n > max || n > rest.len() - 4 {
                return Err(Error::Invalid);
            }
            let value = &rest[4..4 + n];
            *rest = &rest[4 + n..];
            Ok(value)
        }
        let source = StagedSource::decode(field(&mut rest, MAX_PACKAGE_BYTES + 4096)?)?;
        let archive = field(&mut rest, MAX_PACKAGE_BYTES)?.to_vec();
        let index_path = std::str::from_utf8(field(&mut rest, 512)?)
            .map_err(|_| Error::Invalid)?
            .to_owned();
        let index_line = field(&mut rest, MAX_INDEX_LINE_BYTES)?.to_vec();
        let provenance = if built {
            if rest.len() < 16 {
                return Err(Error::Invalid);
            }
            let attempt =
                crate::jobs::JobId::from_bytes(rest[..16].try_into().map_err(|_| Error::Invalid)?);
            rest = &rest[16..];
            let receipt = field(&mut rest, super::build_provenance::MAX_RECEIPT_BYTES)?.to_vec();
            Some(
                super::build_provenance::BuildProvenance::new(attempt, receipt, &source)
                    .map_err(|_| Error::Integrity)?,
            )
        } else {
            None
        };
        if !rest.is_empty() {
            return Err(Error::Invalid);
        }
        let sha256 = Sha256::digest(&archive);
        let coordinates = source.coordinates();
        let package = devoption_registry::LockedPackage {
            origin: devoption_registry::CargoOrigin::DevOption(coordinates.registry().id().clone()),
            name: coordinates.name().into(),
            version: coordinates.version().into(),
            archive_sha256: sha256,
        };
        // Reuse the bounded in-memory archive reader; never extract stored bytes.
        let mut remaining = MAX_PACKAGE_BYTES;
        let files = super::vendor::archive::read(&archive, &package, &mut remaining)
            .map_err(|_| Error::Integrity)?;
        // Reverse only the two generated manifest changes and require every
        // retained source byte/path to match the canonical source archive.
        let mut original_files = files.clone();
        let original = original_files
            .remove("Cargo.toml.orig")
            .ok_or(Error::Integrity)?;
        original_files.insert("Cargo.toml".into(), original.clone());
        let source_tar = source_tar(
            coordinates.name(),
            coordinates.version(),
            original_files
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        )
        .map_err(|_| Error::Integrity)?;
        if source_tar != source.archive() {
            return Err(Error::Integrity);
        }
        // The admitted lock retains each exact hosted dependency's index URL;
        // recover bindings there and revalidate original inputs through admission.
        let lock = files.get("Cargo.lock").ok_or(Error::Integrity)?;
        let registries = recover_registries(&original, lock)?;
        let inputs = cargo_inputs::inspect_with_registries(&original, lock, &registries)
            .map_err(|_| Error::Integrity)?;
        if inputs.package() != coordinates.name()
            || inputs.version() != coordinates.version()
            || !inputs.target().is_library()
        {
            return Err(Error::Integrity);
        }
        let normalized = normalized_manifest(
            inputs.package(),
            inputs.version(),
            inputs.edition(),
            inputs.direct_dependencies(),
            &registries,
        )
        .map_err(|_| Error::Integrity)?;
        if files.get("Cargo.toml").map(Vec::as_slice) != Some(normalized.as_bytes()) {
            return Err(Error::Integrity);
        }
        let expected = index_metadata(&source, sha256, inputs.direct_dependencies(), &registries)
            .map_err(|_| Error::Integrity)?;
        if index_path != index_location(&coordinates.name().to_ascii_lowercase())
            || index_line != expected
        {
            return Err(Error::Integrity);
        }
        Ok(Self {
            source,
            archive,
            sha256,
            index_path,
            index_line,
            provenance,
        })
    }
    /// Captured retained-build evidence, not publication or execution authority.
    pub fn build_provenance(&self) -> Option<&super::build_provenance::BuildProvenance> {
        self.provenance.as_ref()
    }
    pub(crate) fn bind_build(
        &mut self,
        attempt: crate::jobs::JobId,
        receipt: Vec<u8>,
    ) -> io::Result<()> {
        self.provenance = Some(
            super::build_provenance::BuildProvenance::new(attempt, receipt, &self.source)
                .map_err(|_| invalid())?,
        );
        Ok(())
    }
    pub fn source(&self) -> &StagedSource {
        &self.source
    }
    pub fn archive(&self) -> &[u8] {
        &self.archive
    }
    pub fn archive_sha256(&self) -> Sha256 {
        self.sha256
    }
    pub fn index_path(&self) -> &str {
        &self.index_path
    }
    pub fn index_line(&self) -> &[u8] {
        &self.index_line
    }
}
pub(super) fn index_location(name: &str) -> String {
    match name.len() {
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", &name[..1]),
        _ => format!("{}/{}/{name}", &name[..2], &name[2..4]),
    }
}
fn registry_index<'a>(
    origin: &CargoOrigin,
    registries: &'a HostedRegistries,
) -> io::Result<&'a str> {
    match origin {
        CargoOrigin::CratesIo => Ok(PUBLIC_INDEX),
        CargoOrigin::DevOption(id) => registries
            .named(id.as_str())
            .map(HostedRegistry::index)
            .ok_or_else(invalid),
    }
}

// Both generation and decoding derive the exact index bytes from the admitted
// direct dependencies, including destination-relative dependency namespaces.
fn index_metadata(
    source: &StagedSource,
    sha256: Sha256,
    dependencies: &[DirectDependency],
    registries: &HostedRegistries,
) -> io::Result<Vec<u8>> {
    let coordinates = source.coordinates();
    let mut entries = Vec::with_capacity(dependencies.len());
    for DirectDependency {
        name,
        requirement,
        default_features,
        origin,
    } in dependencies
    {
        let index = registry_index(origin, registries)?;
        entries.push(json!({
            "name": name,
            "req": requirement,
            "features": [],
            "optional": false,
            "default_features": default_features,
            "target": null,
            "kind": "normal",
            "registry": if index == coordinates.registry().index() { None } else { Some(index) }
        }));
    }
    let checksum: String = sha256.bytes().iter().map(|b| format!("{b:02x}")).collect();
    let mut line = serde_json::to_vec(&json!({
        "name": coordinates.name(),
        "vers": coordinates.version(),
        "deps": entries,
        "cksum": checksum,
        "features": {},
        "yanked": false
    }))
    .map_err(|_| invalid())?;
    line.push(b'\n');
    if line.len() > MAX_INDEX_LINE_BYTES {
        return Err(invalid());
    }
    Ok(line)
}

fn normalized_manifest(
    name: &str,
    version: &str,
    edition: &str,
    dependencies: &[DirectDependency],
    registries: &HostedRegistries,
) -> io::Result<String> {
    // All interpolated source values are already constrained by source admission;
    // quote through JSON for canonical ASCII TOML string escaping.
    let quote = |value: &str| serde_json::to_string(value).map_err(|_| invalid());
    let mut normalized = format!(
        "# Generated by DevOption; original bytes are in Cargo.toml.orig.\n[package]\nname={}\nversion={}\nedition={}\nbuild=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n[lib]\nname={}\npath=\"src/lib.rs\"\ncrate-type=[\"rlib\"]\n",
        quote(name)?,
        quote(version)?,
        quote(edition)?,
        quote(&name.replace('-', "_"))?
    );
    for DirectDependency {
        name,
        requirement,
        default_features,
        origin,
    } in dependencies
    {
        let index = registry_index(origin, registries)?;
        normalized += &format!(
            "[dependencies.{}]\nversion={}\ndefault-features={}\nregistry-index={}\n",
            quote(name)?,
            quote(requirement)?,
            default_features,
            quote(index)?
        );
    }
    Ok(normalized)
}

fn recover_registries(
    original: &[u8],
    lock: &[u8],
) -> Result<HostedRegistries, super::staging::Error> {
    use super::staging::Error;
    if original.len() > cargo_inputs::MAX_MANIFEST_BYTES
        || lock.len() > cargo_inputs::MAX_LOCKFILE_BYTES
    {
        return Err(Error::Integrity);
    }
    let manifest: toml::Table = std::str::from_utf8(original)
        .map_err(|_| Error::Integrity)?
        .parse()
        .map_err(|_| Error::Integrity)?;
    let lock: toml::Table = std::str::from_utf8(lock)
        .map_err(|_| Error::Integrity)?
        .parse()
        .map_err(|_| Error::Integrity)?;
    let rows = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or(Error::Integrity)?;
    let mut roots = rows.iter().filter(|row| row.get("source").is_none());
    let root = roots.next().ok_or(Error::Integrity)?;
    if roots.next().is_some() {
        return Err(Error::Integrity);
    }
    let edges = match root.get("dependencies") {
        Some(value) => value.as_array().ok_or(Error::Integrity)?.as_slice(),
        None => &[],
    };
    let public_source = format!("registry+{PUBLIC_INDEX}");
    let mut by_alias: BTreeMap<String, HostedRegistry> = BTreeMap::new();
    if let Some(dependencies) = manifest.get("dependencies") {
        let entries = dependencies.as_table().ok_or(Error::Integrity)?;
        for (name, value) in entries {
            let Some(fields) = value.as_table() else {
                continue;
            };
            let Some(alias) = fields.get("registry") else {
                continue;
            };
            let alias = alias.as_str().ok_or(Error::Integrity)?;
            let pin = fields
                .get("version")
                .and_then(toml::Value::as_str)
                .and_then(|v| v.strip_prefix('='))
                .ok_or(Error::Integrity)?;
            let pin_release = release_version(pin);
            let mut matches = rows.iter().filter(|row| {
                let Some(source) = row.get("source").and_then(toml::Value::as_str) else {
                    return false;
                };
                let Some(version) = row.get("version").and_then(toml::Value::as_str) else {
                    return false;
                };
                source != public_source
                    && row.get("name").and_then(toml::Value::as_str) == Some(name.as_str())
                    && release_version(version) == pin_release
                    && edges.iter().any(|edge| {
                        edge.as_str().is_some_and(|edge| {
                            edge == name
                                || edge == format!("{name} {version}")
                                || edge == format!("{name} {version} ({source})")
                        })
                    })
            });
            let row = matches.next().ok_or(Error::Integrity)?;
            if matches.next().is_some() {
                return Err(Error::Integrity);
            }
            let source = row
                .get("source")
                .and_then(toml::Value::as_str)
                .ok_or(Error::Integrity)?;
            let binding = binding_from_lock_source(alias, source)?;
            if let Some(existing) = by_alias.get(alias) {
                if existing != &binding {
                    return Err(Error::Integrity);
                }
            } else {
                by_alias.insert(alias.to_owned(), binding);
            }
        }
    }
    let mut synthetic = 0usize;
    for row in rows {
        let Some(source) = row.get("source").and_then(toml::Value::as_str) else {
            continue;
        };
        if source == public_source || by_alias.values().any(|b| b.lock_source() == source) {
            continue;
        }
        let alias = loop {
            let candidate = format!("transitive-{synthetic}");
            synthetic += 1;
            if !by_alias.contains_key(&candidate) {
                break candidate;
            }
        };
        let binding = binding_from_lock_source(&alias, source)?;
        by_alias.insert(alias, binding);
    }
    HostedRegistries::new(by_alias.into_values().collect()).map_err(|_| Error::Integrity)
}

fn binding_from_lock_source(
    alias: &str,
    source: &str,
) -> Result<HostedRegistry, super::staging::Error> {
    use super::staging::Error;
    let index = if source.starts_with("sparse+") {
        source
    } else {
        source
            .strip_prefix("registry+")
            .filter(|index| !index.starts_with("sparse+"))
            .ok_or(Error::Integrity)?
    };
    let id = RegistryId::new(alias).map_err(|_| Error::Integrity)?;
    let binding = HostedRegistry::new(id, index).map_err(|_| Error::Integrity)?;
    if binding.lock_source() != source {
        return Err(Error::Integrity);
    }
    Ok(binding)
}

fn release_version(version: &str) -> &str {
    version
        .split_once('+')
        .map_or(version, |(release, _)| release)
}

fn invalid() -> io::Error {
    io::Error::other("unsupported or oversized Cargo source package")
}

#[cfg(test)]
pub(super) mod tests;
