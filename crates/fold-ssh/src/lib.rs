// ABOUTME: Shared SSH authentication utilities for fold services.
// ABOUTME: Provides key management, fingerprinting, and gRPC auth credentials.

//! # fold-ssh
//!
//! SSH key management and authentication utilities for fold services.
//!
//! This crate provides a unified implementation of SSH-based authentication
//! used by fold-agent, fold-leader, and fold-swarm to communicate with
//! fold-gateway.
//!
//! ## Features
//!
//! - **Key Management**: Load existing SSH keys or generate new ed25519 keys
//! - **Fingerprinting**: Compute SHA256 fingerprints compatible with Go's ssh library
//! - **gRPC Auth**: Apply SSH authentication credentials to tonic requests
//!
//! ## Example
//!
//! ```no_run
//! use fold_ssh::{load_or_generate_key, compute_fingerprint, SshAuthCredentials};
//! use std::path::PathBuf;
//!
//! // Load or generate a key
//! let key_path = PathBuf::from("/path/to/key");
//! let private_key = load_or_generate_key(&key_path).expect("key should load");
//!
//! // Compute fingerprint for identification
//! let fingerprint = compute_fingerprint(private_key.public_key()).expect("fingerprint should compute");
//! println!("Key fingerprint: {}", fingerprint);
//!
//! // Create auth credentials for gRPC
//! let creds = SshAuthCredentials::new(&private_key).expect("credentials should create");
//!
//! // Apply to a gRPC request
//! let mut request = tonic::Request::new(());
//! creds.apply_to_request(&mut request).expect("should apply");
//! ```

mod credentials;
mod error;
mod fingerprint;
mod key;

// Re-export primary types and functions
pub use credentials::{current_timestamp, generate_nonce, sign_message, SshAuthCredentials};
pub use error::{Result, SshError};
pub use fingerprint::compute_fingerprint;
pub use key::{
    default_agent_key_path, default_client_key_path, default_swarm_key_path, generate_key,
    load_key, load_or_generate_key, xdg_config_dir,
};

// Re-export ssh_key types for convenience
pub use ssh_key::{PrivateKey, PublicKey};
