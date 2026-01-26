// ABOUTME: Agents command implementation.
// ABOUTME: Lists all available agents from the gateway.

use coven_client::CovenClient;
use coven_ssh::default_client_key_path;

use crate::error::{AppError, Result};
use crate::state::config::Config;

pub async fn run(config: &Config) -> Result<()> {
    let key_path = default_client_key_path().ok_or_else(|| {
        AppError::Config("Could not determine SSH key path (HOME not set?)".into())
    })?;

    let client = CovenClient::new_with_auth(config.gateway.url(), &key_path)
        .map_err(|e| AppError::Config(format!("Failed to initialize SSH auth: {}", e)))?;

    match client.refresh_agents_async().await {
        Ok(agents) => {
            if agents.is_empty() {
                println!("No agents available.");
            } else {
                println!("Available agents:");
                for agent in &agents {
                    let status = if agent.connected { "●" } else { "○" };
                    println!("  {} {} ({})", status, agent.name, agent.backend);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to fetch agents from gateway: {}", e);
            eprintln!();
            eprintln!(
                "Is the gateway running at {}:{}?",
                config.gateway.host, config.gateway.port
            );
            eprintln!("Try 'folder doctor' to diagnose connectivity issues.");
            std::process::exit(1);
        }
    }

    Ok(())
}
