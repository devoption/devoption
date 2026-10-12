//! First-owner setup and password authentication against the durable owner store.
//!
//! The [`service`] wrapper isolates blocking work and shares an attempt limit for
//! existing-owner HTTP sign-in. Explicit local/SSH setup uses [`local_setup`];
//! other setup callers must independently authorize enrollment and enforce policy.
//! The service can issue sessions after verification; these synchronous primitives
//! only return identities.
//! No directory selection, permission grant, password reset, or provider integration
//! is performed here. Passwords and stored hashes must never enter responses or logs.

use std::{error::Error, fmt};

use crate::{
    identity::IdentityId,
    owner_store::{OwnerStore, StoreError},
    password::{self, HashError, SALT_BYTES},
};

pub mod local_setup;
pub mod service;

/// Setup failures contain no password, salt, or password hash.
#[derive(Debug)]
pub enum SetupError {
    Store(StoreError),
    Password(HashError),
    RandomnessUnavailable,
}

impl fmt::Display for SetupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => error.fmt(formatter),
            Self::Password(error) => error.fmt(formatter),
            Self::RandomnessUnavailable => {
                formatter.write_str("owner setup randomness unavailable")
            }
        }
    }
}

impl Error for SetupError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Password(error) => Some(error),
            Self::RandomnessUnavailable => None,
        }
    }
}

impl From<StoreError> for SetupError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<HashError> for SetupError {
    fn from(error: HashError) -> Self {
        Self::Password(error)
    }
}

/// Create the first owner after the caller has independently authorized setup.
///
/// Identity and salt come from OS cryptographic randomness, never from the caller.
/// Existing or broken storage cannot become a new account. The store's atomic
/// publication arbitrates concurrent attempts; its I/O errors may occur after
/// publication, so inspect storage rather than deleting or resetting it on error.
/// This enforces only the hashing input bound, not enrollment password policy.
pub fn setup(store: &OwnerStore, password: &str) -> Result<IdentityId, SetupError> {
    setup_with_entropy(store, password, |bytes| {
        getrandom::fill(bytes).map_err(|_| SetupError::RandomnessUnavailable)
    })
}

fn setup_with_entropy(
    store: &OwnerStore,
    password: &str,
    mut fill: impl FnMut(&mut [u8]) -> Result<(), SetupError>,
) -> Result<IdentityId, SetupError> {
    if store.load()?.is_some() {
        return Err(StoreError::AlreadyExists.into());
    }
    if !password::valid_length(password) {
        return Err(HashError::InvalidLength.into());
    }
    let mut identity = [0; 16];
    let mut salt = [0; SALT_BYTES];
    fill(&mut identity)?;
    fill(&mut salt)?;
    let identity = IdentityId::from_bytes(identity);
    let hash = password::hash_password(password, &salt)?;
    store.create(identity, &hash)?;
    Ok(identity)
}

/// Return the persisted identity only after password verification succeeds.
///
/// Missing owner and incorrect/out-of-bounds passwords all return `None` without
/// changing storage. Corrupt, unsupported, or unsafe storage returns an error.
/// A successful identity is not a session or a permission grant.
pub fn authenticate(store: &OwnerStore, password: &str) -> Result<Option<IdentityId>, StoreError> {
    let Some(record) = store.load()? else {
        return Ok(None);
    };
    Ok(password::verify_password(password, record.password_hash()).then_some(record.identity()))
}

#[cfg(test)]
mod tests;
