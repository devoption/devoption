//! Explicit local/SSH owner setup; never expose this operation through HTTP.
//!
//! Authority comes from the operator's OS access to the private owner store.
//! Terminal interaction belongs to the CLI; policy and publication stay here.

use super::{SetupError, setup};
use crate::{identity::IdentityId, owner_store::OwnerStore, password::MAX_PASSWORD_BYTES};
use std::{error::Error, fmt, path::Path};
use subtle::ConstantTimeEq;

pub const MIN_PASSWORD_CHARACTERS: usize = 15;

#[derive(Debug)]
pub enum LocalSetupError {
    Setup(SetupError),
    PasswordPolicy,
    ConfirmationMismatch,
}

impl fmt::Display for LocalSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Setup(error) => error.fmt(f),
            Self::PasswordPolicy => f.write_str(
                "use at least 15 characters, at most 1024 UTF-8 bytes, and no control characters",
            ),
            Self::ConfirmationMismatch => f.write_str("passwords do not match"),
        }
    }
}

impl Error for LocalSetupError {}

/// Retains the validated directory descriptor while the CLI obtains a password.
pub struct LocalSetup {
    store: OwnerStore,
}

impl LocalSetup {
    /// Fail before prompting if the store is unsafe, corrupt, or already configured.
    pub fn open(path: &Path) -> Result<Self, LocalSetupError> {
        let store = OwnerStore::open_absolute(path)
            .map_err(|error| LocalSetupError::Setup(error.into()))?;
        if store
            .load()
            .map_err(|error| LocalSetupError::Setup(error.into()))?
            .is_some()
        {
            return Err(LocalSetupError::Setup(
                crate::owner_store::StoreError::AlreadyExists.into(),
            ));
        }
        Ok(Self { store })
    }

    /// Check exact confirmation and policy, then recheck/create once in storage.
    /// No trimming or normalization, session issuance, or directory creation.
    pub fn complete(
        self,
        password: &str,
        confirmation: &str,
    ) -> Result<IdentityId, LocalSetupError> {
        if password.len() > MAX_PASSWORD_BYTES
            || password.chars().count() < MIN_PASSWORD_CHARACTERS
            || password.chars().any(char::is_control)
        {
            return Err(LocalSetupError::PasswordPolicy);
        }
        if !bool::from(password.as_bytes().ct_eq(confirmation.as_bytes())) {
            return Err(LocalSetupError::ConfirmationMismatch);
        }
        setup(&self.store, password).map_err(LocalSetupError::Setup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = fs::canonicalize(dir.path()).unwrap();
        (dir, path)
    }

    #[test]
    fn rejected_policy_and_confirmation_never_publish() {
        let (_dir, path) = fixture();
        for password in [
            "".into(),
            "a".repeat(14),
            "a".repeat(1025),
            "a\n".repeat(15),
        ] {
            assert!(matches!(
                LocalSetup::open(&path)
                    .unwrap()
                    .complete(&password, &password),
                Err(LocalSetupError::PasswordPolicy)
            ));
        }
        assert!(matches!(
            LocalSetup::open(&path)
                .unwrap()
                .complete("fixture passphrase", "different fixture"),
            Err(LocalSetupError::ConfirmationMismatch)
        ));
        assert_eq!(fs::read_dir(path).unwrap().count(), 0);
    }

    #[test]
    fn exact_unicode_and_whitespace_and_boundaries_are_accepted() {
        for password in [
            "é".repeat(15),
            "a".repeat(15),
            "a".repeat(1024),
            "  fixture phrase  ".into(),
        ] {
            let (_dir, path) = fixture();
            let identity = LocalSetup::open(&path)
                .unwrap()
                .complete(&password, &password)
                .unwrap();
            let store = OwnerStore::open_absolute(&path).unwrap();
            assert_eq!(
                super::super::authenticate(&store, &password).unwrap(),
                Some(identity)
            );
            if password.trim() != password {
                assert_eq!(
                    super::super::authenticate(&store, password.trim()).unwrap(),
                    None
                );
            }
            assert!(
                !fs::read(path.join("owner.bin"))
                    .unwrap()
                    .windows(password.len())
                    .any(|part| part == password.as_bytes())
            );
        }
    }

    #[test]
    fn concurrent_preparations_cannot_overwrite_and_corruption_fails_closed() {
        let (_dir, path) = fixture();
        let first = LocalSetup::open(&path).unwrap();
        let second = LocalSetup::open(&path).unwrap();
        first
            .complete("first fixture phrase", "first fixture phrase")
            .unwrap();
        let before = fs::read(path.join("owner.bin")).unwrap();
        assert!(
            second
                .complete("other fixture phrase", "other fixture phrase")
                .is_err()
        );
        assert!(LocalSetup::open(&path).is_err());
        assert_eq!(fs::read(path.join("owner.bin")).unwrap(), before);
        fs::write(path.join("owner.bin"), b"corrupt fixture").unwrap();
        assert!(LocalSetup::open(&path).is_err());
        assert_eq!(
            fs::read(path.join("owner.bin")).unwrap(),
            b"corrupt fixture"
        );
    }
}
