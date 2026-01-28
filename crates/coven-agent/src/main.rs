// ABOUTME: coven-agent binary - connects to control server via GRPC
// ABOUTME: Run directly or use 'new' subcommand for interactive wizard

mod client;
mod metadata;
mod pack_tool;
mod single;
mod tui;
mod wizard;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

// Re-export from lib for use in this binary and for tests
pub use coven_agent::build_mcp_url;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "coven-agent")]
#[command(about = "Fold agent - connects to control server")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Control server address
    #[arg(short, long, default_value = "http://127.0.0.1:50051", global = true)]
    server: String,

    /// Agent name
    #[arg(short, long, default_value = "agent-1", global = true)]
    name: String,

    /// Agent ID (auto-generated if not provided)
    #[arg(long, global = true)]
    id: Option<String>,

    /// Backend to use: "mux" (direct API) or "cli" (Claude CLI)
    #[arg(short, long, env = "COVEN_BACKEND", global = true)]
    backend: Option<String>,

    /// Working directory for the agent (defaults to current directory)
    #[arg(short, long, global = true)]
    working_dir: Option<PathBuf>,

    /// Load configuration from a file (default: ~/.config/coven/agent.toml)
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Headless mode (minimal output, for servers). Default is visual TUI.
    #[arg(long, conflicts_with = "single", global = true)]
    headless: bool,

    /// Run in single-user interactive mode (no gRPC server)
    #[arg(long, conflicts_with = "headless", global = true)]
    single: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new agent configuration interactively
    New,
}

/// Determine display mode based on flags
#[derive(Debug, Clone, Copy, PartialEq)]
enum DisplayMode {
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
    // Try to load .coven/project.toml (no exists() check - read_to_string handles it)
    let project_config_path = working_dir.join(".coven").join("project.toml");
    if let Ok(content) = std::fs::read_to_string(&project_config_path) {
        if let Ok(config) = content.parse::<toml::Table>() {
            if let Some(name) = config.get("project_name").and_then(|v| v.as_str()) {
                // Validate: non-empty, reasonable length, not just whitespace
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

/// Sanitize project name for use in agent ID (ASCII alphanumeric, hyphens, underscores)
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

    // Trim leading/trailing hyphens and collapse consecutive hyphens
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

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if present (ignore errors if not found)
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::New) => {
            // Run the interactive wizard (no logging needed for TUI)
            wizard::run_with_prefix("coven-agent").await
        }
        None => {
            // Default: run the agent with provided flags
            let mode = DisplayMode::from_headless_flag(cli.headless);
            run_agent(
                cli.server,
                cli.name,
                cli.id,
                cli.backend,
                cli.working_dir,
                cli.config,
                mode,
                cli.single,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_agent(
    server: String,
    name: String,
    id: Option<String>,
    backend: Option<String>,
    working_dir: Option<PathBuf>,
    config: Option<PathBuf>,
    mode: DisplayMode,
    single: bool,
) -> Result<()> {
    // Try to load config: explicit path > project-local > user-global
    let config_path = config.or_else(|| {
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
    let (server, name, backend, working_dir, workspaces) = if let Some(ref config_path) =
        config_path
    {
        tracing::info!("Loading config from: {}", config_path.display());
        let config_content = std::fs::read_to_string(config_path)
            .with_context(|| format!("failed to read config file: {}", config_path.display()))?;
        let config: toml::Table = toml::from_str(&config_content)
            .with_context(|| format!("failed to parse config file: {}", config_path.display()))?;

        // Check if this is a reference to a global agent (agent = "name")
        let config = if let Some(agent_ref) = config.get("agent").and_then(|v| v.as_str()) {
            // Resolve to ~/.config/coven/agents/{agent}.toml
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
            config
        };

        // Server can come from:
        // 1. Agent config file (server = "...")
        // 2. User's coven config from `coven link` (~/.config/coven/config.toml)
        // 3. CLI args / defaults
        let server = config
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
            .unwrap_or(server);
        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or(name);
        let backend = config
            .get("backend")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or(backend);
        let config_working_dir = config
            .get("working_dir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);

        // Load workspaces from config
        let workspaces: Vec<String> = config
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
            working_dir.or(config_working_dir),
            workspaces,
        )
    } else if !single {
        // Config is required for gateway mode
        bail!(
            "No configuration found. Create one with 'coven-agent new' or specify --config.\n\
             Searched:\n\
             - .coven/agent.toml (project-local)\n\
             - ~/.config/coven/agent.toml (user-global)"
        );
    } else {
        // Single mode can work without config (just uses defaults)
        (server, name, backend, working_dir, Vec::new())
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
    // Project name comes from: .coven/project.toml > directory basename
    let agent_id = id.unwrap_or_else(|| {
        let project_name = resolve_project_name(&working_dir);
        format!("{}-{}", name, project_name)
    });

    if single {
        return single::run(&name, &agent_id, &backend_type, &working_dir).await;
    }

    match mode {
        DisplayMode::Tui => tui::run(&server, &agent_id, &backend_type, &working_dir).await,
        DisplayMode::Headless => {
            // Gather metadata for registration
            let mut metadata = metadata::AgentMetadata::gather(&working_dir);
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

            client::run(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_project_name_basic() {
        assert_eq!(sanitize_project_name("my-project"), "my-project");
        assert_eq!(sanitize_project_name("MyProject"), "myproject");
        assert_eq!(sanitize_project_name("my_project"), "my_project");
    }

    #[test]
    fn test_sanitize_project_name_special_chars() {
        assert_eq!(sanitize_project_name("my.project"), "my-project");
        assert_eq!(sanitize_project_name("my project"), "my-project");
        assert_eq!(sanitize_project_name("my@project!"), "my-project");
    }

    #[test]
    fn test_sanitize_project_name_edge_cases() {
        // Empty string
        assert_eq!(sanitize_project_name(""), "");
        // All special chars becomes empty (all hyphens trimmed)
        assert_eq!(sanitize_project_name("@#$%^"), "");
        // Leading/trailing special chars
        assert_eq!(sanitize_project_name("@my-project!"), "my-project");
        // Consecutive special chars
        assert_eq!(sanitize_project_name("my...project"), "my-project");
    }

    #[test]
    fn test_sanitize_project_name_unicode() {
        // Non-ASCII alphanumeric should be replaced with hyphen
        assert_eq!(sanitize_project_name("über-project"), "ber-project");
        assert_eq!(sanitize_project_name("项目"), "");
    }

    #[test]
    fn test_resolve_project_name_fallback() {
        // When given a path, should extract basename
        let path = std::path::Path::new("/Users/test/my-project");
        assert_eq!(resolve_project_name(path), "my-project");

        // Root path should return "unknown"
        let root = std::path::Path::new("/");
        assert_eq!(resolve_project_name(root), "unknown");
    }

    #[test]
    fn test_build_mcp_url_simple() {
        let url = build_mcp_url("https://example.com/mcp", "my-token");
        assert_eq!(url, "https://example.com/mcp/my-token");
    }

    #[test]
    fn test_build_mcp_url_with_trailing_slash() {
        let url = build_mcp_url("https://example.com/mcp/", "my-token");
        assert_eq!(url, "https://example.com/mcp/my-token");
    }

    #[test]
    fn test_build_mcp_url_token_in_path() {
        // Token becomes a path segment
        let url = build_mcp_url("http://localhost:8080/mcp", "abc-123-def");
        assert_eq!(url, "http://localhost:8080/mcp/abc-123-def");
    }

    #[test]
    fn test_build_mcp_url_malformed_fallback() {
        // Malformed URL (no scheme) uses percent-encoding fallback
        let url = build_mcp_url("not-a-valid-url/path", "my-token");
        assert_eq!(url, "not-a-valid-url/path/my%2Dtoken");
    }

    #[test]
    fn test_build_mcp_url_malformed_trailing_slash() {
        let url = build_mcp_url("not-a-valid-url/path/", "my-token");
        assert_eq!(url, "not-a-valid-url/path/my%2Dtoken");
    }

    #[test]
    fn test_build_mcp_url_special_chars_in_token() {
        let url = build_mcp_url("http://localhost:8080/mcp", "abc/def?key=val#frag");
        // Path-separator, query, and fragment delimiters must be percent-encoded
        assert!(!url.contains("?key=val"), "query delimiter must be encoded");
        assert!(!url.contains("#frag"), "fragment delimiter must be encoded");
        // The url crate encodes /, ?, # but leaves = (which is valid in path segments)
        assert!(url.contains("abc%2Fdef%3Fkey=val%23frag"));
    }

    #[test]
    fn test_build_mcp_url_malformed_special_chars_in_token() {
        let url = build_mcp_url("not-a-valid-url/path", "abc/def?key=val#frag");
        // Fallback uses NON_ALPHANUMERIC which encodes all non-alphanumeric chars
        assert!(!url.contains("?key=val"), "query delimiter must be encoded");
        assert!(!url.contains("#frag"), "fragment delimiter must be encoded");
        assert!(url.contains("abc%2Fdef%3Fkey%3Dval%23frag"));
    }

    #[test]
    fn test_build_mcp_url_fallback_with_query_in_base() {
        // Fallback path: base URL has query string - token goes before it
        let url = build_mcp_url("not-a-url/path?existing=param", "my-token");
        assert_eq!(url, "not-a-url/path/my%2Dtoken?existing=param");
    }

    #[test]
    fn test_build_mcp_url_fallback_with_fragment_in_base() {
        // Fallback path: base URL has fragment - token goes before it
        let url = build_mcp_url("not-a-url/path#section", "my-token");
        assert_eq!(url, "not-a-url/path/my%2Dtoken#section");
    }

    #[test]
    fn test_build_mcp_url_non_hierarchical() {
        // Non-hierarchical URL (path_segments_mut returns Err) falls to fallback
        let url = build_mcp_url("data:text/plain,hello", "my-token");
        // data: URLs can't have path segments, so fallback kicks in
        assert!(url.contains("my%2Dtoken"));
    }
}
