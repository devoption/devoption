//! Identity references, independent of authentication methods and permissions.

/// An opaque, stable reference assigned by owner setup and persisted by its store.
///
/// Owner setup generates IDs independently of passwords, session tokens, usernames,
/// or external-provider identifiers; storage preserves them across restarts.
/// Constructing an ID does not create an account, authenticate it, or grant permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IdentityId([u8; 16]);

impl IdentityId {
    /// Wrap an existing ID. This neither generates an ID nor verifies an account.
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Serialize this stable reference without deriving it from credentials.
    pub const fn to_bytes(self) -> [u8; 16] {
        self.0
    }
}
