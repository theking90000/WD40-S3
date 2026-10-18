//! Embeddable portal service. The caller owns the listener and runtime.

mod api;
#[cfg(feature = "embed-console")]
mod assets;
#[cfg(feature = "cli")]
pub mod cli;
mod portal;
pub mod sync;

pub use api::auth::{CurrentUser, OidcConfig};
pub use portal::Portal;
