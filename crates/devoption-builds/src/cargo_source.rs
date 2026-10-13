//! Bounded root Cargo membership in supplied Git SHA-1 objects, without I/O.
//! Selection/authenticity and execution context remain external trust requirements.

use devoption_registry::{CargoDependencyRequest, RegistryId, Sha256, cargo_inputs};
use sha1collisiondetection::Sha1CD;
use std::{collections::BTreeSet, fmt};

pub const MAX_COMMIT_BYTES: usize = 64 * 1024;
pub const MAX_TREE_BYTES: usize = 64 * 1024;
pub const MAX_ROOT_ENTRIES: usize = 1024;
pub const MAX_NAME_BYTES: usize = 128;

/// Raw, uncompressed object payloads; no headers, packfiles or filesystem paths.
pub struct RootProof<'a> {
    pub commit: &'a [u8],
    pub tree: &'a [u8],
    pub manifest: &'a [u8],
    pub lockfile: &'a [u8],
}

/// Object membership relative to a selected ID, NOT an authenticated repository,
/// full source snapshot, approved plan or safe materialization. No public constructor.
#[derive(Clone, PartialEq, Eq)]
pub struct RootCargoSource {
    commit: String,
    commit_sha256: Sha256,
    tree_sha256: Sha256,
    request: CargoDependencyRequest,
}

impl RootCargoSource {
    pub fn commit(&self) -> &str {
        &self.commit
    }

    pub fn commit_sha256(&self) -> Sha256 {
        self.commit_sha256
    }

    pub fn tree_sha256(&self) -> Sha256 {
        self.tree_sha256
    }

    pub fn request(&self) -> &CargoDependencyRequest {
        &self.request
    }
}

/// Verify typed Git object hashes and root paths before invoking the Cargo adapter.
/// The selected commit must come from a future independent selection/authorization
/// boundary. Allowing the proof supplier to choose it proves only self-consistency.
pub fn verify_root(
    registry: RegistryId,
    selected_commit: &str,
    proof: RootProof<'_>,
) -> Result<RootCargoSource, Error> {
    for (bytes, limit) in [
        (proof.commit, MAX_COMMIT_BYTES),
        (proof.tree, MAX_TREE_BYTES),
        (proof.manifest, cargo_inputs::MAX_MANIFEST_BYTES),
        (proof.lockfile, cargo_inputs::MAX_LOCKFILE_BYTES),
    ] {
        if bytes.is_empty() {
            return Err(Error::MissingInput);
        }
        if bytes.len() > limit {
            return Err(Error::Capacity);
        }
    }
    let selected = oid(selected_commit.as_bytes()).ok_or(Error::InvalidCommit)?;
    require_object("commit", proof.commit, selected)?;
    let tree = commit_tree(proof.commit)?;
    require_object("tree", proof.tree, tree)?;
    let (manifest, lockfile) = root_entries(proof.tree)?;
    require_object("blob", proof.manifest, manifest)?;
    require_object("blob", proof.lockfile, lockfile)?;
    let request = cargo_inputs::validate(
        registry,
        proof.manifest,
        proof.lockfile,
        cargo_inputs::InputHashes {
            manifest: Sha256::digest(proof.manifest),
            lockfile: Sha256::digest(proof.lockfile),
        },
    )
    .map_err(Error::CargoInputs)?;
    Ok(RootCargoSource {
        commit: selected_commit.into(),
        commit_sha256: Sha256::digest(proof.commit),
        tree_sha256: Sha256::digest(proof.tree),
        request,
    })
}

fn require_object(kind: &str, bytes: &[u8], expected: [u8; 20]) -> Result<(), Error> {
    let header = format!("{kind} {}\0", bytes.len());
    if checked_hash(&[header.as_bytes(), bytes])? != expected {
        return Err(Error::ObjectMismatch);
    }
    Ok(())
}

fn checked_hash(parts: &[&[u8]]) -> Result<[u8; 20], Error> {
    let mut hash = Sha1CD::default();
    for part in parts {
        hash.update(part);
    }
    // Never accept a substituted safe hash after a detected collision.
    hash.finalize_cd()
        .map(Into::into)
        .map_err(|_| Error::Collision)
}

fn oid(hex: &[u8]) -> Option<[u8; 20]> {
    if hex.len() != 40 || hex.iter().all(|&b| b == b'0') {
        return None;
    }
    let digit = |b| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    let mut result = [0; 20];
    for (out, pair) in result.iter_mut().zip(hex.as_chunks::<2>().0) {
        *out = digit(pair[0])? * 16 + digit(pair[1])?;
    }
    Some(result)
}

fn commit_tree(commit: &[u8]) -> Result<[u8; 20], Error> {
    if commit.contains(&0) {
        return Err(Error::UnsupportedCommit);
    }
    let end = commit
        .windows(2)
        .position(|p| p == b"\n\n")
        .ok_or(Error::UnsupportedCommit)?;
    let mut lines = commit[..end].split(|&b| b == b'\n');
    let tree = lines
        .next()
        .and_then(|l| l.strip_prefix(b"tree "))
        .and_then(oid)
        .ok_or(Error::UnsupportedCommit)?;
    let mut line = lines.next().ok_or(Error::UnsupportedCommit)?;
    let mut parents = BTreeSet::new();
    while let Some(parent) = line.strip_prefix(b"parent ") {
        if parents.len() == 2 || !parents.insert(oid(parent).ok_or(Error::UnsupportedCommit)?) {
            return Err(Error::UnsupportedCommit);
        }
        line = lines.next().ok_or(Error::UnsupportedCommit)?;
    }
    for prefix in [b"author ".as_slice(), b"committer "] {
        let value = line.strip_prefix(prefix).ok_or(Error::UnsupportedCommit)?;
        // Metadata is opaque, single-line ASCII; it is not an identity attestation.
        if value.is_empty() || !value.iter().all(|b| (b' '..=b'~').contains(b)) {
            return Err(Error::UnsupportedCommit);
        }
        if prefix == b"author " {
            line = lines.next().ok_or(Error::UnsupportedCommit)?;
        }
    }
    if lines.next().is_some() {
        return Err(Error::UnsupportedCommit);
    }
    Ok(tree)
}

fn root_entries(mut bytes: &[u8]) -> Result<([u8; 20], [u8; 20]), Error> {
    let mut names = BTreeSet::new();
    let mut previous = Vec::new();
    let (mut manifest, mut lockfile) = (None, None);
    while !bytes.is_empty() {
        if names.len() == MAX_ROOT_ENTRIES {
            return Err(Error::Capacity);
        }
        let end = bytes
            .iter()
            .position(|&b| b == 0)
            .ok_or(Error::InvalidTree)?;
        let entry = &bytes[..end];
        let space = entry
            .iter()
            .position(|&b| b == b' ')
            .ok_or(Error::InvalidTree)?;
        let (mode, name) = (&entry[..space], &entry[space + 1..]);
        if !matches!(mode, b"100644" | b"100755" | b"40000")
            || name.is_empty()
            || name.len() > MAX_NAME_BYTES
            || name.ends_with(b".")
            || !name
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(b))
        {
            return Err(Error::InvalidTree);
        }
        let folded: Vec<_> = name.iter().map(u8::to_ascii_lowercase).collect();
        if !names.insert(folded.clone()) {
            return Err(Error::InvalidTree);
        }
        if matches!(
            folded.as_slice(),
            b".git"
                | b".cargo"
                | b".gitmodules"
                | b".gitattributes"
                | b"rust-toolchain"
                | b"rust-toolchain.toml"
        ) {
            return Err(Error::UnsupportedContext);
        }
        let mut order = name.to_vec();
        if mode == b"40000" {
            order.push(b'/');
        }
        if previous >= order {
            return Err(Error::InvalidTree);
        }
        previous = order;
        let id: [u8; 20] = bytes
            .get(end + 1..end + 21)
            .ok_or(Error::InvalidTree)?
            .try_into()
            .map_err(|_| Error::InvalidTree)?;
        if id == [0; 20] {
            return Err(Error::InvalidTree);
        }
        bytes = &bytes[end + 21..];
        match name {
            b"Cargo.toml" | b"Cargo.lock" => {
                if mode != b"100644" {
                    return Err(Error::InvalidTree);
                }
                if name == b"Cargo.toml" {
                    manifest = Some(id);
                } else {
                    lockfile = Some(id);
                }
            }
            _ => {}
        }
    }
    Ok((
        manifest.ok_or(Error::InvalidTree)?,
        lockfile.ok_or(Error::InvalidTree)?,
    ))
}

/// Fixed diagnostics omit all supplied object contents and private identities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    MissingInput,
    Capacity,
    InvalidCommit,
    UnsupportedCommit,
    InvalidTree,
    UnsupportedContext,
    ObjectMismatch,
    Collision,
    CargoInputs(cargo_inputs::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingInput => "source proof requires all four object payloads",
            Self::Capacity => "source proof exceeds input or entry bounds",
            Self::InvalidCommit => "selected revision requires a full nonzero lowercase SHA-1 ID",
            Self::UnsupportedCommit => "source proof requires the supported commit header shape",
            Self::InvalidTree => "source proof has missing, malformed or ambiguous root entries",
            Self::UnsupportedContext => "source proof contains unsupported root configuration",
            Self::ObjectMismatch => "typed Git object does not match its required identity",
            Self::Collision => "source proof contains a detected SHA-1 collision",
            Self::CargoInputs(_) => {
                "source proof contains unsupported or inconsistent Cargo inputs"
            }
        })
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests;
