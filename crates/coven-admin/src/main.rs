// ABOUTME: Entry point for coven-admin CLI
// ABOUTME: Provides admin commands for managing coven-gateway

use anyhow::Result;
use clap::Parser;
use coven_admin::commands::Cli;

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

    coven_admin::run_command(cli.command, cli.gateway, cli.token).await
}
