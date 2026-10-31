//! Local HTTP service and its authentication, session, and owner-storage components.
//!
//! The CLI starts the service through [`run`]; this crate owns HTTP/runtime behavior
//! and uses devoption-ui for rendering. Existing-owner HTTP sign-in is available
//! with explicitly configured storage; local/SSH setup is explicit and browser
//! owner enrollment routes remain closed. Existing owners can invite members or
//! enable owner-approved registration; members share the workspace. Configured page reads require a session;
//! Sign in and the unconfigured preview remain public.

mod server;
#[cfg(unix)]
mod users;

pub use server::{
    ProjectSecretsOptions, PublicOrigin, run, run_with_build_history, run_with_deployments,
    run_with_execution, run_with_managed_repository_root, run_with_project_secrets,
    run_with_public_origin, run_with_repository_root,
};

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
#[cfg(unix)]
mod profile;
#[cfg(unix)]
mod project_secrets;
pub mod session;
