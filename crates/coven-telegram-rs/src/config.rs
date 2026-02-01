// ABOUTME: Configuration loading and validation for the Telegram bridge.
// ABOUTME: Supports TOML config files with environment variable expansion.

use crate::error::{BridgeError, Result};
use serde::Deserialize;
use std::path::PathBuf;
use tracing::warn;

/// Top-level configuration structure for coven-telegram-rs.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub telegram: TelegramConfig,
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub bridge: BridgeConfig,
}

/// Telegram bot credentials for Long Polling connection.
#[derive(Clone, Deserialize)]
pub struct TelegramConfig {
    /// Bot token from @BotFather (e.g., "123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11").
    pub bot_token: String,
}

impl std::fmt::Debug for TelegramConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelegramConfig")
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
    /// List of allowed chat IDs (empty = allow all chats the bot is in).
    #[serde(default)]
    pub allowed_chats: Vec<i64>,

    /// Response trigger mode: when to respond to messages.
    #[serde(default)]
    pub response_mode: ResponseMode,

    /// Reply in threads using Telegram's reply-to feature.
    #[serde(default = "default_thread_replies")]
    pub thread_replies: bool,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            allowed_chats: Vec::new(),
            response_mode: ResponseMode::default(),
            thread_replies: default_thread_replies(),
        }
    }
}

fn default_thread_replies() -> bool {
    true
}

/// Response mode determines when the bot responds to messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResponseMode {
    /// Only respond when @mentioned, in private chats, or in reply threads.
    #[default]
    Mention,
    /// Respond to every message in allowed chats.
    All,
}

impl Config {
    /// Load configuration from the specified path or default location.
    ///
    /// Default location: `~/.config/coven/telegram-bridge.toml`
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let path = path
            .or_else(|| dirs::config_dir().map(|d| d.join("coven").join("telegram-bridge.toml")))
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
        if self.telegram.bot_token.is_empty() {
            return Err(BridgeError::Config("telegram.bot_token is required".into()));
        }
        // Telegram bot tokens have format: <bot_id>:<token_string>
        if !self.telegram.bot_token.contains(':') {
            return Err(BridgeError::Config(
                "telegram.bot_token must contain ':' (format: BOT_ID:TOKEN_STRING)".into(),
            ));
        }
        if self.gateway.url.is_empty() {
            return Err(BridgeError::Config("gateway.url is required".into()));
        }
        Ok(())
    }

    /// Check if a chat is in the allowed list.
    /// Returns true if allowed_chats is empty (allow all) or chat is in list.
    pub fn is_chat_allowed(&self, chat_id: i64) -> bool {
        self.bridge.allowed_chats.is_empty() || self.bridge.allowed_chats.contains(&chat_id)
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
        assert!(config.allowed_chats.is_empty());
        assert_eq!(config.response_mode, ResponseMode::Mention);
        assert!(config.thread_replies);
    }

    #[test]
    fn test_is_chat_allowed_empty_allows_all() {
        let config = Config {
            telegram: TelegramConfig {
                bot_token: "123456:ABC".to_string(),
            },
            gateway: GatewayConfig {
                url: "http://localhost:6666".to_string(),
                token: None,
            },
            bridge: BridgeConfig::default(),
        };

        assert!(config.is_chat_allowed(12345));
        assert!(config.is_chat_allowed(-67890));
    }

    #[test]
    fn test_is_chat_allowed_specific_chats() {
        let config = Config {
            telegram: TelegramConfig {
                bot_token: "123456:ABC".to_string(),
            },
            gateway: GatewayConfig {
                url: "http://localhost:6666".to_string(),
                token: None,
            },
            bridge: BridgeConfig {
                allowed_chats: vec![12345, -67890],
                response_mode: ResponseMode::Mention,
                thread_replies: true,
            },
        };

        assert!(config.is_chat_allowed(12345));
        assert!(config.is_chat_allowed(-67890));
        assert!(!config.is_chat_allowed(99999));
    }

    #[test]
    fn test_config_validates_bot_token_format() {
        let config = Config {
            telegram: TelegramConfig {
                bot_token: "invalid_token_no_colon".to_string(),
            },
            gateway: GatewayConfig {
                url: "http://localhost:6666".to_string(),
                token: None,
            },
            bridge: BridgeConfig::default(),
        };

        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(":"));
    }

    #[test]
    fn test_config_validates_empty_bot_token() {
        let config = Config {
            telegram: TelegramConfig {
                bot_token: String::new(),
            },
            gateway: GatewayConfig {
                url: "http://localhost:6666".to_string(),
                token: None,
            },
            bridge: BridgeConfig::default(),
        };

        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("bot_token is required"));
    }

    #[test]
    fn test_config_validates_empty_gateway_url() {
        let config = Config {
            telegram: TelegramConfig {
                bot_token: "123456:ABC".to_string(),
            },
            gateway: GatewayConfig {
                url: String::new(),
                token: None,
            },
            bridge: BridgeConfig::default(),
        };

        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("url"));
    }

    #[test]
    fn test_config_valid() {
        let config = Config {
            telegram: TelegramConfig {
                bot_token: "123456:ABC-DEF".to_string(),
            },
            gateway: GatewayConfig {
                url: "http://localhost:6666".to_string(),
                token: Some("test-token".to_string()),
            },
            bridge: BridgeConfig::default(),
        };

        assert!(config.validate().is_ok());
    }
}
