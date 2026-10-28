//! beaM. core crate: edit model, sidecars, color science, and errors.

/// The official user-facing display name of the application.
/// Note the required trailing dot: "beaM."
pub const APP_NAME: &str = "beaM.";

pub mod color;
pub mod error;
pub mod sidecar;

pub use error::{BeamError, Result};
pub use sidecar::{EditStack, Sidecar, CURRENT_FORMAT_VERSION};
