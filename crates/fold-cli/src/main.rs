// ABOUTME: Unified CLI entry point for all fold commands.
// ABOUTME: Dispatches to swarm, agent, chat, pack, and other subcommands.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "fold")]
#[command(about = "Unified CLI for fold agent orchestration")]
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
        #[arg(short, long, default_value = "http://127.0.0.1:50051")]
        server: String,

        /// Agent name
        #[arg(short, long, default_value = "agent-1")]
        name: String,

        /// Agent ID (auto-generated if not provided)
        #[arg(long)]
        id: Option<String>,

        /// Backend to use: "mux" (direct API) or "cli" (Claude CLI)
        #[arg(short, long, env = "FOLD_BACKEND")]
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
            run_init().await
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
async fn run_init() -> Result<()> {
    fold_swarm::run_init()
}

/// Handle swarm subcommands
async fn run_swarm(cmd: SwarmCommands) -> Result<()> {
    match cmd {
        SwarmCommands::Start { config, headless } => {
            let options = fold_swarm::SupervisorOptions {
                config_path: config,
                headless,
            };
            fold_swarm::run_supervisor(options).await
        }
        SwarmCommands::Stop => {
            // Load config to get prefix for socket path
            let config = fold_swarm_core::Config::load(
                &fold_swarm_core::Config::default_path()?,
            )?;

            match fold_swarm::SocketClient::connect(&config.prefix).await {
                Ok(mut client) => {
                    client.stop().await?;
                    println!("Supervisor stopped.");
                    Ok(())
                }
                Err(e) => {
                    eprintln!("Failed to connect to supervisor: {}", e);
                    eprintln!("Is the supervisor running? Try 'fold swarm start' first.");
                    Err(e)
                }
            }
        }
        SwarmCommands::Status => {
            // Load config to get prefix for socket path
            let config = fold_swarm_core::Config::load(
                &fold_swarm_core::Config::default_path()?,
            )?;

            match fold_swarm::SocketClient::connect(&config.prefix).await {
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
                    eprintln!("Is the supervisor running? Try 'fold swarm start' first.");
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
            let agent_config = fold_agent::AgentRunConfig {
                server,
                name,
                id,
                backend,
                working_dir,
                config,
                headless,
                single,
            };
            fold_agent::run_agent(agent_config).await
        }
        AgentCommands::New => {
            fold_agent::run_wizard().await
        }
    }
}

/// Open the TUI chat interface
async fn run_chat(gateway: Option<String>, theme: Option<String>) -> Result<()> {
    fold_tui::run_chat(gateway, theme)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))
}

/// Handle pack subcommands
async fn run_pack(cmd: PackCommands) -> Result<()> {
    match cmd {
        PackCommands::List => {
            println!("fold pack list");
            println!();
            println!("Available packs:");
            println!("  - productivity-pack: Task management and productivity tools");
            println!("  - mcp-bridge-pack: MCP protocol bridge");
            println!("  - test-pack: Example pack for testing");
            println!();
            println!("TODO: Query gateway for registered packs");
            Ok(())
        }
        PackCommands::Install { pack } => {
            println!("fold pack install {}", pack);
            println!();
            println!("TODO: Pack installation from registry");
            println!("  - Download pack from registry");
            println!("  - Verify signature");
            println!("  - Install to ~/.config/fold/packs/");
            Ok(())
        }
        PackCommands::Run { pack } => {
            println!("fold pack run {}", pack);
            println!();
            println!("TODO: Run pack directly");
            println!("  - Load pack configuration");
            println!("  - Connect to gateway");
            println!("  - Register tools and handle requests");
            Ok(())
        }
    }
}

/// Print version information
fn print_version() {
    println!("fold {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Component versions:");
    println!("  fold-cli:    {}", env!("CARGO_PKG_VERSION"));
    println!("  fold-client: (linked)");
    println!("  fold-pack:   (linked)");
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
