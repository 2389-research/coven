// ABOUTME: Library exports for coven-admin functionality
// ABOUTME: Allows coven-cli to integrate admin commands directly

use anyhow::Result;

pub mod client;
pub mod commands;

pub use commands::{AgentsCommand, BindingsCommand, Command, PrincipalsCommand, TokenCommand};

/// Config file structure (subset of what coven-link writes)
#[derive(serde::Deserialize, Default)]
pub struct CovenConfig {
    #[serde(default)]
    pub gateway: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
}

impl CovenConfig {
    pub fn load() -> Self {
        dirs::home_dir()
            .map(|d| d.join(".config/coven/config.toml"))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }
}

/// Normalize gateway address to include scheme
pub fn normalize_gateway(gateway: &str) -> String {
    let g = gateway.trim();
    if g.starts_with("http://") || g.starts_with("https://") {
        return g.to_string();
    }
    // Default to http for gRPC (TLS usually handled at network layer e.g. Tailscale)
    format!("http://{}", g)
}

/// Run an admin command with the given gateway and token
pub async fn run_command(
    command: Command,
    gateway: Option<String>,
    token: Option<String>,
) -> Result<()> {
    // Load config file for defaults
    let config = CovenConfig::load();

    // Determine gateway address (arg > env > config > default)
    let gateway = gateway
        .or_else(|| std::env::var("COVEN_GATEWAY_GRPC").ok())
        .or(config.gateway)
        .map(|g| normalize_gateway(&g))
        .unwrap_or_else(|| "http://localhost:50051".to_string());

    // Determine token (arg > env > config)
    let token = token
        .or_else(|| std::env::var("COVEN_TOKEN").ok())
        .or(config.token);

    match command {
        Command::Me => commands::me::run(&gateway, token.as_deref()).await,
        Command::Agents(cmd) => commands::agents::run(&gateway, token.as_deref(), cmd).await,
        Command::Bindings(cmd) => commands::bindings::run(&gateway, token.as_deref(), cmd).await,
        Command::Principals(cmd) => {
            commands::principals::run(&gateway, token.as_deref(), cmd).await
        }
        Command::Token(cmd) => commands::token::run(&gateway, token.as_deref(), cmd).await,
    }
}
