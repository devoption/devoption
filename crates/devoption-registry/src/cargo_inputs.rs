//! Validate supplied Cargo bytes, never a checkout or proof of committedness.
//! Supported subset and remaining source/registry proofs: docs/cargo-inputs.md.

use crate::{CargoDependencyRequest, CargoOrigin, LockedPackage, RegistryId, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};
use toml::{Table, Value};

pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_LOCKFILE_BYTES: usize = 512 * 1024;
pub const MAX_DEPENDENCY_EDGES: usize = 4096;
const CRATES_IO_SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";

/// Caller-declared expectations. A future verified snapshot must supply these;
/// matching caller-selected hashes alone cannot prove that a commit contains them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct InputHashes {
    pub manifest: Sha256,
    pub lockfile: Sha256,
}

/// Supported source shape, not a target triple, ABI proof or execution grant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceTarget {
    Binary,
    Cdylib { name: String },
}

impl SourceTarget {
    pub fn path(&self) -> &'static str {
        match self {
            Self::Binary => "src/main.rs",
            Self::Cdylib { .. } => "src/lib.rs",
        }
    }
}

/// Internally consistent supplied inputs, without a provider or source proof.
/// No Debug implementation prints package metadata implicitly.
pub struct InspectedInputs {
    package: String,
    version: String,
    hashes: InputHashes,
    packages: Vec<LockedPackage>,
    target: SourceTarget,
}

impl InspectedInputs {
    pub fn target(&self) -> &SourceTarget {
        &self.target
    }
    pub fn package(&self) -> &str {
        &self.package
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn hashes(&self) -> InputHashes {
        self.hashes
    }
    pub fn dependency_count(&self) -> usize {
        self.packages.len()
    }

    fn into_request(self, registry: RegistryId) -> CargoDependencyRequest {
        CargoDependencyRequest {
            registry,
            manifest_sha256: self.hashes.manifest,
            lockfile_sha256: self.hashes.lockfile,
            packages: self.packages,
        }
    }
}

/// Inspect current supplied bytes without selecting a registry or claiming that
/// expected hashes came from an independently verified source snapshot.
pub fn inspect(manifest: &[u8], lockfile: &[u8]) -> Result<InspectedInputs, Error> {
    inspect_expected(manifest, lockfile, None)
}

/// Validate one standalone package, exact direct pins and a closed v4 lock graph.
/// This checks internal consistency only, not Cargo resolution, package archives,
/// source files, Cargo configuration, target discovery or execution permissions.
pub fn validate(
    registry: RegistryId,
    manifest: &[u8],
    lockfile: &[u8],
    expected: InputHashes,
) -> Result<CargoDependencyRequest, Error> {
    Ok(inspect_expected(manifest, lockfile, Some(expected))?.into_request(registry))
}

fn inspect_expected(
    manifest: &[u8],
    lockfile: &[u8],
    expected: Option<InputHashes>,
) -> Result<InspectedInputs, Error> {
    if manifest.is_empty() || lockfile.is_empty() {
        return Err(Error::MissingInput);
    }
    if manifest.len() > MAX_MANIFEST_BYTES || lockfile.len() > MAX_LOCKFILE_BYTES {
        return Err(Error::Capacity);
    }
    let actual = InputHashes {
        manifest: Sha256::digest(manifest),
        lockfile: Sha256::digest(lockfile),
    };
    if expected.is_some_and(|expected| actual != expected) {
        return Err(Error::HashMismatch);
    }
    let manifest = parse(manifest)?;
    keys(&manifest, &["package", "dependencies", "lib"])?;
    let package = table(manifest.get("package"))?;
    keys(package, &["name", "version", "edition", "build"])?;
    let root_name = string(package.get("name"))?;
    let root_version = string(package.get("version"))?;
    if !crate::package_name_valid(root_name) || !crate::exact_version(root_version) {
        return Err(Error::InvalidPackage);
    }
    if !matches!(string(package.get("edition"))?, "2021" | "2024")
        || package.get("build").and_then(Value::as_bool) != Some(false)
    {
        return Err(Error::Unsupported);
    }
    let target = match manifest.get("lib") {
        None => SourceTarget::Binary,
        Some(value) => {
            let library = table(Some(value))?;
            keys(library, &["name", "crate-type"])?;
            let name = string(library.get("name"))?;
            let types = library.get("crate-type").and_then(Value::as_array);
            // One conventional library name/path only; no renamed or extra targets.
            if name != root_name.replace('-', "_")
                || !name.bytes().enumerate().all(|(i, b)| {
                    b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit())
                })
                || !matches!(types, Some(values) if values.len() == 1 && values[0].as_str() == Some("cdylib"))
            {
                return Err(Error::Unsupported);
            }
            SourceTarget::Cdylib { name: name.into() }
        }
    };
    let mut direct = BTreeMap::new();
    if let Some(dependencies) = manifest.get("dependencies") {
        for (name, value) in table(Some(dependencies))? {
            let requirement = match value {
                Value::String(pin) => pin.as_str(),
                Value::Table(dependency) => {
                    keys(dependency, &["version", "default-features"])?;
                    if dependency.get("default-features").and_then(Value::as_bool) != Some(false) {
                        return Err(Error::Unsupported);
                    }
                    string(dependency.get("version"))?
                }
                _ => return Err(Error::Unsupported),
            };
            let pin = requirement.strip_prefix('=').ok_or(Error::Unsupported)?;
            if !crate::package_name_valid(name) || !crate::exact_version(pin) {
                return Err(Error::Unsupported);
            }
            direct.insert(name.as_str(), pin);
        }
    }
    if direct.len() > crate::MAX_PACKAGES {
        return Err(Error::Capacity);
    }

    let lockfile = parse(lockfile)?;
    keys(&lockfile, &["version", "package"])?;
    if lockfile.get("version").and_then(Value::as_integer) != Some(4) {
        return Err(Error::Unsupported);
    }
    let rows = lockfile
        .get("package")
        .and_then(Value::as_array)
        .ok_or(Error::InvalidInput)?;
    if rows.is_empty() || rows.len() > crate::MAX_PACKAGES + 1 {
        return Err(Error::Capacity);
    }
    let mut nodes = Vec::new();
    let mut packages = Vec::new();
    let mut root = None;
    let mut edge_count = 0usize;
    for row in rows {
        let row = table(Some(row))?;
        keys(
            row,
            &["name", "version", "source", "checksum", "dependencies"],
        )?;
        let name = string(row.get("name"))?;
        let version = string(row.get("version"))?;
        if !crate::package_name_valid(name) || !crate::exact_version(version) {
            return Err(Error::InvalidPackage);
        }
        if let Some(source) = row.get("source") {
            if string(Some(source))? != CRATES_IO_SOURCE {
                return Err(Error::Unsupported);
            }
            let checksum = row
                .get("checksum")
                .and_then(Value::as_str)
                .ok_or(Error::InvalidChecksum)?;
            packages.push(LockedPackage {
                origin: CargoOrigin::CratesIo,
                name: name.into(),
                version: version.into(),
                archive_sha256: Sha256::from_hex(checksum).map_err(|_| Error::InvalidChecksum)?,
            });
        } else {
            if root.replace(nodes.len()).is_some() || row.contains_key("checksum") {
                return Err(Error::Unsupported);
            }
            if name != root_name || version != root_version {
                return Err(Error::RootMismatch);
            }
        }
        let mut dependencies = Vec::new();
        if let Some(value) = row.get("dependencies") {
            let entries = value.as_array().ok_or(Error::InvalidInput)?;
            edge_count = edge_count
                .checked_add(entries.len())
                .ok_or(Error::Capacity)?;
            if edge_count > MAX_DEPENDENCY_EDGES {
                return Err(Error::Capacity);
            }
            for entry in entries {
                let reference = string(Some(entry))?;
                if reference.len() > crate::MAX_PACKAGE_NAME_BYTES + 1 + crate::MAX_VERSION_BYTES {
                    return Err(Error::Unsupported);
                }
                dependencies.push(reference.to_owned());
            }
        }
        nodes.push(Node {
            name: name.into(),
            version: version.into(),
            dependencies,
        });
    }
    let root = root.ok_or(Error::RootMismatch)?;
    let packages = crate::canonical_packages(packages).map_err(|_| Error::InvalidPackage)?;
    let edges = resolve_edges(&nodes)?;
    if edges[root].len() != direct.len()
        || edges[root].iter().any(|&i| {
            direct.get(nodes[i].name.as_str()).is_none_or(|pin| {
                // Cargo ignores build metadata in version requirements. Full locked
                // versions and checksums remain in the dependency request identity.
                release_version(pin) != release_version(&nodes[i].version)
            })
        })
    {
        return Err(Error::DependencyMismatch);
    }
    check_graph(root, &edges)?;
    Ok(InspectedInputs {
        package: root_name.into(),
        version: root_version.into(),
        hashes: actual,
        packages,
        target,
    })
}

fn release_version(version: &str) -> &str {
    version
        .split_once('+')
        .map_or(version, |(release, _)| release)
}

struct Node {
    name: String,
    version: String,
    dependencies: Vec<String>,
}

fn resolve_edges(nodes: &[Node]) -> Result<Vec<Vec<usize>>, Error> {
    let mut names: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut exact = BTreeMap::new();
    for (i, node) in nodes.iter().enumerate() {
        names.entry(&node.name).or_default().push(i);
        if exact
            .insert((node.name.as_str(), node.version.as_str()), i)
            .is_some()
        {
            return Err(Error::AmbiguousDependency);
        }
    }
    let mut edges = Vec::new();
    for node in nodes {
        let mut seen = BTreeSet::new();
        let mut targets = Vec::new();
        for reference in &node.dependencies {
            let parts: Vec<_> = reference.split(' ').take(3).collect();
            let target = match parts.as_slice() {
                [name] => match names.get(name).map(Vec::as_slice) {
                    Some([only]) => *only,
                    Some(_) => return Err(Error::AmbiguousDependency),
                    None => return Err(Error::MissingDependency),
                },
                [name, version] if !name.is_empty() && !version.is_empty() => *exact
                    .get(&(*name, *version))
                    .ok_or(Error::MissingDependency)?,
                _ => return Err(Error::Unsupported),
            };
            if !seen.insert(target) {
                return Err(Error::InvalidGraph);
            }
            targets.push(target);
        }
        edges.push(targets);
    }
    Ok(edges)
}

// Iterative traversal keeps adversarially deep graphs off the call stack.
fn check_graph(root: usize, edges: &[Vec<usize>]) -> Result<(), Error> {
    let mut state = vec![0; edges.len()];
    let mut stack = vec![(root, 0)];
    state[root] = 1;
    while let Some((node, next)) = stack.last_mut() {
        if let Some(&child) = edges[*node].get(*next) {
            *next += 1;
            match state[child] {
                1 => return Err(Error::InvalidGraph),
                0 => {
                    state[child] = 1;
                    stack.push((child, 0));
                }
                _ => {}
            }
        } else {
            state[*node] = 2;
            stack.pop();
        }
    }
    if state.contains(&0) {
        return Err(Error::InvalidGraph);
    }
    Ok(())
}

fn parse(bytes: &[u8]) -> Result<Table, Error> {
    std::str::from_utf8(bytes)
        .map_err(|_| Error::Syntax)?
        .parse()
        .map_err(|_| Error::Syntax)
}

fn keys(table: &Table, allowed: &[&str]) -> Result<(), Error> {
    if table.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(Error::Unsupported);
    }
    Ok(())
}

fn table(value: Option<&Value>) -> Result<&Table, Error> {
    value.and_then(Value::as_table).ok_or(Error::InvalidInput)
}

fn string(value: Option<&Value>) -> Result<&str, Error> {
    value.and_then(Value::as_str).ok_or(Error::InvalidInput)
}

/// Fixed diagnostics never include untrusted TOML text, private package names or paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    MissingInput,
    Capacity,
    HashMismatch,
    Syntax,
    Unsupported,
    InvalidInput,
    InvalidPackage,
    InvalidChecksum,
    RootMismatch,
    MissingDependency,
    AmbiguousDependency,
    DependencyMismatch,
    InvalidGraph,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingInput => "manifest and lockfile bytes are required",
            Self::Capacity => "Cargo input or graph capacity exceeded",
            Self::HashMismatch => "Cargo input bytes do not match their expected hashes",
            Self::Syntax => "Cargo input is not valid bounded UTF-8 TOML",
            Self::Unsupported => "Cargo input uses unsupported manifest or lockfile semantics",
            Self::InvalidInput => "Cargo input has missing or incorrectly typed fields",
            Self::InvalidPackage => "Cargo package coordinates are invalid or duplicate",
            Self::InvalidChecksum => "registry lock entry requires a valid archive checksum",
            Self::RootMismatch => "standalone package does not match the lockfile root",
            Self::MissingDependency => "lockfile dependency reference is missing",
            Self::AmbiguousDependency => "lockfile dependency reference is ambiguous",
            Self::DependencyMismatch => "direct locked dependencies do not match manifest pins",
            Self::InvalidGraph => "lockfile dependency graph is cyclic, repeated or disconnected",
        })
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests;
