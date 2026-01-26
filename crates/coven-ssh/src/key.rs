// ABOUTME: SSH key loading and generation utilities.
// ABOUTME: Handles ed25519 key pair creation and persistence to filesystem.

use crate::error::{Result, SshError};
use ssh_key::{Algorithm, LineEnding, PrivateKey};
use std::path::{Path, PathBuf};

/// Get XDG-style config directory (~/.config/fold).
///
/// Uses `XDG_CONFIG_HOME` if set, otherwise falls back to `~/.config`.
pub fn xdg_config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .map(|p| p.join("coven"))
}

/// Get the default SSH key path for coven-agent (~/.config/coven/agent_key).
pub fn default_agent_key_path() -> Option<PathBuf> {
    xdg_config_dir().map(|p| p.join("agent_key"))
}

/// Get the default SSH key path for coven-tui/clients (~/.config/coven/client_key).
pub fn default_client_key_path() -> Option<PathBuf> {
    xdg_config_dir().map(|p| p.join("client_key"))
}

/// Get the default SSH key path for coven-swarm (~/.config/coven/coven-swarm/agent_key).
pub fn default_swarm_key_path() -> Option<PathBuf> {
    xdg_config_dir().map(|p| p.join("coven-swarm").join("agent_key"))
}

/// Load an existing SSH private key from disk.
///
/// # Errors
/// Returns an error if the file cannot be read or parsed.
pub fn load_key(key_path: &Path) -> Result<PrivateKey> {
    let key_data = std::fs::read_to_string(key_path).map_err(|e| SshError::ReadKey {
        path: key_path.to_path_buf(),
        source: e,
    })?;

    PrivateKey::from_openssh(&key_data).map_err(|e| SshError::ParseKey {
        path: key_path.to_path_buf(),
        source: e,
    })
}

/// Generate a new ed25519 SSH key pair and save to disk.
///
/// Creates the parent directory if needed. Sets Unix permissions to 0600
/// on the private key. Also writes the public key with `.pub` extension.
///
/// # Errors
/// Returns an error if directory creation, key generation, or file writing fails.
pub fn generate_key(key_path: &Path) -> Result<PrivateKey> {
    eprintln!("Generating new SSH key at {}...", key_path.display());

    // Ensure parent directory exists
    if let Some(parent) = key_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| SshError::CreateDirectory {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }

    // Generate ed25519 key
    let private_key = PrivateKey::random(&mut rand::thread_rng(), Algorithm::Ed25519)
        .map_err(SshError::GenerateKey)?;

    // Write private key in OpenSSH format
    let private_key_str = private_key
        .to_openssh(LineEnding::LF)
        .map_err(SshError::SerializeKey)?;

    std::fs::write(key_path, private_key_str.as_bytes()).map_err(|e| SshError::WriteKey {
        path: key_path.to_path_buf(),
        source: e,
    })?;

    // Set restrictive permissions on Unix (0600 = rw-------)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o600)).map_err(
            |e| SshError::SetPermissions {
                path: key_path.to_path_buf(),
                source: e,
            },
        )?;
    }

    // Write public key with .pub extension
    let pub_key_path = key_path.with_extension("pub");
    let public_key = private_key.public_key();
    let public_key_str = public_key.to_openssh().map_err(SshError::SerializeKey)?;

    std::fs::write(&pub_key_path, public_key_str.as_bytes()).map_err(|e| SshError::WriteKey {
        path: pub_key_path.clone(),
        source: e,
    })?;

    eprintln!("SSH key generated!");
    eprintln!("  Private: {}", key_path.display());
    eprintln!("  Public:  {}", pub_key_path.display());

    Ok(private_key)
}

/// Load an existing SSH key or generate a new one if it doesn't exist.
///
/// This is the primary entry point for obtaining an SSH key. If the key file
/// exists, it will be loaded. Otherwise, a new ed25519 key pair is generated.
///
/// # Errors
/// Returns an error if key loading fails (for existing keys) or generation fails.
pub fn load_or_generate_key(key_path: &Path) -> Result<PrivateKey> {
    if key_path.exists() {
        load_key(key_path)
    } else {
        generate_key(key_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_xdg_config_dir_returns_some() {
        // Should return Some path on most systems
        let dir = xdg_config_dir();
        assert!(dir.is_some() || std::env::var_os("HOME").is_none());
    }

    #[test]
    fn test_default_agent_key_path_ends_with_agent_key() {
        if let Some(path) = default_agent_key_path() {
            assert!(path.ends_with("agent_key"));
            assert!(path.to_string_lossy().contains("coven"));
        }
    }

    #[test]
    fn test_default_swarm_key_path_ends_with_agent_key() {
        if let Some(path) = default_swarm_key_path() {
            assert!(path.ends_with("agent_key"));
            assert!(path.to_string_lossy().contains("coven-swarm"));
        }
    }

    #[test]
    fn test_generate_and_load_key() {
        let temp_dir = TempDir::new().expect("should create temp dir");
        let key_path = temp_dir.path().join("test_key");

        // Generate key
        let generated = generate_key(&key_path).expect("should generate key");

        // Verify files exist
        assert!(key_path.exists(), "private key should exist");
        assert!(
            key_path.with_extension("pub").exists(),
            "public key should exist"
        );

        // Load key
        let loaded = load_key(&key_path).expect("should load key");

        // Verify same key
        assert_eq!(
            generated.public_key().to_openssh().unwrap(),
            loaded.public_key().to_openssh().unwrap(),
            "loaded key should match generated key"
        );
    }

    #[test]
    fn test_load_or_generate_generates_when_missing() {
        let temp_dir = TempDir::new().expect("should create temp dir");
        let key_path = temp_dir.path().join("new_key");

        assert!(!key_path.exists(), "key should not exist initially");

        let key = load_or_generate_key(&key_path).expect("should generate key");
        assert!(key_path.exists(), "key should exist after generation");
        assert!(key.public_key().key_data().is_ed25519());
    }

    #[test]
    fn test_load_or_generate_loads_when_exists() {
        let temp_dir = TempDir::new().expect("should create temp dir");
        let key_path = temp_dir.path().join("existing_key");

        // Generate first
        let original = generate_key(&key_path).expect("should generate key");

        // Load via load_or_generate
        let loaded = load_or_generate_key(&key_path).expect("should load key");

        assert_eq!(
            original.public_key().to_openssh().unwrap(),
            loaded.public_key().to_openssh().unwrap(),
            "should load existing key, not generate new"
        );
    }

    #[test]
    fn test_generated_key_is_ed25519() {
        let temp_dir = TempDir::new().expect("should create temp dir");
        let key_path = temp_dir.path().join("ed25519_key");

        let key = generate_key(&key_path).expect("should generate key");
        assert!(key.public_key().key_data().is_ed25519());
    }

    #[cfg(unix)]
    #[test]
    fn test_private_key_has_restrictive_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().expect("should create temp dir");
        let key_path = temp_dir.path().join("secure_key");

        generate_key(&key_path).expect("should generate key");

        let metadata = std::fs::metadata(&key_path).expect("should read metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "private key should have 0600 permissions");
    }

    #[test]
    fn test_load_key_file_not_found() {
        let temp_dir = TempDir::new().expect("should create temp dir");
        let nonexistent_path = temp_dir.path().join("nonexistent_key");

        let result = load_key(&nonexistent_path);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(matches!(err, crate::error::SshError::ReadKey { .. }));
    }

    #[test]
    fn test_load_key_invalid_format() {
        let temp_dir = TempDir::new().expect("should create temp dir");
        let invalid_key_path = temp_dir.path().join("invalid_key");

        // Write invalid content
        std::fs::write(&invalid_key_path, "not a valid ssh key").expect("should write file");

        let result = load_key(&invalid_key_path);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(matches!(err, crate::error::SshError::ParseKey { .. }));
    }

    #[test]
    fn test_xdg_config_home_override() {
        // Save original value
        let original = std::env::var_os("XDG_CONFIG_HOME");

        // Set custom XDG_CONFIG_HOME
        std::env::set_var("XDG_CONFIG_HOME", "/custom/config");

        let dir = xdg_config_dir();
        assert!(dir.is_some());
        assert_eq!(dir.unwrap(), PathBuf::from("/custom/config/coven"));

        // Restore original value
        match original {
            Some(val) => std::env::set_var("XDG_CONFIG_HOME", val),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}
