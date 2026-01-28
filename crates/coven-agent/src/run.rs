// ABOUTME: Public entry points for running coven-agent from external crates.
// ABOUTME: Exposes run_agent and run_wizard for use by coven-cli.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

/// Configuration options for running an agent.
#[derive(Debug, Clone)]
pub struct AgentRunConfig {
    /// Control server address (e.g., "http://127.0.0.1:50051")
    pub server: String,
    /// Agent name
    pub name: String,
    /// Agent ID (auto-generated if None)
    pub id: Option<String>,
    /// Backend type: "mux" or "cli"
    pub backend: Option<String>,
    /// Working directory for the agent
    pub working_dir: Option<PathBuf>,
    /// Configuration file path
    pub config: Option<PathBuf>,
    /// Run in headless mode (minimal output)
    pub headless: bool,
    /// Run in single-user interactive mode (no gRPC server)
    pub single: bool,
}

impl Default for AgentRunConfig {
    fn default() -> Self {
        Self {
            server: "http://127.0.0.1:50051".to_string(),
            name: "agent-1".to_string(),
            id: None,
            backend: None,
            working_dir: None,
            config: None,
            headless: false,
            single: false,
        }
    }
}

/// Display mode for agent output
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisplayMode {
    /// Visual ratatui TUI (default)
    Tui,
    /// Minimal server output (--headless flag)
    Headless,
}

impl DisplayMode {
    fn from_headless_flag(headless: bool) -> Self {
        if headless {
            DisplayMode::Headless
        } else {
            DisplayMode::Tui
        }
    }
}

/// Get XDG-style config directory (~/.config/coven)
/// Respects XDG_CONFIG_HOME if set, otherwise uses ~/.config
fn xdg_config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .map(|p| p.join("coven"))
}

/// Get default config path (~/.config/coven/agent.toml)
fn default_config_path() -> Option<PathBuf> {
    xdg_config_dir().map(|p| p.join("agent.toml"))
}

/// Resolve project name from .coven/project.toml or directory basename
fn resolve_project_name(working_dir: &std::path::Path) -> String {
    // Try to load .coven/project.toml
    let project_config_path = working_dir.join(".coven").join("project.toml");
    if let Ok(content) = std::fs::read_to_string(&project_config_path) {
        if let Ok(config) = content.parse::<toml::Table>() {
            if let Some(name) = config.get("project_name").and_then(|v| v.as_str()) {
                let trimmed = name.trim();
                if !trimmed.is_empty() && trimmed.len() <= 64 {
                    let sanitized = sanitize_project_name(trimmed);
                    if !sanitized.is_empty() {
                        return sanitized;
                    }
                }
            }
        }
    }

    // Fall back to directory basename
    working_dir
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| {
            let sanitized = sanitize_project_name(n);
            if sanitized.is_empty() {
                None
            } else {
                Some(sanitized)
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Sanitize project name for use in agent ID
fn sanitize_project_name(name: &str) -> String {
    let result: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();

    let result = result.trim_matches('-');
    let mut collapsed = String::with_capacity(result.len());
    let mut prev_hyphen = false;
    for c in result.chars() {
        if c == '-' {
            if !prev_hyphen {
                collapsed.push(c);
            }
            prev_hyphen = true;
        } else {
            collapsed.push(c);
            prev_hyphen = false;
        }
    }
    collapsed
}

/// Run an agent with the given configuration.
///
/// This is the main entry point for running a coven agent. It handles:
/// - Loading configuration from file or defaults
/// - Connecting to the gateway server
/// - Running in TUI, headless, or single mode
pub async fn run_agent(config: AgentRunConfig) -> Result<()> {
    let mode = DisplayMode::from_headless_flag(config.headless);

    // Try to load config: explicit path > project-local > user-global
    let config_path = config.config.or_else(|| {
        // Check for project-local config first (.coven/agent.toml in cwd)
        if let Ok(cwd) = std::env::current_dir() {
            let project_config = cwd.join(".coven").join("agent.toml");
            if project_config.exists() {
                return Some(project_config);
            }
        }
        // Fall back to user-global config (~/.config/coven/agent.toml)
        let default = default_config_path()?;
        if default.exists() {
            Some(default)
        } else {
            None
        }
    });

    // Load settings from config - required unless running in single mode
    let (server, name, backend, working_dir, workspaces) = if let Some(ref config_path) = config_path
    {
        tracing::info!("Loading config from: {}", config_path.display());
        let config_content = std::fs::read_to_string(config_path)
            .with_context(|| format!("failed to read config file: {}", config_path.display()))?;
        let loaded_config: toml::Table = toml::from_str(&config_content)
            .with_context(|| format!("failed to parse config file: {}", config_path.display()))?;

        // Check if this is a reference to a global agent (agent = "name")
        let loaded_config = if let Some(agent_ref) = loaded_config.get("agent").and_then(|v| v.as_str()) {
            let agents_dir = xdg_config_dir()
                .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?
                .join("agents");
            let agent_config_path = agents_dir.join(format!("{}.toml", agent_ref));
            tracing::info!(
                "Resolving agent reference '{}' -> {}",
                agent_ref,
                agent_config_path.display()
            );
            let agent_content = std::fs::read_to_string(&agent_config_path).with_context(|| {
                format!(
                    "failed to read agent config '{}' at {}",
                    agent_ref,
                    agent_config_path.display()
                )
            })?;
            toml::from_str(&agent_content).with_context(|| {
                format!(
                    "failed to parse agent config '{}' at {}",
                    agent_ref,
                    agent_config_path.display()
                )
            })?
        } else {
            loaded_config
        };

        // Server can come from:
        // 1. Agent config file (server = "...")
        // 2. User's coven config from `coven link` (~/.config/coven/config.toml)
        // 3. CLI args / defaults
        let server = loaded_config
            .get("server")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                // Try to load gateway from user's coven config
                coven_link::config::CovenConfig::load().ok().map(|c| {
                    // Gateway is "host:port" format, convert to URL
                    if c.gateway.starts_with("http://") || c.gateway.starts_with("https://") {
                        c.gateway
                    } else {
                        format!("http://{}", c.gateway)
                    }
                })
            })
            .unwrap_or(config.server);
        let name = loaded_config
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or(config.name);
        let backend = loaded_config
            .get("backend")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or(config.backend);
        let config_working_dir = loaded_config
            .get("working_dir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);

        let workspaces: Vec<String> = loaded_config
            .get("workspaces")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        (
            server,
            name,
            backend,
            config.working_dir.or(config_working_dir),
            workspaces,
        )
    } else if !config.single {
        // Config is required for gateway mode
        bail!(
            "No configuration found. Create one with 'coven agent new' or specify --config.\n\
             Searched:\n\
             - .coven/agent.toml (project-local)\n\
             - ~/.config/coven/agent.toml (user-global)"
        );
    } else {
        // Single mode can work without config
        (config.server, config.name, config.backend, config.working_dir, Vec::new())
    };

    // Default to current directory if not specified
    let working_dir = working_dir
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    // Canonicalize the path for consistent identity
    let working_dir = working_dir.canonicalize().unwrap_or(working_dir);

    // Default to "cli" which uses Claude CLI and doesn't require ANTHROPIC_API_KEY
    let backend_type = backend.unwrap_or_else(|| "cli".to_string());

    // Create agent ID from name + project name
    let agent_id = config.id.unwrap_or_else(|| {
        let project_name = resolve_project_name(&working_dir);
        format!("{}-{}", name, project_name)
    });

    if config.single {
        return crate::single::run(&name, &agent_id, &backend_type, &working_dir).await;
    }

    match mode {
        DisplayMode::Tui => crate::tui::run(&server, &agent_id, &backend_type, &working_dir).await,
        DisplayMode::Headless => {
            // Gather metadata for registration
            let mut metadata = crate::metadata::AgentMetadata::gather(&working_dir);
            metadata.workspaces = workspaces;
            metadata.backend = backend_type.to_string();

            // Log metadata for debugging
            eprintln!("Agent metadata:");
            eprintln!("  Working dir: {}", metadata.working_directory);
            eprintln!("  Hostname: {}", metadata.hostname);
            eprintln!("  OS: {}", metadata.os);
            eprintln!("  Backend: {}", metadata.backend);
            if !metadata.workspaces.is_empty() {
                eprintln!("  Workspaces: {:?}", metadata.workspaces);
            }
            if let Some(ref git) = metadata.git {
                eprintln!(
                    "  Git: {} @ {} (dirty={})",
                    git.branch, git.commit, git.dirty
                );
                if !git.remote.is_empty() {
                    eprintln!("  Remote: {} (+{}, -{})", git.remote, git.ahead, git.behind);
                }
            }

            crate::client::run(
                &server,
                &agent_id,
                &backend_type,
                &working_dir,
                false,
                metadata,
            )
            .await
        }
    }
}

/// Run the interactive agent configuration wizard.
///
/// This guides the user through creating a new agent configuration,
/// including name, backend selection, and server settings.
///
/// The `command_prefix` is used in the success message to show how to run the agent.
/// For standalone binary: "coven-agent"
/// For unified CLI: "coven agent"
pub async fn run_wizard(command_prefix: &str) -> Result<()> {
    crate::wizard::run_with_prefix(command_prefix).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AgentRunConfig::default();
        assert_eq!(config.server, "http://127.0.0.1:50051");
        assert_eq!(config.name, "agent-1");
        assert!(!config.headless);
        assert!(!config.single);
    }

    #[test]
    fn test_display_mode_from_flag() {
        assert_eq!(DisplayMode::from_headless_flag(false), DisplayMode::Tui);
        assert_eq!(DisplayMode::from_headless_flag(true), DisplayMode::Headless);
    }

    #[test]
    fn test_sanitize_project_name() {
        assert_eq!(sanitize_project_name("my-project"), "my-project");
        assert_eq!(sanitize_project_name("MyProject"), "myproject");
        assert_eq!(sanitize_project_name("my.project"), "my-project");
    }
}
