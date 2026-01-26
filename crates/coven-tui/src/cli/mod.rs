// ABOUTME: CLI command definitions using clap.
// ABOUTME: Handles subcommands like send, doctor, agents.

pub mod agents;
pub mod doctor;
pub mod send;
pub mod setup;
pub mod themes;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "coven-chat", about = "Coven TUI - Chat with AI agents")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Gateway URL override
    #[arg(long, global = true)]
    pub gateway: Option<String>,

    /// Theme override
    #[arg(long, global = true)]
    pub theme: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Interactive setup wizard
    Setup,
    /// Check gateway connectivity and list agents
    Doctor,
    /// List available agents
    Agents,
    /// Send a message and print response
    Send {
        /// Agent ID or name
        agent: String,
        /// Message to send
        message: String,
    },
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    /// Manage themes
    Themes {
        #[command(subcommand)]
        action: Option<ThemeAction>,
    },
    /// Generate shell completions
    Completion {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Show version
    Version,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Open config in editor
    Edit,
    /// Show config file path
    Path,
    /// Set a config value
    Set { pair: String },
}

#[derive(Subcommand, Debug)]
pub enum ThemeAction {
    /// List available themes
    List,
    /// Set active theme
    Set { name: String },
}
