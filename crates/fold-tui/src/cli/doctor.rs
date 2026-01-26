// ABOUTME: Doctor command implementation.
// ABOUTME: Checks gateway connectivity and lists agents.

use fold_client::FoldClient;
use fold_ssh::default_client_key_path;

use crate::error::{AppError, Result};
use crate::state::config::Config;

pub async fn run(config: &Config) -> Result<()> {
    println!("Checking fold gateway...");
    println!(
        "Gateway: {}:{} (gRPC)",
        config.gateway.host, config.gateway.port
    );
    println!();

    let key_path = default_client_key_path().ok_or_else(|| {
        AppError::Config("Could not determine SSH key path (HOME not set?)".into())
    })?;

    let client = FoldClient::new_with_auth(config.gateway.url(), &key_path)
        .map_err(|e| AppError::Config(format!("Failed to initialize SSH auth: {}", e)))?;

    // Check health
    print!("Health check... ");
    match client.check_health_async().await {
        Ok(()) => println!("OK"),
        Err(e) => {
            println!("FAILED");
            println!("Error: {}", e);
            return Ok(());
        }
    }

    // List agents
    print!("Fetching agents... ");
    match client.refresh_agents_async().await {
        Ok(agents) => {
            println!("OK ({} agents)", agents.len());
            println!();

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
            println!("FAILED");
            println!("Error: {}", e);
        }
    }

    Ok(())
}
