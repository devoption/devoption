//! Immutable Cargo dependency descriptors and a DevOption registry contract.
//!
//! A bounded supplied-byte Cargo adapter checks a deliberately small manifest and
//! lockfile subset and hashes those inputs. It performs no filesystem/network I/O.
//! A single-crate seed provider verifies and owns supplied archive bytes. There
//! is no acquisition service or runtime caller. Metadata grants no approval.
//! Provider implementations are responsible for their effects. A separate Linux
//! integration test compiles only the checked-in trusted offline fixture.

use std::{cmp::Ordering, fmt};

pub mod cargo_inputs;
pub mod seeded;

pub const MAX_REGISTRY_ID_BYTES: usize = 64;
pub const MAX_PACKAGE_NAME_BYTES: usize = 64;
pub const MAX_VERSION_BYTES: usize = 128;
pub const MAX_PACKAGES: usize = 1024;

/// Logical DevOption registry identity, never a URL, path or access grant.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegistryId(String);

impl RegistryId {
    pub fn new(value: &str) -> Result<Self, Error> {
        if !name_valid(value, MAX_REGISTRY_ID_BYTES) {
            return Err(Error::InvalidRegistry);
        }
        Ok(Self(value.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// SHA-256 value. Parsing accepts a claim; digest computes supplied bytes.
/// Neither operation establishes source, registry or commit provenance.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Sha256([u8; 32]);

impl Sha256 {
    /// Hash exact supplied bytes; this establishes no source or registry provenance.
    pub fn digest(bytes: &[u8]) -> Self {
        use sha2::Digest;
        Self(sha2::Sha256::digest(bytes).into())
    }

    pub fn from_hex(value: &str) -> Result<Self, Error> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::InvalidChecksum);
        }
        let mut bytes = [0; 32];
        for (i, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            let digit = |b: u8| {
                if b.is_ascii_digit() {
                    b - b'0'
                } else {
                    b - b'a' + 10
                }
            };
            bytes[i] = digit(pair[0]) * 16 + digit(pair[1]);
        }
        Ok(Self(bytes))
    }

    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Original package namespace, distinct from the DevOption provider serving it.
/// CratesIo always means delivery through DevOption's proxy, never direct fetch.
/// Hosted registry visibility and access policy are not represented or granted.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CargoOrigin {
    CratesIo,
    DevOption(RegistryId),
}

/// Exact locked coordinates and the expected .crate archive checksum.
/// Values are validated and owned by CargoDependencyRequest; they are not resolved
/// from version requirements by this crate. No Debug impl exposes private names.
#[derive(Clone, PartialEq, Eq)]
pub struct LockedPackage {
    pub origin: CargoOrigin,
    pub name: String,
    pub version: String,
    pub archive_sha256: Sha256,
}

/// Immutable claimed registry-package set with both input hashes, in canonical order.
/// The supplied-byte adapter checks its supported lock graph, not completeness
/// against verified source and actual dependency manifests. An empty set is permitted.
#[derive(Clone, PartialEq, Eq)]
pub struct CargoDependencyRequest {
    registry: RegistryId,
    manifest_sha256: Sha256,
    lockfile_sha256: Sha256,
    packages: Vec<LockedPackage>,
}

impl CargoDependencyRequest {
    pub fn new(
        registry: RegistryId,
        manifest_sha256: Sha256,
        lockfile_sha256: Sha256,
        mut packages: Vec<LockedPackage>,
    ) -> Result<Self, Error> {
        if packages.len() > MAX_PACKAGES {
            return Err(Error::TooManyPackages);
        }
        for package in &packages {
            if !package_name_valid(&package.name) {
                return Err(Error::InvalidPackageName);
            }
            if !exact_version(&package.version) {
                return Err(Error::InvalidVersion);
            }
        }
        packages.sort_by(compare_coordinates);
        if packages
            .windows(2)
            .any(|pair| compare_coordinates(&pair[0], &pair[1]).is_eq())
        {
            return Err(Error::DuplicatePackage);
        }
        Ok(Self {
            registry,
            manifest_sha256,
            lockfile_sha256,
            packages,
        })
    }

    pub fn registry(&self) -> &RegistryId {
        &self.registry
    }

    pub fn lockfile_sha256(&self) -> Sha256 {
        self.lockfile_sha256
    }

    pub fn manifest_sha256(&self) -> Sha256 {
        self.manifest_sha256
    }

    pub fn packages(&self) -> &[LockedPackage] {
        &self.packages
    }
}

/// Immutable Cargo dependency-bundle metadata, including its
/// provenance and complete claimed request. No path, URL, executable or credential.
/// The digest/size describe bundle bytes, not a serialized Rust struct. A general
/// physical bundle format remains unimplemented. The single-crate seed provider
/// uses the exact raw .crate archive as its narrowly defined bundle.
#[derive(Clone, PartialEq, Eq)]
pub struct CargoMaterialization {
    request: CargoDependencyRequest,
    bundle_sha256: Sha256,
    bundle_bytes: u64,
}

impl CargoMaterialization {
    pub fn new(request: CargoDependencyRequest, bundle_sha256: Sha256, bundle_bytes: u64) -> Self {
        Self {
            request,
            bundle_sha256,
            bundle_bytes,
        }
    }

    pub fn request(&self) -> &CargoDependencyRequest {
        &self.request
    }

    pub fn bundle_sha256(&self) -> Sha256 {
        self.bundle_sha256
    }

    /// Claimed size, not an authorized quota or permission to allocate storage.
    pub fn bundle_bytes(&self) -> u64 {
        self.bundle_bytes
    }
}

/// Provider selected by DevOption, not by repository-supplied URLs or commands.
/// A future provider requires independent authorization for I/O and must verify
/// immutable bytes and provenance. No production implementation exists here.
pub trait CargoRegistryProvider {
    fn materialize(&self, request: &CargoDependencyRequest) -> Result<CargoMaterialization, Error>;
}

/// Invoke one provider and reject a response for different locked inputs or a
/// different registry. Errors propagate unchanged; there is no fallback fetch.
/// Matching metadata is not evidence that the provider verified physical bytes.
pub fn prepare_dependencies(
    provider: &impl CargoRegistryProvider,
    request: &CargoDependencyRequest,
) -> Result<CargoMaterialization, Error> {
    let materialization = provider.materialize(request)?;
    if materialization.request() != request {
        return Err(Error::MismatchedRequest);
    }
    Ok(materialization)
}

/// Fixed diagnostics omit package names, paths, URLs and provider details.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidRegistry,
    InvalidChecksum,
    InvalidPackageName,
    InvalidVersion,
    TooManyPackages,
    DuplicatePackage,
    MismatchedRequest,
    Unavailable,
    Denied,
    Integrity,
    UnsupportedSeed,
    SeedTooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidRegistry => "registry identity is malformed or oversized",
            Self::InvalidChecksum => "checksum requires 64 lowercase hexadecimal characters",
            Self::InvalidPackageName => "package name is malformed or oversized",
            Self::InvalidVersion => "package version must be a bounded exact Cargo version",
            Self::TooManyPackages => "dependency metadata capacity exceeded",
            Self::DuplicatePackage => "duplicate locked package coordinates",
            Self::MismatchedRequest => "registry response does not match the dependency request",
            Self::Unavailable => "registry dependency materialization is unavailable",
            Self::Denied => "registry dependency request is denied",
            Self::Integrity => "registry dependency integrity check failed",
            Self::UnsupportedSeed => "seed provider requires exactly one crates.io package",
            Self::SeedTooLarge => "seed archive exceeds the provider input bound",
        })
    }
}

impl std::error::Error for Error {}

fn version_identity(package: &LockedPackage) -> &str {
    // Cargo registry versions are unique without SemVer build metadata.
    package
        .version
        .split('+')
        .next()
        .unwrap_or(&package.version)
}

fn compare_coordinates(a: &LockedPackage, b: &LockedPackage) -> Ordering {
    a.origin
        .cmp(&b.origin)
        .then_with(|| {
            a.name
                .bytes()
                .map(|b| b.to_ascii_lowercase())
                .cmp(b.name.bytes().map(|b| b.to_ascii_lowercase()))
        })
        .then_with(|| version_identity(a).cmp(version_identity(b)))
}

fn package_name_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PACKAGE_NAME_BYTES
        && value.bytes().enumerate().all(|(i, b)| {
            b.is_ascii_alphabetic() || (i > 0 && (b.is_ascii_digit() || b == b'-' || b == b'_'))
        })
}

fn name_valid(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value.bytes().enumerate().all(|(i, b)| {
            b.is_ascii_lowercase() || (i > 0 && (b.is_ascii_digit() || b == b'-' || b == b'_'))
        })
}

// Exact SemVer spelling with bounded u64 core components, as used by Cargo.
// Ranges, floating requirements and caller-supplied arguments are not accepted.
fn exact_version(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_VERSION_BYTES {
        return false;
    }
    let (release, build) = value
        .split_once('+')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    if build.is_some_and(|b| !identifiers(b, false)) {
        return false;
    }
    let (core, pre) = release
        .split_once('-')
        .map_or((release, None), |(a, b)| (a, Some(b)));
    if pre.is_some_and(|p| !identifiers(p, true)) {
        return false;
    }
    let parts: Vec<_> = core.split('.').take(4).collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && (part.len() == 1 || !part.starts_with('0'))
                && part.bytes().all(|b| b.is_ascii_digit())
                && part.parse::<u64>().is_ok()
        })
}

fn identifiers(value: &str, prerelease: bool) -> bool {
    value.split('.').all(|part| {
        !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !(prerelease
                && part.len() > 1
                && part.starts_with('0')
                && part.bytes().all(|b| b.is_ascii_digit()))
    })
}

#[cfg(test)]
mod tests;
