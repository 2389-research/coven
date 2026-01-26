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
    println!("fold init - First-time setup wizard");
    println!();
    println!("This wizard will help you configure fold for your environment.");
    println!();

    // Check for existing configuration
    let config_dir = dirs::config_dir()
        .map(|p| p.join("fold"))
        .unwrap_or_else(|| PathBuf::from(".fold"));

    if config_dir.exists() {
        println!("Configuration directory already exists: {}", config_dir.display());
        println!("Run 'fold agent new' to create a new agent configuration.");
    } else {
        println!("TODO: Interactive setup wizard");
        println!("  - Configure gateway connection");
        println!("  - Set up SSH keys");
        println!("  - Create initial agent configuration");
        println!();
        println!("For now, use 'fold agent new' to create agent configurations.");
    }

    Ok(())
}

/// Handle swarm subcommands
async fn run_swarm(cmd: SwarmCommands) -> Result<()> {
    match cmd {
        SwarmCommands::Start { config, headless } => {
            println!("fold swarm start");
            println!("  config: {:?}", config);
            println!("  headless: {}", headless);
            println!();
            println!("TODO: Delegate to fold-swarm supervisor");
            println!("  Run: fold-swarm supervisor --config <path> {}", if headless { "--headless" } else { "" });
            Ok(())
        }
        SwarmCommands::Stop => {
            println!("fold swarm stop");
            println!();
            println!("TODO: Send stop signal to running supervisor");
            println!("  - Locate supervisor socket");
            println!("  - Send shutdown command");
            Ok(())
        }
        SwarmCommands::Status => {
            println!("fold swarm status");
            println!();
            println!("TODO: Query supervisor for agent status");
            println!("  - Connect to supervisor socket");
            println!("  - List running agents and their state");
            Ok(())
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
            println!("fold agent run");
            println!("  server: {}", server);
            println!("  name: {}", name);
            println!("  id: {:?}", id);
            println!("  backend: {:?}", backend);
            println!("  working_dir: {:?}", working_dir);
            println!("  config: {:?}", config);
            println!("  headless: {}", headless);
            println!("  single: {}", single);
            println!();
            println!("TODO: Delegate to fold-agent");
            println!("  Run: fold-agent --server {} --name {} ...", server, name);
            Ok(())
        }
        AgentCommands::New => {
            println!("fold agent new");
            println!();
            println!("TODO: Delegate to fold-agent new wizard");
            println!("  Run: fold-agent new");
            Ok(())
        }
    }
}

/// Open the TUI chat interface
async fn run_chat(gateway: Option<String>, theme: Option<String>) -> Result<()> {
    println!("fold chat");
    println!("  gateway: {:?}", gateway);
    println!("  theme: {:?}", theme);
    println!();
    println!("TODO: Delegate to fold-tui (folder)");
    let mut args = String::new();
    if let Some(g) = gateway {
        args.push_str(&format!(" --gateway {}", g));
    }
    if let Some(t) = theme {
        args.push_str(&format!(" --theme {}", t));
    }
    println!("  Run: folder{}", args);
    Ok(())
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
