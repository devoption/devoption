//! Complete, bounded source bytes relative to a separately selected Git commit.
//! No repository reads, extraction, revision authorization or execution.

use crate::cargo_source::{self, RootCargoSource, RootProof};
use devoption_registry::{CargoDependencyRequest, RegistryId, Sha256, cargo_inputs};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

pub const MAX_OBJECTS: usize = 256;
pub const MAX_FILES: usize = 128;
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_FILE_BYTES: usize = 512 * 1024;
pub const MAX_PATH_BYTES: usize = 1024;
pub const MAX_DEPTH: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Tree,
    Blob,
}

/// Uncompressed Git payload without its object header. Identity is computed from
/// kind and bytes, never accepted as an unverified caller-supplied map key.
pub struct Object<'a> {
    pub kind: ObjectKind,
    pub bytes: &'a [u8],
}

/// An owned regular non-executable file; no mutating accessors or path authority.
#[derive(Clone, PartialEq, Eq)]
pub struct SourceFile {
    path: String,
    object_id: [u8; 20],
    sha256: Sha256,
    bytes: Vec<u8>,
}

impl SourceFile {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn object_id(&self) -> [u8; 20] {
        self.object_id
    }
    pub fn sha256(&self) -> Sha256 {
        self.sha256
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// The entire supported commit tree, with exact bytes and the sole conventional
/// binary target. Immutable ownership is not filesystem isolation or permission.
#[derive(Clone, PartialEq, Eq)]
pub struct SourceSnapshot {
    cargo: RootCargoSource,
    files: Vec<SourceFile>,
    package: String,
    version: String,
    sha256: Sha256,
}

impl SourceSnapshot {
    pub fn commit(&self) -> &str {
        self.cargo.commit()
    }
    pub fn commit_sha256(&self) -> Sha256 {
        self.cargo.commit_sha256()
    }
    pub fn tree_sha256(&self) -> Sha256 {
        self.cargo.tree_sha256()
    }
    pub fn request(&self) -> &CargoDependencyRequest {
        self.cargo.request()
    }
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }
    pub fn package(&self) -> &str {
        &self.package
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn binary(&self) -> &str {
        &self.package
    }
    pub fn target_path(&self) -> &'static str {
        "src/main.rs"
    }
    /// Versioned, length-framed identity of the commit and all source paths/bytes.
    /// Registry policy stays separately bound through request() and plan equality.
    pub fn sha256(&self) -> Sha256 {
        self.sha256
    }
}

/// Verify every reachable tree/blob and retain all file bytes, not working-tree
/// observations. Objects may be shared by multiple paths but supplied only once;
/// missing and unused objects fail. The caller must independently select/authenticate
/// the revision before using this result in any approval or materialization flow.
pub fn verify(
    registry: RegistryId,
    selected_commit: &str,
    commit: &[u8],
    objects: &[Object<'_>],
) -> Result<SourceSnapshot, Error> {
    if commit.is_empty()
        || commit.len() > cargo_source::MAX_COMMIT_BYTES
        || objects.len() > MAX_OBJECTS
    {
        return Err(Error::Capacity);
    }
    let selected =
        cargo_source::oid(selected_commit.as_bytes()).ok_or(cargo_source::Error::InvalidCommit)?;
    if cargo_source::object_id("commit", commit)? != selected {
        return Err(cargo_source::Error::ObjectMismatch.into());
    }
    let root_id = cargo_source::commit_tree(commit)?;
    let mut total = commit.len();
    // Check all supplied sizes before hashing or allocating the object index.
    for object in objects {
        let limit = match object.kind {
            ObjectKind::Tree => cargo_source::MAX_TREE_BYTES,
            ObjectKind::Blob => MAX_FILE_BYTES,
        };
        total = total
            .checked_add(object.bytes.len())
            .ok_or(Error::Capacity)?;
        if object.bytes.len() > limit || total > MAX_BYTES {
            return Err(Error::Capacity);
        }
    }
    let mut index = BTreeMap::new();
    for object in objects {
        let kind = match object.kind {
            ObjectKind::Tree => "tree",
            ObjectKind::Blob => "blob",
        };
        let id = cargo_source::object_id(kind, object.bytes)?;
        if index.insert(id, object).is_some() {
            return Err(Error::DuplicateObject);
        }
    }
    let root = lookup(&index, root_id, ObjectKind::Tree)?;
    let mut traversal = Traversal {
        index: &index,
        used: BTreeSet::new(),
        files: Vec::new(),
        bytes: 0,
        entries: 0,
    };
    traversal.tree(root_id, "", 0)?;
    if traversal.used.len() != index.len() {
        return Err(Error::ExtraObject);
    }
    let mut files = traversal.files;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let file = |path| {
        files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.bytes.as_slice())
            .ok_or(Error::UnsupportedTarget)
    };
    let manifest = file("Cargo.toml")?;
    let lockfile = file("Cargo.lock")?;
    let main = file("src/main.rs")?;
    if std::str::from_utf8(main).map_or(true, |text| text.trim().is_empty()) {
        return Err(Error::UnsupportedTarget);
    }
    let cargo = cargo_source::verify_root(
        registry,
        selected_commit,
        RootProof {
            commit,
            tree: root,
            manifest,
            lockfile,
        },
    )?;
    let inputs =
        cargo_inputs::inspect(manifest, lockfile).map_err(cargo_source::Error::CargoInputs)?;
    if !crate::target_name(inputs.package()) {
        return Err(Error::UnsupportedTarget);
    }
    let mut identity = b"devoption-source-snapshot-v1\0".to_vec();
    frame(&mut identity, selected_commit.as_bytes());
    frame(&mut identity, cargo.commit_sha256().bytes());
    frame(&mut identity, cargo.tree_sha256().bytes());
    for file in &files {
        frame(&mut identity, file.path.as_bytes());
        frame(&mut identity, &file.bytes);
    }
    Ok(SourceSnapshot {
        cargo,
        sha256: Sha256::digest(&identity),
        files,
        package: inputs.package().into(),
        version: inputs.version().into(),
    })
}

fn frame(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value);
}

type Index<'a> = BTreeMap<[u8; 20], &'a Object<'a>>;

fn lookup<'a>(index: &Index<'a>, id: [u8; 20], kind: ObjectKind) -> Result<&'a [u8], Error> {
    let object = index.get(&id).ok_or(Error::MissingObject)?;
    if object.kind != kind {
        return Err(cargo_source::Error::ObjectMismatch.into());
    }
    Ok(object.bytes)
}

struct Traversal<'a> {
    index: &'a Index<'a>,
    used: BTreeSet<[u8; 20]>,
    files: Vec<SourceFile>,
    bytes: usize,
    entries: usize,
}

impl Traversal<'_> {
    fn tree(&mut self, id: [u8; 20], prefix: &str, depth: usize) -> Result<(), Error> {
        if depth >= MAX_DEPTH {
            return Err(Error::Capacity);
        }
        let bytes = lookup(self.index, id, ObjectKind::Tree)?;
        self.used.insert(id);
        let entries = cargo_source::tree_entries(bytes)?;
        if entries.is_empty() {
            return Err(Error::EmptyTree);
        }
        for entry in entries {
            self.entries += 1;
            if self.entries > MAX_OBJECTS {
                return Err(Error::Capacity);
            }
            let path = format!("{prefix}{}", entry.name);
            if path.len() > MAX_PATH_BYTES {
                return Err(Error::Capacity);
            }
            // The supported manifest has no explicit target definitions. Exclude
            // Cargo's other automatic target locations; nested package manifests
            // and configuration are outside this first standalone binary shape.
            let folded = path.to_ascii_lowercase();
            if matches!(
                folded.as_str(),
                "build.rs" | "src/lib.rs" | "src/bin" | "tests" | "examples" | "benches"
            ) || (depth > 0
                && matches!(
                    entry.name.to_ascii_lowercase().as_str(),
                    "cargo.toml" | "cargo.lock"
                ))
            {
                return Err(Error::UnsupportedTarget);
            }
            match entry.mode {
                b"40000" => self.tree(entry.id, &format!("{path}/"), depth + 1)?,
                b"100644" => {
                    if self.files.len() == MAX_FILES {
                        return Err(Error::Capacity);
                    }
                    let bytes = lookup(self.index, entry.id, ObjectKind::Blob)?;
                    self.used.insert(entry.id);
                    self.bytes = self.bytes.checked_add(bytes.len()).ok_or(Error::Capacity)?;
                    if self.bytes > MAX_BYTES {
                        return Err(Error::Capacity);
                    }
                    self.files.push(SourceFile {
                        path,
                        object_id: entry.id,
                        sha256: Sha256::digest(bytes),
                        bytes: bytes.to_vec(),
                    });
                }
                _ => return Err(Error::UnsupportedMode),
            }
        }
        Ok(())
    }
}

/// Fixed diagnostics deliberately omit object contents and source paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Verification(cargo_source::Error),
    Capacity,
    MissingObject,
    DuplicateObject,
    ExtraObject,
    UnsupportedTarget,
    UnsupportedMode,
    EmptyTree,
}

impl From<cargo_source::Error> for Error {
    fn from(error: cargo_source::Error) -> Self {
        Self::Verification(error)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Verification(_) => {
                "source snapshot has inconsistent or unsupported Git/Cargo inputs"
            }
            Self::Capacity => "source snapshot exceeds object, file, byte, path or depth limits",
            Self::MissingObject => "source snapshot is missing a referenced object",
            Self::DuplicateObject => "source snapshot supplies a duplicate object",
            Self::ExtraObject => "source snapshot supplies an unreferenced object",
            Self::UnsupportedTarget => {
                "source snapshot requires one standalone conventional Rust binary"
            }
            Self::UnsupportedMode => "source snapshot supports only regular non-executable files",
            Self::EmptyTree => "source snapshot contains an empty tree",
        })
    }
}
impl std::error::Error for Error {}

#[cfg(test)]
pub(crate) mod tests;
