//! Identity references, independent of authentication methods and permissions.

/// An opaque, stable reference assigned by a future account store.
///
/// The store must assign unique IDs and persist them independently of passwords,
/// session tokens, usernames, or external-provider identifiers. Constructing an
/// ID does not create an account, authenticate it, or grant any permission.
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
