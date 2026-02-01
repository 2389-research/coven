// ABOUTME: Configuration loading and validation for the Slack bridge.
// ABOUTME: Supports TOML config files with environment variable expansion.

use crate::error::{BridgeError, Result};
use serde::Deserialize;
use std::path::PathBuf;
use tracing::warn;

/// Top-level configuration structure for coven-slack-rs.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub slack: SlackConfig,
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub bridge: BridgeConfig,
}

/// Slack API credentials for Socket Mode connection.
#[derive(Clone, Deserialize)]
pub struct SlackConfig {
    /// App-level token (xapp-...) for Socket Mode WebSocket connection.
    pub app_token: String,
    /// Bot token (xoxb-...) for posting messages and API calls.
    pub bot_token: String,
}

impl std::fmt::Debug for SlackConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlackConfig")
            .field("app_token", &"[REDACTED]")
            .field("bot_token", &"[REDACTED]")
            .finish()
    }
}

/// Gateway connection configuration.
#[derive(Clone, Deserialize)]
pub struct GatewayConfig {
    /// gRPC URL for coven-gateway (e.g., "http://localhost:6666").
    pub url: String,
    /// Authentication token for gateway API calls.
    #[serde(default)]
    pub token: Option<String>,
}

impl std::fmt::Debug for GatewayConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayConfig")
            .field("url", &self.url)
            .field("token", &self.token.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

/// Bridge behavior configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct BridgeConfig {
    /// List of allowed channel IDs (empty = allow all channels the bot is in).
    #[serde(default)]
    pub allowed_channels: Vec<String>,

    /// Response trigger mode: when to respond to messages.
    #[serde(default)]
    pub response_mode: ResponseMode,

    /// Show typing indicator while agent is processing.
    #[serde(default = "default_typing_indicator")]
    pub typing_indicator: bool,

    /// Always reply in threads (keeps channels cleaner).
    #[serde(default = "default_thread_replies")]
    pub thread_replies: bool,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            allowed_channels: Vec::new(),
            response_mode: ResponseMode::default(),
            typing_indicator: default_typing_indicator(),
            thread_replies: default_thread_replies(),
        }
    }
}

fn default_typing_indicator() -> bool {
    true
}

fn default_thread_replies() -> bool {
    true
}

/// Response mode determines when the bot responds to messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResponseMode {
    /// Only respond when @mentioned, in DMs, or in threads.
    #[default]
    Mention,
    /// Respond to every message in allowed channels.
    All,
}

impl Config {
    /// Load configuration from the specified path or default location.
    ///
    /// Default location: `~/.config/coven/slack-bridge.toml`
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let path = path
            .or_else(|| dirs::config_dir().map(|d| d.join("coven").join("slack-bridge.toml")))
            .ok_or_else(|| BridgeError::Config("Could not determine config path".into()))?;

        let contents = std::fs::read_to_string(&path).map_err(|e| {
            BridgeError::Config(format!("Failed to read config from {:?}: {}", path, e))
        })?;

        // Expand environment variables, warning on undefined vars.
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

    /// Validate that required fields are present and properly formatted.
    fn validate(&self) -> Result<()> {
        if self.slack.app_token.is_empty() {
            return Err(BridgeError::Config("slack.app_token is required".into()));
        }
        if !self.slack.app_token.starts_with("xapp-") {
            return Err(BridgeError::Config(
                "slack.app_token must start with 'xapp-' (app-level token)".into(),
            ));
        }
        if self.slack.bot_token.is_empty() {
            return Err(BridgeError::Config("slack.bot_token is required".into()));
        }
        if !self.slack.bot_token.starts_with("xoxb-") {
            return Err(BridgeError::Config(
                "slack.bot_token must start with 'xoxb-' (bot token)".into(),
            ));
        }
        if self.gateway.url.is_empty() {
            return Err(BridgeError::Config("gateway.url is required".into()));
        }
        Ok(())
    }

    /// Check if a channel is in the allowed list.
    /// Returns true if allowed_channels is empty (allow all) or channel is in list.
    pub fn is_channel_allowed(&self, channel_id: &str) -> bool {
        self.bridge.allowed_channels.is_empty()
            || self.bridge.allowed_channels.iter().any(|c| c == channel_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_mode_default() {
        assert_eq!(ResponseMode::default(), ResponseMode::Mention);
    }

    #[test]
    fn test_bridge_config_default() {
        let config = BridgeConfig::default();
        assert!(config.allowed_channels.is_empty());
        assert_eq!(config.response_mode, ResponseMode::Mention);
        assert!(config.typing_indicator);
        assert!(config.thread_replies);
    }
}
