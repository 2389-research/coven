// ABOUTME: Configuration file handling.
// ABOUTME: TOML config with env var and .env support.

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Config {
    #[serde(default)]
    pub gateway: GatewayConfig,

    #[serde(default)]
    pub appearance: AppearanceConfig,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GatewayConfig {
    #[serde(default = "default_gateway_host")]
    pub host: String,
    #[serde(default = "default_gateway_port")]
    pub port: u16,
    #[serde(default)]
    pub use_tls: bool,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: default_gateway_host(),
            port: default_gateway_port(),
            use_tls: false,
        }
    }
}

impl GatewayConfig {
    /// Compose the full gRPC gateway URL from host and port.
    pub fn url(&self) -> String {
        let scheme = if self.use_tls { "https" } else { "http" };
        format!("{}://{}:{}", scheme, self.host, self.port)
    }
}

fn default_gateway_host() -> String {
    "localhost".to_string()
}

fn default_gateway_port() -> u16 {
    50051
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AppearanceConfig {
    #[serde(default = "default_theme")]
    pub theme: String,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
        }
    }
}

fn default_theme() -> String {
    "default".to_string()
}

impl Config {
    /// Load config with precedence: CLI > env > .env > file > defaults
    pub fn load(gateway_override: Option<&str>, theme_override: Option<&str>) -> Result<Self> {
        // Load .env file if present (silently ignore if missing)
        let _ = dotenvy::dotenv();

        // Start with file config or defaults
        let mut config = Self::load_from_file().unwrap_or_default();

        // Apply env var overrides
        if let Ok(host) = std::env::var("COVEN_GATEWAY_HOST") {
            config.gateway.host = host;
        }
        if let Ok(port) = std::env::var("COVEN_GATEWAY_PORT") {
            if let Ok(p) = port.parse::<u16>() {
                config.gateway.port = p;
            }
        }
        // Support COVEN_GATEWAY_TLS env var
        if let Ok(tls) = std::env::var("COVEN_GATEWAY_TLS") {
            config.gateway.use_tls = tls == "1" || tls.eq_ignore_ascii_case("true");
        }
        // Support legacy COVEN_GATEWAY_URL for backwards compatibility
        if let Ok(url) = std::env::var("COVEN_GATEWAY_URL") {
            if let Some((host, port, use_tls)) = Self::parse_gateway_url(&url) {
                config.gateway.host = host;
                config.gateway.port = port;
                config.gateway.use_tls = use_tls;
            }
        }
        if let Ok(theme) = std::env::var("COVEN_THEME") {
            config.appearance.theme = theme;
        }

        // Apply CLI overrides (highest priority) - parses URL format
        if let Some(url) = gateway_override {
            if let Some((host, port, use_tls)) = Self::parse_gateway_url(url) {
                config.gateway.host = host;
                config.gateway.port = port;
                config.gateway.use_tls = use_tls;
            }
        }
        if let Some(theme) = theme_override {
            config.appearance.theme = theme.to_string();
        }

        Ok(config)
    }

    /// Parse a gateway URL like "http://localhost:5000" into (host, port, use_tls).
    fn parse_gateway_url(url: &str) -> Option<(String, u16, bool)> {
        // Check for https:// prefix (TLS)
        let use_tls = url.starts_with("https://");

        // Strip http:// or https:// prefix
        let without_scheme = url
            .strip_prefix("http://")
            .or_else(|| url.strip_prefix("https://"))
            .unwrap_or(url);

        // Split host:port
        if let Some((host, port_str)) = without_scheme.rsplit_once(':') {
            if let Ok(port) = port_str.parse::<u16>() {
                return Some((host.to_string(), port, use_tls));
            }
        }

        // No port specified, use host with default port
        if !without_scheme.is_empty() && !without_scheme.contains(':') {
            return Some((without_scheme.to_string(), default_gateway_port(), use_tls));
        }

        None
    }

    fn load_from_file() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| AppError::Config(format!("Failed to read config: {}", e)))?;

        toml::from_str(&content)
            .map_err(|e| AppError::Config(format!("Failed to parse config: {}", e)))
    }

    #[allow(dead_code)]
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Config(format!("Failed to create config dir: {}", e)))?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| AppError::Config(format!("Failed to serialize config: {}", e)))?;

        std::fs::write(&path, content)
            .map_err(|e| AppError::Config(format!("Failed to write config: {}", e)))?;

        Ok(())
    }

    pub fn config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| AppError::Config("Could not find config directory".to_string()))?;
        Ok(config_dir.join("coven").join("tui-config.toml"))
    }

    pub fn state_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| AppError::Config("Could not find config directory".to_string()))?;
        Ok(config_dir.join("coven").join("tui-state.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;

    #[test]
    fn test_default_config_toml_snapshot() {
        let config = Config::default();
        let toml = toml::to_string_pretty(&config).expect("Failed to serialize config");
        assert_snapshot!(toml);
    }

    #[test]
    fn test_config_debug_snapshot() {
        let config = Config::default();
        assert_snapshot!(format!("{:?}", config));
    }

    #[test]
    fn test_gateway_config_defaults() {
        let config = GatewayConfig::default();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 50051);
        assert!(!config.use_tls);
        assert_eq!(config.url(), "http://localhost:50051");
    }

    #[test]
    fn test_gateway_config_with_tls() {
        let config = GatewayConfig {
            host: "example.com".to_string(),
            port: 443,
            use_tls: true,
        };
        assert_eq!(config.url(), "https://example.com:443");
    }

    #[test]
    fn test_parse_gateway_url_with_scheme() {
        let result = Config::parse_gateway_url("http://example.com:8080");
        assert_eq!(result, Some(("example.com".to_string(), 8080, false)));
    }

    #[test]
    fn test_parse_gateway_url_https() {
        let result = Config::parse_gateway_url("https://example.com:443");
        assert_eq!(result, Some(("example.com".to_string(), 443, true)));
    }

    #[test]
    fn test_parse_gateway_url_without_scheme() {
        let result = Config::parse_gateway_url("example.com:8080");
        assert_eq!(result, Some(("example.com".to_string(), 8080, false)));
    }

    #[test]
    fn test_parse_gateway_url_without_port() {
        let result = Config::parse_gateway_url("http://example.com");
        assert_eq!(result, Some(("example.com".to_string(), 50051, false)));
    }

    #[test]
    fn test_parse_gateway_url_localhost() {
        let result = Config::parse_gateway_url("http://localhost:5000");
        assert_eq!(result, Some(("localhost".to_string(), 5000, false)));
    }

    #[test]
    fn test_appearance_config_defaults() {
        let config = AppearanceConfig::default();
        assert_eq!(config.theme, "default");
    }
}
