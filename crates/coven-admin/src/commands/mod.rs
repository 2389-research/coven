// ABOUTME: CLI command definitions using clap
// ABOUTME: Defines subcommands for admin operations

use clap::{Parser, Subcommand};

pub mod agents;
pub mod bindings;
pub mod me;
pub mod token;

#[derive(Parser)]
#[command(name = "coven-admin", about = "Admin CLI for coven-gateway")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Gateway gRPC address (e.g., http://localhost:50051)
    #[arg(long, global = true, env = "COVEN_GATEWAY_GRPC")]
    pub gateway: Option<String>,

    /// JWT authentication token
    #[arg(long, global = true, env = "COVEN_TOKEN")]
    pub token: Option<String>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Show your identity (principal info)
    Me,

    /// Manage agents
    #[command(subcommand)]
    Agents(AgentsCommand),

    /// Manage bindings
    #[command(subcommand)]
    Bindings(BindingsCommand),

    /// Manage tokens
    #[command(subcommand)]
    Token(TokenCommand),
}

#[derive(Subcommand)]
pub enum AgentsCommand {
    /// List all connected agents
    List {
        /// Filter by workspace path
        #[arg(long)]
        workspace: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum BindingsCommand {
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
pub enum TokenCommand {
    /// Create a new token for a principal
    Create {
        /// Principal ID to create token for
        principal_id: String,

        /// Token TTL in seconds (default: 30 days)
        #[arg(long, default_value = "2592000")]
        ttl: i64,
    },
}
