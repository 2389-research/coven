// ABOUTME: Library root for coven-matrix-rs.
// ABOUTME: Exports bridge, config, and error modules.

pub mod config;
pub mod error;
pub mod gateway;
pub mod matrix;

pub use config::Config;
pub use error::{BridgeError, Result};
pub use gateway::GatewayClient;
pub use matrix::MatrixClient;
