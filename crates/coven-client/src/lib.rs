// ABOUTME: Fold gateway client library shared between iOS and TUI
// ABOUTME: Provides gateway communication, streaming, and state management

// Allow empty lines after doc comments in generated UniFFI scaffolding code.
#![allow(clippy::empty_line_after_doc_comments)]

mod client;
mod error;
mod models;

pub use client::CovenClient;
pub use error::CovenError;
pub use models::*;

// UniFFI scaffolding
uniffi::include_scaffolding!("coven_client");

// ============================================================================
// Standalone Functions (for FFI)
// ============================================================================

/// Generate or load an SSH key at the given path and return its fingerprint.
///
/// This creates a proper OpenSSH-formatted Ed25519 key that can be loaded
/// by the Rust ssh_key crate. Use this from Swift instead of generating keys
/// natively to ensure format compatibility.
///
/// Returns the hex-encoded SHA256 fingerprint of the public key.
///
/// # Errors
/// Returns `CovenError::Api` with a descriptive message if:
/// - The key file cannot be read or written
/// - The key format is invalid
/// - Directory creation fails
pub fn generate_ssh_key(key_path: String) -> Result<String, CovenError> {
    use coven_ssh::{compute_fingerprint, load_or_generate_key};
    use std::path::Path;

    let path = Path::new(&key_path);
    let key =
        load_or_generate_key(path).map_err(|e| CovenError::Api(format!("SSH key error: {}", e)))?;

    let fingerprint = compute_fingerprint(key.public_key())
        .map_err(|e| CovenError::Api(format!("SSH fingerprint error: {}", e)))?;

    Ok(fingerprint)
}

// ============================================================================
// Callback Traits
// ============================================================================

/// Callback for streaming events from agents
pub trait StreamCallback: Send + Sync {
    fn on_event(&self, agent_id: String, event: StreamEvent);
}

/// Callback for state changes (for UI updates)
pub trait StateCallback: Send + Sync {
    fn on_connection_status(&self, status: ConnectionStatus);
    fn on_messages_changed(&self, agent_id: String);
    fn on_queue_changed(&self, agent_id: String, count: u32);
    fn on_unread_changed(&self, agent_id: String, count: u32);
    fn on_streaming_changed(&self, agent_id: String, is_streaming: bool);
}
