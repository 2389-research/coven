// ABOUTME: coven-swarm CLI entry point.
// ABOUTME: Provides init, supervisor, and agent subcommands.

use clap::{Parser, Subcommand};
use coven_swarm::{run_agent, run_init, run_supervisor, AgentOptions, SupervisorOptions};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "coven-swarm")]
#[command(about = "Supervisor for coven workspace agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new coven-swarm configuration
    Init,
    /// Run the supervisor daemon
    Supervisor {
        /// Path to configuration file
        #[arg(long)]
        config: Option<PathBuf>,
        /// Headless mode (minimal output, no TUI)
        #[arg(long)]
        headless: bool,
    },
    /// Run a single workspace agent (internal, spawned by supervisor)
    Agent {
        /// Workspace name
        #[arg(long)]
        workspace: String,
        /// Run in dispatch mode (swarm management tools)
        #[arg(long)]
        dispatch_mode: bool,
        /// Path to configuration file
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present (ignore errors if not found)
    let _ = dotenvy::dotenv();

    coven_log::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init => run_init(),
        Commands::Supervisor {
            config: config_path,
            headless,
        } => {
            run_supervisor(SupervisorOptions {
                config_path,
                headless,
            })
            .await
        }
        Commands::Agent {
            workspace,
            dispatch_mode,
            config: config_path,
        } => {
            run_agent(AgentOptions {
                workspace,
                dispatch_mode,
                config_path,
            })
            .await
        }
    }
}
