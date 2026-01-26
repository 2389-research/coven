// ABOUTME: Bootstrap wizard for fold-swarm configuration.
// ABOUTME: Guides user through gateway URL, prefix, SSH key, and working directory.

use anyhow::{Context, Result};
use fold_swarm_core::{BackendType, Config};
use std::io::{self, Write};
use std::path::PathBuf;

fn prompt(message: &str, default: &str) -> Result<String> {
    print!("{} [{}]: ", message, default);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let input = input.trim();
    if input.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(input.to_string())
    }
}

fn prompt_choice(message: &str, choices: &[&str], default: usize) -> Result<usize> {
    println!("{}:", message);
    for (i, choice) in choices.iter().enumerate() {
        let marker = if i == default { "*" } else { " " };
        println!("  {} [{}] {}", marker, i + 1, choice);
    }

    print!("Choice [{}]: ", default + 1);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let input = input.trim();
    if input.is_empty() {
        Ok(default)
    } else {
        let choice: usize = input.parse().context("Invalid choice")?;
        if choice < 1 || choice > choices.len() {
            anyhow::bail!("Choice must be between 1 and {}", choices.len());
        }
        Ok(choice - 1)
    }
}

pub fn run_init() -> Result<()> {
    println!("fold-swarm initialization\n");

    let gateway_host = prompt("Gateway host", "localhost")?;
    let gateway_port = prompt("Gateway port", "50051")?;
    let gateway_url = format!("grpc://{}:{}", gateway_host, gateway_port);

    let hostname = hostname::get()?.to_string_lossy().to_string();
    let prefix = prompt("Agent prefix", &hostname)?;

    let default_workdir = dirs::home_dir()
        .map(|h| h.join("workspaces").display().to_string())
        .unwrap_or_else(|| "~/workspaces".to_string());
    let working_directory = prompt("Working directory", &default_workdir)?;

    let backend_choices = ["acp", "mux", "direct"];
    let backend_idx = prompt_choice("Default backend", &backend_choices, 0)?;
    let default_backend = match backend_idx {
        0 => BackendType::Acp,
        1 => BackendType::Mux,
        _ => BackendType::Direct,
    };

    let config = Config {
        gateway_url,
        prefix: prefix.clone(),
        working_directory: working_directory.clone(),
        default_backend,
        acp_binary: "claude".to_string(),
    };

    // Save config
    let config_path = Config::default_path()?;
    config.save(&config_path)?;
    println!("\nConfig written to {}", config_path.display());

    // Create dispatch workspace
    let working_dir = PathBuf::from(shellexpand::tilde(&working_directory).into_owned());
    let dispatch_dir = working_dir.join("dispatch");
    std::fs::create_dir_all(&dispatch_dir)?;
    println!("Created dispatch workspace at {}", dispatch_dir.display());

    println!("\nReady to run: fold-swarm supervisor");

    Ok(())
}
