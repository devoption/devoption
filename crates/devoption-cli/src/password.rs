//! Password hashing and verification for future browser owner setup/sign-in.
//!
//! No identity, permissions, sessions, persistence, or salt generation lives here.
//! Owner setup generates a fresh random salt internally for each password; users
//! never supply salts or session keys. Only temporary test fixtures exercise the
//! setup/sign-in components today. Password policy, blocking-work isolation, and
//! rate limits must be added before exposing it through HTTP. Memory erasure is not
//! guaranteed, and callers must treat returned records as sensitive storage data.

use std::{error::Error, fmt};

use argon2::{
    Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version,
    password_hash::phc::PasswordHash,
};

/// Input resource limit in UTF-8 bytes, without truncation or normalization.
/// This is not an enrollment password-strength policy.
pub const MAX_PASSWORD_BYTES: usize = 1024;
/// Required salt length. Owner setup supplies fresh OS randomness.
pub const SALT_BYTES: usize = 16;
const MAX_RECORD_BYTES: usize = 128;
const HASH_BYTES: usize = 32;
// OWASP's Argon2id minimum profile: 19 MiB, two passes, one lane.
const MEMORY_KIB: u32 = 19 * 1024;
const ITERATIONS: u32 = 2;
const LANES: u32 = 1;

/// Hashing errors never include the password, salt, or resulting hash.
#[derive(Debug, PartialEq, Eq)]
pub enum HashError {
    InvalidLength,
    HashingFailed,
}

impl fmt::Display for HashError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLength => "password must contain 1 to 1024 UTF-8 bytes",
            Self::HashingFailed => "password hashing failed",
        })
    }
}

impl Error for HashError {}

/// Produce an Argon2id v19 PHC record using an internally supplied salt.
///
/// This primitive creates no account or access grant. Callers must supply a fresh
/// cryptographically random salt, enforce enrollment policy, and store only this
/// record, never the plaintext password.
pub fn hash_password(password: &str, salt: &[u8; SALT_BYTES]) -> Result<String, HashError> {
    if !valid_length(password) {
        return Err(HashError::InvalidLength);
    }
    hasher()
        .hash_password_with_salt(password.as_bytes(), salt)
        .map(|record| record.to_string())
        .map_err(|_| HashError::HashingFailed)
}

/// Check password knowledge only; this does not identify or authorize an owner.
///
/// Malformed, unsupported, or out-of-bounds records fail before Argon2 runs.
/// Only this component's exact cost profile is accepted; future cost migrations
/// must explicitly extend the allowlist rather than trusting stored parameters.
pub fn verify_password(password: &str, record: &str) -> bool {
    if !valid_length(password) {
        return false;
    }
    let Some(record) = supported_record(record) else {
        return false;
    };
    hasher()
        .verify_password(password.as_bytes(), &record)
        .is_ok()
}

pub(crate) fn valid_length(password: &str) -> bool {
    !password.is_empty() && password.len() <= MAX_PASSWORD_BYTES
}

fn hasher() -> Argon2<'static> {
    let params = Params::new(MEMORY_KIB, ITERATIONS, LANES, Some(HASH_BYTES))
        .expect("fixed Argon2 parameters are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub(crate) fn supported_record(record: &str) -> Option<PasswordHash> {
    if record.len() > MAX_RECORD_BYTES {
        return None;
    }
    let record = PasswordHash::new(record).ok()?;
    let supported = record.algorithm.as_str() == "argon2id"
        && record.version == Some(19)
        && record.params.iter().count() == 3
        && record.params.get_decimal("m") == Some(MEMORY_KIB)
        && record.params.get_decimal("t") == Some(ITERATIONS)
        && record.params.get_decimal("p") == Some(LANES)
        && record
            .salt
            .as_ref()
            .is_some_and(|salt| salt.len() == SALT_BYTES)
        && record
            .hash
            .as_ref()
            .is_some_and(|hash| hash.len() == HASH_BYTES);
    supported.then_some(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::password_hash::phc::{Output, Salt};
    use std::sync::OnceLock;

    // Public, deterministic test data. Never use these as real credentials/salts.
    const PASSWORD: &str = "test fixture password only";
    const SALT: [u8; SALT_BYTES] = [0x5a; SALT_BYTES];

    fn fixture_record() -> &'static str {
        static RECORD: OnceLock<String> = OnceLock::new();
        RECORD.get_or_init(|| hash_password(PASSWORD, &SALT).unwrap())
    }

    #[test]
    fn correct_password_matches_and_incorrect_passwords_do_not() {
        let record = fixture_record();
        assert!(record.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert!(record.len() <= MAX_RECORD_BYTES);
        assert!(!record.contains(PASSWORD));
        assert!(verify_password(PASSWORD, record));
        assert!(!verify_password("test fixture password onlz", record));
        assert!(!verify_password("test fixture password only extra", record));
    }

    #[test]
    fn separate_salts_produce_separate_verifiable_records() {
        let other = hash_password(PASSWORD, &[0xa5; SALT_BYTES]).unwrap();
        assert_ne!(fixture_record(), other);
        assert!(verify_password(PASSWORD, &other));
    }

    #[test]
    fn whitespace_unicode_and_embedded_nulls_are_preserved() {
        let password = "  café 🔑\0\n";
        let record = hash_password(password, &SALT).unwrap();
        assert!(verify_password(password, &record));
        for changed in [password.trim(), "  cafe\u{301} 🔑\0\n", "  café 🔑"] {
            assert!(!verify_password(changed, &record));
        }
    }

    #[test]
    fn password_size_is_bounded_in_bytes_without_truncation() {
        for invalid in [String::new(), "x".repeat(MAX_PASSWORD_BYTES + 1)] {
            assert_eq!(
                hash_password(&invalid, &SALT),
                Err(HashError::InvalidLength)
            );
            assert!(!verify_password(&invalid, fixture_record()));
        }
        let password = "🔑".repeat(MAX_PASSWORD_BYTES / 4);
        assert_eq!(password.len(), MAX_PASSWORD_BYTES);
        let record = hash_password(&password, &SALT).unwrap();
        assert!(verify_password(&password, &record));
        let oversized = format!("{password}x");
        assert_eq!(
            hash_password(&oversized, &SALT),
            Err(HashError::InvalidLength)
        );
        assert!(!verify_password(&oversized, &record));
    }

    #[test]
    fn corrupt_and_unsupported_records_are_rejected_before_hashing() {
        let valid = fixture_record();
        let mut invalid = vec![
            String::new(),
            "not a PHC record".into(),
            "x".repeat(MAX_RECORD_BYTES + 1),
            format!("{valid}$extra"),
            valid.replace("argon2id", "argon2i"),
            valid.replace("v=19", "v=16"),
            valid.replace("$v=19", ""),
            valid.replace("m=19456", "m=8"),
            valid.replace("m=19456", "m=4294967295"),
            valid.replace("t=2", "t=4294967295"),
            valid.replace("p=1", "p=16777215"),
            valid.replace("t=2,", ""),
            valid.replace("p=1", "p=1,t=2"),
            valid.replace("p=1", "p=1,data=eA"),
            valid.replace("p=1", "p=1,extra=1"),
        ];
        for salt in [None, Some(Salt::new(&[0x5a; SALT_BYTES - 1]).unwrap())] {
            let mut record = PasswordHash::new(valid).unwrap();
            record.salt = salt;
            invalid.push(record.to_string());
        }
        for hash in [None, Some(Output::new(&[0; HASH_BYTES - 1]).unwrap())] {
            let mut record = PasswordHash::new(valid).unwrap();
            record.hash = hash;
            invalid.push(record.to_string());
        }
        for record in invalid {
            assert!(supported_record(&record).is_none());
            assert!(!verify_password(PASSWORD, &record));
        }
    }

    #[test]
    fn tampered_hash_with_supported_parameters_does_not_match() {
        let mut record = PasswordHash::new(fixture_record()).unwrap();
        record.hash = Some(Output::new(&[0; HASH_BYTES]).unwrap());
        let record = record.to_string();
        assert!(supported_record(&record).is_some());
        assert!(!verify_password(PASSWORD, &record));
    }
}
