// ABOUTME: Configuration loading and management for coven
// ABOUTME: Supports TOML config files with sensible defaults

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Main configuration structure
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Workspace directory to launch from
    pub workspace: Option<PathBuf>,
    /// Database settings
    pub database: DatabaseConfig,
    /// Claude API settings (for DirectCli backend)
    pub claude: ClaudeConfig,
    /// Mux backend settings (for native Rust backend)
    pub mux: MuxBackendConfig,
    /// Slack frontend settings
    pub slack: SlackConfig,
    /// TUI settings
    pub tui: TuiConfig,
    /// Matrix frontend settings
    pub matrix: MatrixConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    /// Path to SQLite database file
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeConfig {
    /// Path to the claude binary (defaults to "claude")
    pub binary: String,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// System prompt to use (for SDK backend only)
    pub system_prompt: Option<String>,
    /// Base URL for Anthropic API (for SDK backend only)
    pub base_url: Option<String>,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            binary: "claude".to_string(),
            timeout_secs: 300,
            system_prompt: None,
            base_url: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MuxBackendConfig {
    /// Model to use (e.g., "claude-sonnet-4-20250514")
    pub model: String,
    /// Maximum tokens for response
    pub max_tokens: u32,
    /// Path to global system prompt file (e.g., ~/.mux/system.md)
    pub global_system_prompt_path: Option<PathBuf>,
    /// Filenames to look for local system prompts
    pub local_prompt_files: Vec<String>,
}

impl Default for MuxBackendConfig {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 8192,
            global_system_prompt_path: None,
            local_prompt_files: vec![
                "claude.md".to_string(),
                "CLAUDE.md".to_string(),
                "agent.md".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SlackConfig {
    /// Slack bot token (xoxb-...)
    pub bot_token: Option<String>,
    /// Slack app token for socket mode (xapp-...)
    pub app_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TuiConfig {
    /// Default thread name
    pub default_thread: String,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            default_thread: "default".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MatrixConfig {
    /// Matrix homeserver URL
    pub homeserver: Option<String>,
    /// Matrix username (e.g., @bot:matrix.org)
    pub username: Option<String>,
    /// Matrix password
    pub password: Option<String>,
    /// Recovery key for E2E encryption (base58 encoded)
    pub recovery_key: Option<String>,
    /// Allowlist of Matrix user IDs that can interact with the bot
    /// If empty, all users can interact (not recommended for public servers)
    pub allowed_users: Vec<String>,
}

impl Config {
    /// Get the XDG config directory for coven (~/.config/fold)
    pub fn config_dir() -> PathBuf {
        // Respect XDG_CONFIG_HOME if set, otherwise use ~/.config
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .map(|p| p.join(".config"))
                    .unwrap_or_else(|| PathBuf::from("."))
            })
            .join("coven")
    }

    /// Get the XDG data directory for coven (~/.local/share/fold)
    pub fn data_dir() -> PathBuf {
        // Respect XDG_DATA_HOME if set, otherwise use ~/.local/share
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .map(|p| p.join(".local").join("share"))
                    .unwrap_or_else(|| PathBuf::from("."))
            })
            .join("coven")
    }

    /// Get the default config file path
    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Load config from XDG config directory
    pub fn load() -> Result<Self> {
        let path = Self::config_path();

        if path.exists() {
            Self::load_from(&path)
        } else {
            // No config found, use defaults
            Ok(Self::default())
        }
    }

    /// Load config from a specific path
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config from {}", path.display()))?;

        Ok(config)
    }

    /// Get the database path, using default if not configured
    pub fn db_path(&self) -> PathBuf {
        self.database
            .path
            .clone()
            .unwrap_or_else(|| Self::data_dir().join("threads.db"))
    }

    /// Get the workspace directory, defaulting to home if not set
    pub fn workspace_path(&self) -> PathBuf {
        self.workspace
            .clone()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
    }

    /// Generate a default config file content
    pub fn default_toml() -> String {
        let home = dirs::home_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "~".to_string());

        format!(
            r#"# fold configuration
# Location: ~/.config/coven/config.toml

# Workspace directory to launch from
workspace = "{home}"

[database]
# path = "~/.local/share/coven/threads.db"  # Default location

[claude]
timeout_secs = 300
# system_prompt = "You are a helpful assistant."
# base_url = "http://localhost:4000"  # For proxies like LiteLLM

[mux]
# model = "claude-sonnet-4-20250514"
# max_tokens = 8192
# global_system_prompt_path = "~/.mux/system.md"
# local_prompt_files = ["claude.md", "CLAUDE.md", "agent.md"]

[slack]
# bot_token = "xoxb-..."
# app_token = "xapp-..."

[matrix]
# homeserver = "https://matrix.org"
# username = "@bot:matrix.org"
# password = "your-password"
# recovery_key = "EsT..."  # Base58 recovery key for E2E encryption

[tui]
default_thread = "default"
"#
        )
    }

    /// Initialize config directory and create default config if needed
    pub fn init() -> Result<PathBuf> {
        let config_dir = Self::config_dir();
        let config_path = Self::config_path();
        let data_dir = Self::data_dir();

        // Create directories
        std::fs::create_dir_all(&config_dir)
            .with_context(|| format!("Failed to create config dir: {}", config_dir.display()))?;
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("Failed to create data dir: {}", data_dir.display()))?;

        // Write default config if it doesn't exist
        if !config_path.exists() {
            std::fs::write(&config_path, Self::default_toml())
                .with_context(|| format!("Failed to write config: {}", config_path.display()))?;
        }

        Ok(config_path)
    }
}
