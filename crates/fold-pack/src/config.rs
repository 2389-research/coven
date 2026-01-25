// ABOUTME: Configuration loading for fold-pack SDK with file, env, and default precedence.
// ABOUTME: Resolves gateway URL from env vars, .env files, and ~/.config/fold/packs.toml.

use serde::Deserialize;
use std::path::PathBuf;

use crate::PackError;

/// Raw TOML structure for ~/.config/fold/packs.toml
#[derive(Deserialize, Default)]
struct PacksToml {
    server: Option<String>,
    port: Option<u16>,
}

/// Resolved pack configuration.
pub struct PackConfig {
    /// Constructed gateway URL: http://{server}:{port}
    pub gateway_url: String,
    /// Path to SSH key: ~/.config/fold/packs/{pack_name}/id_ed25519
    pub ssh_key_path: PathBuf,
}

impl PackConfig {
    /// Load config with precedence: env > .env > packs.toml > defaults
    ///
    /// Gateway URL resolution order:
    /// 1. `FOLD_GATEWAY_URL` (full URL, e.g., "https://gw.example.com:50051")
    /// 2. `GATEWAY_ADDR` (legacy compat, treated as full URL)
    /// 3. Constructed from `FOLD_SERVER`/`FOLD_PORT` + packs.toml + defaults
    ///
    /// SSH key path resolution order:
    /// 1. `FOLD_SSH_KEY_PATH` (explicit path)
    /// 2. `PACK_SSH_KEY` (legacy compat)
    /// 3. Default: ~/.config/fold/packs/{pack_name}/id_ed25519
    pub fn load(pack_name: &str) -> Result<Self, PackError> {
        // Load .env from cwd (adds to env vars, so env lookups below catch both)
        let _ = dotenvy::dotenv();

        // Resolve gateway URL: full URL env vars take priority over server/port construction
        let gateway_url = std::env::var("FOLD_GATEWAY_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("GATEWAY_ADDR").ok().filter(|s| !s.is_empty()))
            .unwrap_or_else(|| {
                // Load packs.toml for server/port fallback
                let toml_config = load_packs_toml();

                let server = std::env::var("FOLD_SERVER")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .or(toml_config.server)
                    .unwrap_or_else(|| "localhost".to_string());

                let port = std::env::var("FOLD_PORT")
                    .ok()
                    .and_then(|p| p.parse::<u16>().ok())
                    .or(toml_config.port)
                    .unwrap_or(50051);

                format!("http://{}:{}", server, port)
            });

        // Resolve SSH key path: explicit env vars take priority over default
        let ssh_key_path = std::env::var("FOLD_SSH_KEY_PATH")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("PACK_SSH_KEY").ok().filter(|s| !s.is_empty()))
            .map(PathBuf::from)
            .or_else(|| default_pack_key_path(pack_name))
            .ok_or_else(|| {
                PackError::ConfigError("could not determine config directory".to_string())
            })?;

        Ok(Self {
            gateway_url,
            ssh_key_path,
        })
    }
}

/// Load ~/.config/fold/packs.toml, returning defaults if file doesn't exist or can't be parsed.
fn load_packs_toml() -> PacksToml {
    let Some(config_dir) = config_dir() else {
        return PacksToml::default();
    };
    let path = config_dir.join("packs.toml");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return PacksToml::default();
    };
    toml::from_str(&content).unwrap_or_default()
}

/// Get ~/.config/fold directory (XDG convention, consistent with fold-ssh).
/// Ignores empty or non-absolute XDG_CONFIG_HOME values to avoid surprising paths.
fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .map(|p| p.join("fold"))
}

/// Get default SSH key path for a pack: ~/.config/fold/packs/{pack_name}/id_ed25519
fn default_pack_key_path(pack_name: &str) -> Option<PathBuf> {
    config_dir().map(|p| p.join("packs").join(pack_name).join("id_ed25519"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Mutex to serialize tests that modify environment variables, preventing races.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    /// Helper to run a closure with specific env vars set, cleaning up afterward.
    fn with_env_vars<F, R>(vars: &[(&str, &str)], f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = ENV_MUTEX.lock().unwrap();

        // Save original values
        let originals: Vec<(&str, Option<String>)> = vars
            .iter()
            .map(|(key, _)| (*key, std::env::var(key).ok()))
            .collect();

        // Set new values
        for (key, val) in vars {
            std::env::set_var(key, val);
        }

        let result = f();

        // Restore original values
        for (key, original) in &originals {
            match original {
                Some(val) => std::env::set_var(key, val),
                None => std::env::remove_var(key),
            }
        }

        result
    }

    /// Helper to run a closure with env vars removed, restoring afterward.
    fn without_env_vars<F, R>(keys: &[&str], f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = ENV_MUTEX.lock().unwrap();

        // Save and remove
        let originals: Vec<(&str, Option<String>)> = keys
            .iter()
            .map(|key| (*key, std::env::var(key).ok()))
            .collect();
        for key in keys {
            std::env::remove_var(key);
        }

        let result = f();

        // Restore
        for (key, original) in &originals {
            match original {
                Some(val) => std::env::set_var(key, val),
                None => std::env::remove_var(key),
            }
        }

        result
    }

    #[test]
    fn test_load_packs_toml_returns_defaults_when_file_missing() {
        // load_packs_toml should gracefully return defaults when no file exists.
        // On most CI/test systems ~/.config/fold/packs.toml won't exist.
        let config = load_packs_toml();
        assert!(config.server.is_none());
        assert!(config.port.is_none());
    }

    #[test]
    fn test_default_pack_key_path_constructs_expected_path() {
        let path = default_pack_key_path("my-pack");
        assert!(path.is_some());
        let path = path.unwrap();

        // Should end with packs/my-pack/id_ed25519
        assert!(path.ends_with("fold/packs/my-pack/id_ed25519"));

        // Should contain the config directory prefix
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains("fold/packs/my-pack/id_ed25519"),
            "Path was: {}",
            path_str
        );
    }

    #[test]
    fn test_default_pack_key_path_different_pack_names() {
        let path_a = default_pack_key_path("alpha-pack").unwrap();
        let path_b = default_pack_key_path("beta-pack").unwrap();

        assert!(path_a.ends_with("fold/packs/alpha-pack/id_ed25519"));
        assert!(path_b.ends_with("fold/packs/beta-pack/id_ed25519"));
        assert_ne!(path_a, path_b);
    }

    #[test]
    fn test_config_dir_returns_fold_subdir() {
        let dir = config_dir();
        assert!(dir.is_some());
        let dir = dir.unwrap();
        assert!(
            dir.ends_with("fold"),
            "Expected path ending in 'fold', got: {:?}",
            dir
        );
    }

    #[test]
    fn test_pack_config_load_with_defaults() {
        // With no env vars and no config file, should fall back to defaults
        without_env_vars(&["FOLD_SERVER", "FOLD_PORT"], || {
            let config = PackConfig::load("test-pack").unwrap();
            assert_eq!(config.gateway_url, "http://localhost:50051");
            assert!(config
                .ssh_key_path
                .ends_with("fold/packs/test-pack/id_ed25519"));
        });
    }

    #[test]
    fn test_pack_config_load_env_server_override() {
        with_env_vars(&[("FOLD_SERVER", "custom.example.com")], || {
            // Remove FOLD_PORT to test server-only override
            std::env::remove_var("FOLD_PORT");
            let config = PackConfig::load("test-pack").unwrap();
            assert_eq!(config.gateway_url, "http://custom.example.com:50051");
        });
    }

    #[test]
    fn test_pack_config_load_env_port_override() {
        with_env_vars(&[("FOLD_PORT", "9999")], || {
            std::env::remove_var("FOLD_SERVER");
            let config = PackConfig::load("test-pack").unwrap();
            assert_eq!(config.gateway_url, "http://localhost:9999");
        });
    }

    #[test]
    fn test_pack_config_load_both_env_overrides() {
        with_env_vars(
            &[("FOLD_SERVER", "prod.example.com"), ("FOLD_PORT", "8080")],
            || {
                let config = PackConfig::load("test-pack").unwrap();
                assert_eq!(config.gateway_url, "http://prod.example.com:8080");
            },
        );
    }

    #[test]
    fn test_pack_config_load_invalid_port_uses_default() {
        with_env_vars(&[("FOLD_PORT", "not-a-number")], || {
            std::env::remove_var("FOLD_SERVER");
            let config = PackConfig::load("test-pack").unwrap();
            // Invalid port parse falls through to default
            assert_eq!(config.gateway_url, "http://localhost:50051");
        });
    }

    #[test]
    fn test_pack_config_load_empty_port_uses_default() {
        with_env_vars(&[("FOLD_PORT", "")], || {
            std::env::remove_var("FOLD_SERVER");
            let config = PackConfig::load("test-pack").unwrap();
            assert_eq!(config.gateway_url, "http://localhost:50051");
        });
    }

    #[test]
    fn test_pack_config_ssh_key_path_varies_by_pack_name() {
        without_env_vars(&["FOLD_SERVER", "FOLD_PORT"], || {
            let config_a = PackConfig::load("pack-alpha").unwrap();
            let config_b = PackConfig::load("pack-beta").unwrap();

            assert!(config_a
                .ssh_key_path
                .ends_with("fold/packs/pack-alpha/id_ed25519"));
            assert!(config_b
                .ssh_key_path
                .ends_with("fold/packs/pack-beta/id_ed25519"));
            assert_ne!(config_a.ssh_key_path, config_b.ssh_key_path);
        });
    }

    #[test]
    fn test_packs_toml_deserialization() {
        let toml_str = r#"
            server = "gateway.example.com"
            port = 4321
        "#;
        let config: PacksToml = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.as_deref(), Some("gateway.example.com"));
        assert_eq!(config.port, Some(4321));
    }

    #[test]
    fn test_packs_toml_partial_deserialization() {
        // Only server specified
        let toml_str = r#"server = "only-server.com""#;
        let config: PacksToml = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.as_deref(), Some("only-server.com"));
        assert_eq!(config.port, None);

        // Only port specified
        let toml_str = r#"port = 7777"#;
        let config: PacksToml = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server, None);
        assert_eq!(config.port, Some(7777));
    }

    #[test]
    fn test_packs_toml_empty_deserialization() {
        let toml_str = "";
        let config: PacksToml = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server, None);
        assert_eq!(config.port, None);
    }

    #[test]
    fn test_packs_toml_invalid_returns_default() {
        let toml_str = "this is not valid toml {{{";
        let config: PacksToml = toml::from_str(toml_str).unwrap_or_default();
        assert_eq!(config.server, None);
        assert_eq!(config.port, None);
    }

    #[test]
    fn test_pack_config_load_gateway_url_env_override() {
        with_env_vars(&[("FOLD_GATEWAY_URL", "https://gw.example.com:443")], || {
            std::env::remove_var("GATEWAY_ADDR");
            std::env::remove_var("FOLD_SERVER");
            std::env::remove_var("FOLD_PORT");
            let config = PackConfig::load("test-pack").unwrap();
            assert_eq!(config.gateway_url, "https://gw.example.com:443");
        });
    }

    #[test]
    fn test_pack_config_load_legacy_gateway_addr() {
        without_env_vars(&["FOLD_GATEWAY_URL", "FOLD_SERVER", "FOLD_PORT"], || {
            std::env::set_var("GATEWAY_ADDR", "http://legacy-host:9090");
            let config = PackConfig::load("test-pack").unwrap();
            std::env::remove_var("GATEWAY_ADDR");
            assert_eq!(config.gateway_url, "http://legacy-host:9090");
        });
    }

    #[test]
    fn test_pack_config_gateway_url_takes_priority_over_legacy() {
        with_env_vars(
            &[
                ("FOLD_GATEWAY_URL", "https://new-gw:443"),
                ("GATEWAY_ADDR", "http://old-gw:8080"),
            ],
            || {
                let config = PackConfig::load("test-pack").unwrap();
                assert_eq!(config.gateway_url, "https://new-gw:443");
            },
        );
    }

    #[test]
    fn test_pack_config_ssh_key_path_env_override() {
        with_env_vars(&[("FOLD_SSH_KEY_PATH", "/custom/key/path")], || {
            std::env::remove_var("PACK_SSH_KEY");
            let config = PackConfig::load("test-pack").unwrap();
            assert_eq!(config.ssh_key_path, PathBuf::from("/custom/key/path"));
        });
    }

    #[test]
    fn test_pack_config_legacy_ssh_key_env() {
        without_env_vars(&["FOLD_SSH_KEY_PATH"], || {
            std::env::set_var("PACK_SSH_KEY", "/legacy/ssh/key");
            let config = PackConfig::load("test-pack").unwrap();
            std::env::remove_var("PACK_SSH_KEY");
            assert_eq!(config.ssh_key_path, PathBuf::from("/legacy/ssh/key"));
        });
    }

    #[test]
    fn test_config_dir_ignores_empty_xdg() {
        with_env_vars(&[("XDG_CONFIG_HOME", "")], || {
            let dir = config_dir();
            // Should fall back to ~/.config/fold, not use empty string
            if let Some(d) = dir {
                assert!(d.is_absolute(), "Expected absolute path, got {:?}", d);
            }
        });
    }

    #[test]
    fn test_config_dir_ignores_relative_xdg() {
        with_env_vars(&[("XDG_CONFIG_HOME", "relative/path")], || {
            let dir = config_dir();
            // Should fall back to ~/.config/fold, not use relative path
            if let Some(d) = dir {
                assert!(d.is_absolute(), "Expected absolute path, got {:?}", d);
            }
        });
    }
}
