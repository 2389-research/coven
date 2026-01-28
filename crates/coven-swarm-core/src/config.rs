// ABOUTME: Configuration for coven-swarm supervisor and agents.
// ABOUTME: Loaded from TOML file with sensible defaults.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackendType {
    #[default]
    Acp,
    Mux,
    Direct,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Gateway gRPC URL (e.g., "http://coven.example.com:50051")
    /// If not set, falls back to gateway from ~/.config/coven/config.toml (from `coven link`)
    #[serde(default)]
    pub gateway_url: Option<String>,

    /// Agent name prefix (e.g., "home" -> "home_research")
    pub prefix: String,

    /// Directory containing workspaces
    pub working_directory: String,

    /// Default backend for agents
    #[serde(default)]
    pub default_backend: BackendType,

    /// ACP binary path (for acp backend)
    #[serde(default = "default_acp_binary")]
    pub acp_binary: String,
}

fn default_acp_binary() -> String {
    "claude".to_string()
}

impl Config {
    /// Load config from a TOML file
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config from {}", path.display()))?;
        Ok(config)
    }

    /// Save config to a TOML file
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory {}", parent.display())
            })?;
        }
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;
        Ok(())
    }

    /// Get the default config file path (~/.config/coven/swarm.toml)
    pub fn default_path() -> Result<PathBuf> {
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .map(|h| h.join(".config"))
                    .unwrap_or_else(|| PathBuf::from("."))
            })
            .join("coven");
        Ok(config_dir.join("swarm.toml"))
    }

    /// Expand ~ in working directory path
    pub fn working_directory_expanded(&self) -> PathBuf {
        shellexpand::tilde(&self.working_directory)
            .into_owned()
            .into()
    }

    /// Get the gateway URL, falling back to coven link config if not set
    pub fn gateway_url(&self) -> Result<String> {
        if let Some(ref url) = self.gateway_url {
            return Ok(url.clone());
        }

        // Fall back to coven link config
        let coven_config = coven_link::config::CovenConfig::load()
            .context("gateway_url not set in swarm.toml and no coven link config found. Run 'coven link' first or set gateway_url in swarm.toml")?;

        // Convert "host:port" format to URL
        let gateway = &coven_config.gateway;
        if gateway.starts_with("http://") || gateway.starts_with("https://") {
            Ok(gateway.clone())
        } else {
            Ok(format!("http://{}", gateway))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_config_from_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
            gateway_url = "http://localhost:50051"
            prefix = "home"
            working_directory = "~/workspaces"
            default_backend = "acp"
        "#
        )
        .unwrap();

        let config = Config::load(file.path()).unwrap();
        assert_eq!(config.gateway_url, Some("http://localhost:50051".to_string()));
        assert_eq!(config.prefix, "home");
        assert_eq!(config.default_backend, BackendType::Acp);
    }

    #[test]
    fn test_save_and_load_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let config = Config {
            gateway_url: Some("http://example.com:50051".to_string()),
            prefix: "test".to_string(),
            working_directory: "~/test-workspaces".to_string(),
            default_backend: BackendType::Mux,
            acp_binary: "claude".to_string(),
        };

        config.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();

        assert_eq!(loaded.gateway_url, config.gateway_url);
        assert_eq!(loaded.prefix, config.prefix);
        assert_eq!(loaded.default_backend, BackendType::Mux);
    }

    #[test]
    fn test_path_expansion() {
        let config = Config {
            gateway_url: Some("http://localhost:50051".to_string()),
            prefix: "home".to_string(),
            working_directory: "~/workspaces".to_string(),
            default_backend: BackendType::Acp,
            acp_binary: "claude".to_string(),
        };

        let expanded_wd = config.working_directory_expanded();

        // Should not contain ~ after expansion
        assert!(!expanded_wd.to_string_lossy().contains('~'));

        // Should start with home directory
        let home = std::env::var("HOME").unwrap();
        assert!(expanded_wd.to_string_lossy().starts_with(&home));
    }

    #[test]
    fn test_default_backend() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
            gateway_url = "http://localhost:50051"
            prefix = "home"
            working_directory = "~/workspaces"
        "#
        )
        .unwrap();

        let config = Config::load(file.path()).unwrap();
        assert_eq!(config.default_backend, BackendType::Acp);
        assert_eq!(config.acp_binary, "claude");
    }

    #[test]
    fn test_gateway_url_fallback_to_explicit() {
        let config = Config {
            gateway_url: Some("http://explicit.example.com:50051".to_string()),
            prefix: "test".to_string(),
            working_directory: "~/workspaces".to_string(),
            default_backend: BackendType::Acp,
            acp_binary: "claude".to_string(),
        };

        // Should return the explicit URL
        assert_eq!(config.gateway_url().unwrap(), "http://explicit.example.com:50051");
    }

    #[test]
    fn test_backend_type_default() {
        assert_eq!(BackendType::default(), BackendType::Acp);
    }
}
