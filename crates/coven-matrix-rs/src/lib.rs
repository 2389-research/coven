// ABOUTME: Library root for coven-matrix-rs.
// ABOUTME: Exports bridge, config, and error modules.

pub mod config;
pub mod error;

pub use config::Config;
pub use error::{BridgeError, Result};
