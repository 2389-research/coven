// ABOUTME: Configuration loading and validation for the Matrix bridge.
// ABOUTME: Supports TOML config files with environment variable expansion.

use crate::error::{BridgeError, Result};
use serde::Deserialize;
use std::path::PathBuf;
use tracing::warn;

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
    /// Recovery key for E2EE session recovery (future feature, not yet implemented).
    #[serde(default)]
    pub recovery_key: Option<String>,
    #[serde(default)]
    pub state_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GatewayConfig {
    /// Gateway hostname (e.g., "localhost" or "coven.example.com")
    pub host: String,
    /// Gateway gRPC port (default: 6666)
    #[serde(default = "default_gateway_port")]
    pub port: u16,
    /// Use TLS for gRPC connection (default: false)
    #[serde(default)]
    pub tls: bool,
    /// Authentication token
    #[serde(default)]
    pub token: Option<String>,
}

fn default_gateway_port() -> u16 {
    6666
}

impl GatewayConfig {
    /// Construct the gRPC endpoint URI from host/port/tls settings.
    pub fn endpoint_uri(&self) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        format!("{}://{}:{}", scheme, self.host, self.port)
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BridgeConfig {
    /// Restrict to specific Matrix room IDs (empty = allow all rooms)
    #[serde(default)]
    pub allowed_rooms: Vec<String>,
    /// Restrict to specific Matrix user IDs (empty = allow all users)
    #[serde(default)]
    pub allowed_senders: Vec<String>,
    /// Only respond to messages with this prefix
    #[serde(default)]
    pub command_prefix: Option<String>,
    /// Show typing indicator while agent is responding
    #[serde(default = "default_typing_indicator")]
    pub typing_indicator: bool,
}

fn default_typing_indicator() -> bool {
    true
}

impl Config {
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        // Use Linux XDG-style path (~/.config) on all platforms for consistency with other coven tools
        let path = path
            .or_else(|| {
                dirs::home_dir().map(|d| d.join(".config").join("coven").join("matrix-bridge.toml"))
            })
            .ok_or_else(|| BridgeError::Config("Could not determine config path".into()))?;

        let contents = std::fs::read_to_string(&path).map_err(|e| {
            BridgeError::Config(format!("Failed to read config from {:?}: {}", path, e))
        })?;

        // Expand environment variables with warning on undefined vars.
        // Note: Undefined environment variables are replaced with empty strings
        // but a warning is logged to help users identify configuration issues.
        let contents = shellexpand::env_with_context_no_errors(&contents, |var: &str| {
            match std::env::var(var) {
                Ok(val) => Some(val),
                Err(_) => {
                    warn!(
                        variable = %var,
                        "Environment variable not defined, using empty string"
                    );
                    Some(String::new())
                }
            }
        });

        let config: Config = toml::from_str(&contents)
            .map_err(|e| BridgeError::Config(format!("Failed to parse config: {}", e)))?;

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        // Check for empty or whitespace-only values
        if self.matrix.homeserver.trim().is_empty() {
            return Err(BridgeError::Config("matrix.homeserver is required".into()));
        }
        if self.matrix.username.trim().is_empty() {
            return Err(BridgeError::Config("matrix.username is required".into()));
        }
        if self.matrix.password.is_empty() {
            return Err(BridgeError::Config("matrix.password is required".into()));
        }
        if self.gateway.host.trim().is_empty() {
            return Err(BridgeError::Config("gateway.host is required".into()));
        }
        if self.gateway.port == 0 {
            return Err(BridgeError::Config("gateway.port must be non-zero".into()));
        }
        // Validate homeserver looks like a URL
        if !self.matrix.homeserver.starts_with("http://")
            && !self.matrix.homeserver.starts_with("https://")
        {
            return Err(BridgeError::Config(
                "matrix.homeserver must start with http:// or https://".into(),
            ));
        }
        Ok(())
    }

    pub fn is_room_allowed(&self, room_id: &str) -> bool {
        self.bridge.allowed_rooms.is_empty()
            || self.bridge.allowed_rooms.iter().any(|r| r == room_id)
    }

    pub fn is_sender_allowed(&self, sender: &str) -> bool {
        self.bridge.allowed_senders.is_empty()
            || self.bridge.allowed_senders.iter().any(|s| s == sender)
    }
}
