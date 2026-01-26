// ABOUTME: Unified CLI entry point for all coven commands.
// ABOUTME: Dispatches to swarm, agent, chat, pack, and other subcommands.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::Command;

/// Default gateway server address
const DEFAULT_SERVER: &str = "http://127.0.0.1:50051";

#[derive(Parser)]
#[command(name = "coven")]
#[command(about = "Unified CLI for coven agent orchestration")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// First-time setup wizard
    Init,

    /// Swarm management commands
    #[command(subcommand)]
    Swarm(SwarmCommands),

    /// Agent commands
    #[command(subcommand)]
    Agent(AgentCommands),

    /// Open the TUI chat interface
    Chat {
        /// Gateway server URL
        #[arg(short, long)]
        gateway: Option<String>,

        /// Theme name
        #[arg(short, long)]
        theme: Option<String>,
    },

    /// Pack management commands
    #[command(subcommand)]
    Pack(PackCommands),

    /// Show version information
    Version,
}

#[derive(Subcommand)]
enum SwarmCommands {
    /// Start the supervisor daemon
    Start {
        /// Path to configuration file
        #[arg(long)]
        config: Option<PathBuf>,

        /// Run in headless mode (no TUI)
        #[arg(long)]
        headless: bool,
    },

    /// Stop the supervisor daemon
    Stop,

    /// Show status of running agents
    Status,
}

#[derive(Subcommand)]
enum AgentCommands {
    /// Run an individual agent
    Run {
        /// Control server address
        #[arg(short, long, default_value = DEFAULT_SERVER)]
        server: String,

        /// Agent name
        #[arg(short, long, default_value = "agent-1")]
        name: String,

        /// Agent ID (auto-generated if not provided)
        #[arg(long)]
        id: Option<String>,

        /// Backend to use: "mux" (direct API) or "cli" (Claude CLI)
        #[arg(short, long, env = "COVEN_BACKEND")]
        backend: Option<String>,

        /// Working directory for the agent
        #[arg(short, long)]
        working_dir: Option<PathBuf>,

        /// Load configuration from a file
        #[arg(long)]
        config: Option<PathBuf>,

        /// Headless mode (minimal output, for servers)
        #[arg(long)]
        headless: bool,

        /// Run in single-user interactive mode (no gRPC server)
        #[arg(long)]
        single: bool,
    },

    /// Create a new agent configuration interactively
    New,
}

#[derive(Subcommand)]
enum PackCommands {
    /// List available packs
    List,

    /// Install a pack
    Install {
        /// Pack name to install
        pack: String,
    },

    /// Run a pack directly
    Run {
        /// Pack name to run
        pack: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if present
    let _ = dotenvy::dotenv();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            run_init()
        }
        Commands::Swarm(cmd) => {
            run_swarm(cmd).await
        }
        Commands::Agent(cmd) => {
            run_agent(cmd).await
        }
        Commands::Chat { gateway, theme } => {
            run_chat(gateway, theme).await
        }
        Commands::Pack(cmd) => {
            run_pack(cmd).await
        }
        Commands::Version => {
            print_version();
            Ok(())
        }
    }
}

/// Run the first-time setup wizard
fn run_init() -> Result<()> {
    coven_swarm::run_init()
}

/// Handle swarm subcommands
async fn run_swarm(cmd: SwarmCommands) -> Result<()> {
    match cmd {
        SwarmCommands::Start { config, headless } => {
            let options = coven_swarm::SupervisorOptions {
                config_path: config,
                headless,
            };
            coven_swarm::run_supervisor(options).await
        }
        SwarmCommands::Stop => {
            // Load config to get prefix for socket path
            let config = coven_swarm_core::Config::load(
                &coven_swarm_core::Config::default_path()?,
            )?;

            match coven_swarm::SocketClient::connect(&config.prefix).await {
                Ok(mut client) => {
                    client.stop().await?;
                    println!("Supervisor stopped.");
                    Ok(())
                }
                Err(e) => {
                    eprintln!("Failed to connect to supervisor: {}", e);
                    eprintln!("Is the supervisor running? Try 'coven swarm start' first.");
                    Err(e)
                }
            }
        }
        SwarmCommands::Status => {
            // Load config to get prefix for socket path
            let config = coven_swarm_core::Config::load(
                &coven_swarm_core::Config::default_path()?,
            )?;

            match coven_swarm::SocketClient::connect(&config.prefix).await {
                Ok(mut client) => {
                    let status = client.status().await?;
                    println!("Swarm Status");
                    println!("  Prefix: {}", status.prefix);
                    println!("  Agents: {}", status.agents.len());
                    println!();
                    for agent in &status.agents {
                        let status_str = if agent.running { "running" } else { "stopped" };
                        let pid_str = agent
                            .pid
                            .map(|p| format!("(pid: {})", p))
                            .unwrap_or_default();
                        println!("  - {} [{}] {}", agent.workspace, status_str, pid_str);
                    }
                    Ok(())
                }
                Err(e) => {
                    eprintln!("Failed to connect to supervisor: {}", e);
                    eprintln!("Is the supervisor running? Try 'coven swarm start' first.");
                    Err(e)
                }
            }
        }
    }
}

/// Handle agent subcommands
async fn run_agent(cmd: AgentCommands) -> Result<()> {
    match cmd {
        AgentCommands::Run {
            server,
            name,
            id,
            backend,
            working_dir,
            config,
            headless,
            single,
        } => {
            let agent_config = coven_agent::AgentRunConfig {
                server,
                name,
                id,
                backend,
                working_dir,
                config,
                headless,
                single,
            };
            coven_agent::run_agent(agent_config).await
        }
        AgentCommands::New => {
            coven_agent::run_wizard().await
        }
    }
}

/// Open the TUI chat interface
async fn run_chat(gateway: Option<String>, theme: Option<String>) -> Result<()> {
    coven_tui::run_chat(gateway, theme)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))
}

/// Built-in pack definitions.
struct BuiltinPack {
    name: &'static str,
    binary: &'static str,
    description: &'static str,
}

/// List of built-in packs that ship with coven.
const BUILTIN_PACKS: &[BuiltinPack] = &[
    BuiltinPack {
        name: "productivity",
        binary: "productivity-pack",
        description: "Task management and productivity tools (todo, notes)",
    },
    BuiltinPack {
        name: "mcp-bridge",
        binary: "mcp-bridge-pack",
        description: "Bridge to expose any MCP server as coven tools",
    },
    BuiltinPack {
        name: "test",
        binary: "test-pack",
        description: "Echo tools for testing pack connectivity",
    },
];

/// Get the packs configuration directory (~/.config/coven/packs/).
fn packs_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("coven").join("packs"))
}

/// List installed packs by checking the packs config directory.
fn list_installed_packs() -> Vec<String> {
    let Some(packs_dir) = packs_config_dir() else {
        return Vec::new();
    };

    if !packs_dir.exists() {
        return Vec::new();
    }

    let Ok(entries) = std::fs::read_dir(&packs_dir) else {
        return Vec::new();
    };

    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

/// Find the pack binary path, checking PATH and cargo target directory.
fn find_pack_binary(pack_name: &str) -> Option<PathBuf> {
    // First, check if there's a built-in pack with this name
    let binary_name = BUILTIN_PACKS
        .iter()
        .find(|p| p.name == pack_name)
        .map(|p| p.binary)
        .unwrap_or(pack_name);

    // Check if binary exists in PATH (cross-platform)
    if let Ok(path_var) = std::env::var("PATH") {
        #[cfg(windows)]
        let separator = ';';
        #[cfg(not(windows))]
        let separator = ':';

        for dir in path_var.split(separator) {
            let candidate = PathBuf::from(dir).join(binary_name);
            if candidate.exists() && candidate.is_file() {
                return Some(candidate);
            }
            // On Windows, also check with .exe extension
            #[cfg(windows)]
            {
                let candidate_exe = PathBuf::from(dir).join(format!("{}.exe", binary_name));
                if candidate_exe.exists() && candidate_exe.is_file() {
                    return Some(candidate_exe);
                }
            }
        }
    }

    // Check cargo target directories (debug and release)
    let cargo_dirs = [
        std::env::current_dir().ok(),
        std::env::var("CARGO_MANIFEST_DIR").ok().map(PathBuf::from),
    ];

    for cargo_dir in cargo_dirs.into_iter().flatten() {
        for profile in ["debug", "release"] {
            let binary_path = cargo_dir.join("target").join(profile).join(binary_name);
            if binary_path.exists() {
                return Some(binary_path);
            }
        }
    }

    None
}

/// Handle pack subcommands
async fn run_pack(cmd: PackCommands) -> Result<()> {
    match cmd {
        PackCommands::List => {
            println!("coven pack list");
            println!();

            // List built-in packs
            println!("Built-in packs:");
            for pack in BUILTIN_PACKS {
                let status = if find_pack_binary(pack.name).is_some() {
                    "available"
                } else {
                    "not built"
                };
                println!("  {:15} [{}] {}", pack.name, status, pack.description);
            }
            println!();

            // List installed packs (those with config directories)
            let installed = list_installed_packs();
            if !installed.is_empty() {
                println!("Configured packs (have SSH keys):");
                for pack in &installed {
                    println!("  {}", pack);
                }
                println!();
            }

            println!("Use 'coven pack run <name>' to start a pack.");
            Ok(())
        }
        PackCommands::Install { pack } => {
            println!("coven pack install {}", pack);
            println!();

            // For now, packs are Rust binaries that need to be compiled
            // Future: support downloading pre-built binaries or building from source
            println!("Pack installation is not yet implemented.");
            println!();
            println!("To use built-in packs, build them with:");
            println!("  cargo build -p {}-pack", pack);
            println!();
            println!("Then run with:");
            println!("  coven pack run {}", pack);

            Ok(())
        }
        PackCommands::Run { pack } => {
            // Find the pack binary
            let binary_path = find_pack_binary(&pack)
                .with_context(|| format!("Pack '{}' not found. Try building it first with: cargo build -p {}-pack", pack, pack))?;

            println!("Starting pack: {}", pack);
            println!("Binary: {}", binary_path.display());
            println!();

            // Execute the pack binary, inheriting stdio
            let status = Command::new(&binary_path)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status()
                .with_context(|| format!("Failed to execute pack binary: {}", binary_path.display()))?;

            if !status.success() {
                let code = status.code().unwrap_or(1);
                anyhow::bail!("Pack '{}' exited with code {}", pack, code);
            }

            Ok(())
        }
    }
}

/// Print version information
fn print_version() {
    println!("fold {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Component versions:");
    println!("  coven-cli:    {}", env!("CARGO_PKG_VERSION"));
    println!("  coven-client: (linked)");
    println!("  coven-pack:   (linked)");
    println!();
    println!("Repository: https://github.com/2389-research/fold");
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli_structure() {
        // Ensure CLI structure is valid
        Cli::command().debug_assert();
    }
}
