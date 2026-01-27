// ABOUTME: Configuration management for coven tools
// ABOUTME: Writes unified config that all coven tools can read

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Unified coven configuration
#[derive(Debug, Serialize, Deserialize)]
pub struct CovenConfig {
    /// Gateway gRPC address (e.g., "coven.example.com:50051")
    pub gateway: String,

    /// JWT token for authentication
    pub token: String,

    /// Principal ID assigned by gateway
    pub principal_id: String,

    /// Device name
    pub device_name: String,
}

impl CovenConfig {
    /// Returns the config directory path (~/.config/coven)
    pub fn config_dir() -> Result<PathBuf> {
        // Use XDG-style path on all platforms for consistency
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(".config").join("coven"))
    }

    /// Returns the path to the unified config file
    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    /// Returns the path to the device key
    pub fn key_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("device_key"))
    }

    /// Saves the configuration to disk
    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir()?;
        fs::create_dir_all(&dir).context("Failed to create config directory")?;

        let path = Self::config_path()?;
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write(&path, content).context("Failed to write config file")?;

        // Also write token to separate file for backwards compatibility
        let token_path = dir.join("token");
        fs::write(&token_path, &self.token).context("Failed to write token file")?;

        Ok(())
    }

    /// Loads existing configuration
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        let content = fs::read_to_string(&path).context("Failed to read config file")?;
        let config: Self = toml::from_str(&content).context("Failed to parse config")?;
        Ok(config)
    }

    /// Checks if configuration exists
    pub fn exists() -> bool {
        Self::config_path().map(|p| p.exists()).unwrap_or(false)
    }
}
