// ABOUTME: SSH authentication utilities for gateway connections
// ABOUTME: Re-exports from coven-ssh with additional helpers

pub use coven_ssh::{
    compute_fingerprint, default_agent_key_path, load_or_generate_key, SshAuthCredentials,
};
