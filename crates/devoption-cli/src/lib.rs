//! Reusable local service components. The HTTP preview validates unsafe requests
//! against this session core but cannot issue sessions; only tests provision them.

pub mod identity;
#[cfg(unix)]
pub mod owner_store;
pub mod password;
pub mod session;
