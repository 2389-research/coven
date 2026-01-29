// ABOUTME: First-time configuration wizard.
// ABOUTME: Prompts for gateway URL and tests connection.

use std::io::{self, Write};

use anyhow::{Context, Result};
use coven_client::CovenClient;
use coven_ssh::default_client_key_path;

use crate::types::Config;

/// Run the setup wizard
pub fn run() -> Result<()> {
    println!("Coven Chat Setup");
    println!("================\n");

    // Prompt for gateway URL
    print!("Gateway URL [http://localhost:7777]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    let gateway_url = if input.is_empty() {
        "http://localhost:7777".to_string()
    } else {
        input.to_string()
    };

    // Test connection
    println!("\nTesting connection to {}...", gateway_url);

    let key_path = default_client_key_path()
        .context("Could not determine SSH key path. Is HOME set?")?;

    let client = CovenClient::new_with_auth(gateway_url.clone(), &key_path)
        .map_err(|e| anyhow::anyhow!("Failed to create client: {}", e))?;

    // Try to list agents as a health check
    match client.refresh_agents() {
        Ok(agents) => {
            println!("Connected successfully!");
            println!("  Found {} agent(s)", agents.len());
        }
        Err(e) => {
            eprintln!("Connection failed: {}", e);
            eprintln!("\nPlease check:");
            eprintln!("  - Is the gateway running?");
            eprintln!("  - Is the URL correct?");
            eprintln!("  - Do you have SSH keys set up?");
            std::process::exit(1);
        }
    }

    // Save config
    let config = Config {
        gateway_url: gateway_url.clone(),
    };
    let config_dir = dirs::config_dir()
        .context("Could not determine config directory")?
        .join("coven-chat");

    std::fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

    let config_path = config_dir.join("config.toml");
    let config_content = toml::to_string_pretty(&config).context("Failed to serialize config")?;

    std::fs::write(&config_path, config_content).context("Failed to write config file")?;

    println!("\nConfiguration saved to {}", config_path.display());
    println!("\nYou're all set! Run 'coven-chat' to start chatting.");

    Ok(())
}
