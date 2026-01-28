// ABOUTME: Unified CLI entry point for all coven commands.
// ABOUTME: Dispatches to swarm, agent, chat, pack, admin, link, bridge and other subcommands.

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

    /// Link this device to a coven-gateway
    Link {
        /// Gateway URL (e.g., https://coven.example.com or http://localhost:8080)
        gateway: String,

        /// Device name (defaults to hostname)
        #[arg(long, short = 'n')]
        name: Option<String>,

        /// Path to SSH key (defaults to ~/.config/coven/device_key)
        #[arg(long)]
        key: Option<String>,
    },

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

    /// Admin commands for gateway management
    #[command(subcommand)]
    Admin(AdminCommands),

    /// Bridge commands for external services
    #[command(subcommand)]
    Bridge(BridgeCommands),

    /// Show version information
    Version,
}

#[derive(Subcommand)]
enum SwarmCommands {
    /// Initialize swarm configuration
    Init,

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

#[derive(Subcommand)]
enum AdminCommands {
    /// Show your identity (principal info)
    Me {
        /// Gateway gRPC address
        #[arg(long, env = "COVEN_GATEWAY_GRPC")]
        gateway: Option<String>,

        /// JWT authentication token
        #[arg(long, env = "COVEN_TOKEN")]
        token: Option<String>,
    },

    /// Manage agents
    Agents {
        /// Gateway gRPC address
        #[arg(long, env = "COVEN_GATEWAY_GRPC")]
        gateway: Option<String>,

        /// JWT authentication token
        #[arg(long, env = "COVEN_TOKEN")]
        token: Option<String>,

        #[command(subcommand)]
        command: AdminAgentsCommand,
    },

    /// Manage bindings
    Bindings {
        /// Gateway gRPC address
        #[arg(long, env = "COVEN_GATEWAY_GRPC")]
        gateway: Option<String>,

        /// JWT authentication token
        #[arg(long, env = "COVEN_TOKEN")]
        token: Option<String>,

        #[command(subcommand)]
        command: AdminBindingsCommand,
    },

    /// Manage principals (agents, clients)
    Principals {
        /// Gateway gRPC address
        #[arg(long, env = "COVEN_GATEWAY_GRPC")]
        gateway: Option<String>,

        /// JWT authentication token
        #[arg(long, env = "COVEN_TOKEN")]
        token: Option<String>,

        #[command(subcommand)]
        command: AdminPrincipalsCommand,
    },

    /// Manage tokens
    Token {
        /// Gateway gRPC address
        #[arg(long, env = "COVEN_GATEWAY_GRPC")]
        gateway: Option<String>,

        /// JWT authentication token
        #[arg(long, env = "COVEN_TOKEN")]
        token: Option<String>,

        #[command(subcommand)]
        command: AdminTokenCommand,
    },
}

#[derive(Subcommand)]
enum AdminAgentsCommand {
    /// List all connected agents
    List {
        /// Filter by workspace path
        #[arg(long)]
        workspace: Option<String>,
    },
}

#[derive(Subcommand)]
enum AdminBindingsCommand {
    /// List all bindings
    List,

    /// Create a binding
    Create {
        /// Frontend identifier (e.g., "slack", "matrix")
        #[arg(long)]
        frontend: String,

        /// Channel ID from the frontend
        #[arg(long)]
        channel_id: String,

        /// Agent ID to route messages to
        #[arg(long)]
        agent_id: String,
    },

    /// Delete a binding
    Delete {
        /// Binding ID to delete
        id: String,
    },
}

#[derive(Subcommand)]
enum AdminPrincipalsCommand {
    /// List all principals
    List {
        /// Filter by type (agent, client)
        #[arg(long, short = 't')]
        r#type: Option<String>,
    },

    /// Create a new principal
    Create {
        /// Principal type: "agent" or "client"
        #[arg(long, short = 't')]
        r#type: String,

        /// Display name
        #[arg(long, short = 'n')]
        name: String,

        /// Public key fingerprint (SHA256 hex, for agents)
        #[arg(long)]
        fingerprint: Option<String>,

        /// Roles to assign (e.g., "member", "owner")
        #[arg(long, short = 'r')]
        role: Vec<String>,
    },

    /// Delete a principal
    Delete {
        /// Principal ID to delete
        id: String,
    },
}

#[derive(Subcommand)]
enum AdminTokenCommand {
    /// Create a new token for a principal
    Create {
        /// Principal ID to create token for
        principal_id: String,

        /// Token TTL in seconds (default: 30 days)
        #[arg(long, default_value = "2592000")]
        ttl: i64,
    },
}

#[derive(Subcommand)]
enum BridgeCommands {
    /// Run Slack bridge
    Slack {
        /// Config file path
        #[arg(short, long, env = "COVEN_SLACK_CONFIG")]
        config: Option<PathBuf>,
    },

    /// Run Matrix bridge
    Matrix {
        /// Config file path
        #[arg(short, long, env = "COVEN_MATRIX_CONFIG")]
        config: Option<PathBuf>,
    },

    /// Run Telegram bridge
    Telegram {
        /// Config file path
        #[arg(short, long, env = "COVEN_TELEGRAM_CONFIG")]
        config: Option<PathBuf>,
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
        Commands::Init => run_init(),
        Commands::Link { gateway, name, key } => run_link(gateway, name, key).await,
        Commands::Swarm(cmd) => run_swarm(cmd).await,
        Commands::Agent(cmd) => run_agent(cmd).await,
        Commands::Chat { gateway, theme } => run_chat(gateway, theme).await,
        Commands::Pack(cmd) => run_pack(cmd).await,
        Commands::Admin(cmd) => run_admin(cmd).await,
        Commands::Bridge(cmd) => run_bridge(cmd).await,
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

/// Link this device to a gateway
async fn run_link(gateway: String, name: Option<String>, key: Option<String>) -> Result<()> {
    coven_link::run(gateway, name, key).await
}

/// Handle swarm subcommands
async fn run_swarm(cmd: SwarmCommands) -> Result<()> {
    match cmd {
        SwarmCommands::Init => {
            coven_swarm::run_init()
        }
        SwarmCommands::Start { config, headless } => {
            let options = coven_swarm::SupervisorOptions {
                config_path: config,
                headless,
            };
            coven_swarm::run_supervisor(options).await
        }
        SwarmCommands::Stop => {
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
        AgentCommands::New => coven_agent::run_wizard("coven agent").await,
    }
}

/// Open the TUI chat interface
async fn run_chat(gateway: Option<String>, theme: Option<String>) -> Result<()> {
    coven_tui::run_chat(gateway, theme)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))
}

/// Handle admin subcommands
async fn run_admin(cmd: AdminCommands) -> Result<()> {
    match cmd {
        AdminCommands::Me { gateway, token } => {
            coven_admin::run_command(coven_admin::Command::Me, gateway, token).await
        }
        AdminCommands::Agents { gateway, token, command } => {
            let admin_cmd = match command {
                AdminAgentsCommand::List { workspace } => {
                    coven_admin::Command::Agents(coven_admin::AgentsCommand::List { workspace })
                }
            };
            coven_admin::run_command(admin_cmd, gateway, token).await
        }
        AdminCommands::Bindings { gateway, token, command } => {
            let admin_cmd = match command {
                AdminBindingsCommand::List => {
                    coven_admin::Command::Bindings(coven_admin::BindingsCommand::List)
                }
                AdminBindingsCommand::Create { frontend, channel_id, agent_id } => {
                    coven_admin::Command::Bindings(coven_admin::BindingsCommand::Create {
                        frontend,
                        channel_id,
                        agent_id,
                    })
                }
                AdminBindingsCommand::Delete { id } => {
                    coven_admin::Command::Bindings(coven_admin::BindingsCommand::Delete { id })
                }
            };
            coven_admin::run_command(admin_cmd, gateway, token).await
        }
        AdminCommands::Principals { gateway, token, command } => {
            let admin_cmd = match command {
                AdminPrincipalsCommand::List { r#type } => {
                    coven_admin::Command::Principals(coven_admin::PrincipalsCommand::List { r#type })
                }
                AdminPrincipalsCommand::Create { r#type, name, fingerprint, role } => {
                    coven_admin::Command::Principals(coven_admin::PrincipalsCommand::Create {
                        r#type,
                        name,
                        fingerprint,
                        role,
                    })
                }
                AdminPrincipalsCommand::Delete { id } => {
                    coven_admin::Command::Principals(coven_admin::PrincipalsCommand::Delete { id })
                }
            };
            coven_admin::run_command(admin_cmd, gateway, token).await
        }
        AdminCommands::Token { gateway, token, command } => {
            let admin_cmd = match command {
                AdminTokenCommand::Create { principal_id, ttl } => {
                    coven_admin::Command::Token(coven_admin::TokenCommand::Create {
                        principal_id,
                        ttl,
                    })
                }
            };
            coven_admin::run_command(admin_cmd, gateway, token).await
        }
    }
}

/// Handle bridge subcommands
async fn run_bridge(cmd: BridgeCommands) -> Result<()> {
    match cmd {
        BridgeCommands::Slack { config } => coven_slack_rs::run(config).await,
        BridgeCommands::Matrix { config } => coven_matrix_rs::run(config).await,
        BridgeCommands::Telegram { config } => coven_telegram_rs::run(config).await,
    }
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
    let binary_name = BUILTIN_PACKS
        .iter()
        .find(|p| p.name == pack_name)
        .map(|p| p.binary)
        .unwrap_or(pack_name);

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
            #[cfg(windows)]
            {
                let candidate_exe = PathBuf::from(dir).join(format!("{}.exe", binary_name));
                if candidate_exe.exists() && candidate_exe.is_file() {
                    return Some(candidate_exe);
                }
            }
        }
    }

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
            let binary_path = find_pack_binary(&pack)
                .with_context(|| format!("Pack '{}' not found. Try building it first with: cargo build -p {}-pack", pack, pack))?;

            println!("Starting pack: {}", pack);
            println!("Binary: {}", binary_path.display());
            println!();

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
    println!("coven {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Component versions:");
    println!("  coven-cli:    {}", env!("CARGO_PKG_VERSION"));
    println!("  coven-client: (linked)");
    println!("  coven-pack:   (linked)");
    println!();
    println!("Repository: https://github.com/2389-research/coven");
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli_structure() {
        Cli::command().debug_assert();
    }
}
