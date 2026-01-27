// ABOUTME: Entry point for coven-admin CLI
// ABOUTME: Provides admin commands for managing coven-gateway

use anyhow::Result;
use clap::Parser;
use serde::Deserialize;

mod client;
mod commands;

use commands::{Cli, Command};

/// Config file structure (subset of what coven-link writes)
#[derive(Deserialize, Default)]
struct CovenConfig {
    #[serde(default)]
    gateway: Option<String>,
    #[serde(default)]
    token: Option<String>,
}

impl CovenConfig {
    fn load() -> Self {
        dirs::home_dir()
            .map(|d| d.join(".config/coven/config.toml"))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }
}

/// Normalize gateway address to include scheme
fn normalize_gateway(gateway: &str) -> String {
    let g = gateway.trim();
    if g.starts_with("http://") || g.starts_with("https://") {
        return g.to_string();
    }
    // Default to http for gRPC (TLS usually handled at network layer e.g. Tailscale)
    format!("http://{}", g)
}

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

    // Load config file for defaults
    let config = CovenConfig::load();

    // Determine gateway address (CLI > env > config > default)
    let gateway = cli
        .gateway
        .or_else(|| std::env::var("COVEN_GATEWAY_GRPC").ok())
        .or(config.gateway)
        .map(|g| normalize_gateway(&g))
        .unwrap_or_else(|| "http://localhost:50051".to_string());

    // Determine token (CLI > env > config)
    let token = cli
        .token
        .or_else(|| std::env::var("COVEN_TOKEN").ok())
        .or(config.token);

    match cli.command {
        Command::Me => commands::me::run(&gateway, token.as_deref()).await,
        Command::Agents(cmd) => commands::agents::run(&gateway, token.as_deref(), cmd).await,
        Command::Bindings(cmd) => commands::bindings::run(&gateway, token.as_deref(), cmd).await,
        Command::Principals(cmd) => commands::principals::run(&gateway, token.as_deref(), cmd).await,
        Command::Token(cmd) => commands::token::run(&gateway, token.as_deref(), cmd).await,
    }
}
