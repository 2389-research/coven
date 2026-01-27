// ABOUTME: Configuration loading and validation for the Matrix bridge.
// ABOUTME: Supports TOML config files with environment variable expansion.

use crate::error::{BridgeError, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub matrix: MatrixConfig,
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub bridge: BridgeConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MatrixConfig {
    pub homeserver: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub recovery_key: Option<String>,
    #[serde(default)]
    pub state_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GatewayConfig {
    pub url: String,
    #[serde(default)]
    pub token: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BridgeConfig {
    #[serde(default)]
    pub allowed_rooms: Vec<String>,
    #[serde(default)]
    pub command_prefix: Option<String>,
    #[serde(default = "default_typing_indicator")]
    pub typing_indicator: bool,
}

fn default_typing_indicator() -> bool {
    true
}

impl Config {
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let path = path
            .or_else(|| {
                dirs::config_dir().map(|d| d.join("coven").join("matrix-bridge.toml"))
            })
            .ok_or_else(|| BridgeError::Config("Could not determine config path".into()))?;

        let contents = std::fs::read_to_string(&path).map_err(|e| {
            BridgeError::Config(format!("Failed to read config from {:?}: {}", path, e))
        })?;

        // Expand environment variables
        let contents = shellexpand::env(&contents)
            .map_err(|e| BridgeError::Config(format!("Failed to expand env vars: {}", e)))?;

        let config: Config = toml::from_str(&contents)
            .map_err(|e| BridgeError::Config(format!("Failed to parse config: {}", e)))?;

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.matrix.homeserver.is_empty() {
            return Err(BridgeError::Config("matrix.homeserver is required".into()));
        }
        if self.matrix.username.is_empty() {
            return Err(BridgeError::Config("matrix.username is required".into()));
        }
        if self.gateway.url.is_empty() {
            return Err(BridgeError::Config("gateway.url is required".into()));
        }
        Ok(())
    }

    pub fn is_room_allowed(&self, room_id: &str) -> bool {
        self.bridge.allowed_rooms.is_empty()
            || self.bridge.allowed_rooms.iter().any(|r| r == room_id)
    }
}
