// ABOUTME: Entry point for coven-admin CLI
// ABOUTME: Provides admin commands for managing coven-gateway

use anyhow::Result;
use clap::Parser;

mod client;
mod commands;

use commands::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if present
    let _ = dotenvy::dotenv();

    // Initialize tracing with RUST_LOG support
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    let cli = Cli::parse();

    // Determine gateway address
    let gateway = cli.gateway.unwrap_or_else(|| {
        std::env::var("COVEN_GATEWAY_GRPC").unwrap_or_else(|_| "http://localhost:50051".to_string())
    });

    // Determine token (CLI flag > env var > token file)
    let token = cli
        .token
        .or_else(|| std::env::var("COVEN_TOKEN").ok())
        .or_else(|| {
            // Try reading from token file
            dirs::config_dir()
                .map(|d| d.join("coven/token"))
                .and_then(|p| std::fs::read_to_string(p).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });

    match cli.command {
        Command::Me => commands::me::run(&gateway, token.as_deref()).await,
        Command::Agents(cmd) => commands::agents::run(&gateway, token.as_deref(), cmd).await,
        Command::Bindings(cmd) => commands::bindings::run(&gateway, token.as_deref(), cmd).await,
        Command::Principals(cmd) => commands::principals::run(&gateway, token.as_deref(), cmd).await,
        Command::Token(cmd) => commands::token::run(&gateway, token.as_deref(), cmd).await,
    }
}
