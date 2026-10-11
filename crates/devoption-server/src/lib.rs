//! Local HTTP service and its authentication, session, and owner-storage components.
//!
//! The CLI starts the service through [`run`]; this crate owns HTTP/runtime behavior
//! and uses devoption-ui for rendering. The preview cannot issue owner sessions yet;
//! only test fixtures provision them.

mod server;

pub use server::run;

#[cfg(unix)]
pub mod browser_session;
pub mod identity;
#[cfg(unix)]
pub mod owner_auth;
#[cfg(unix)]
pub mod owner_store;
pub mod password;
pub mod password_form;
#[cfg(unix)]
pub mod preauth_csrf;
pub mod session;
