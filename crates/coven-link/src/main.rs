// ABOUTME: Entry point for coven-link device linking tool
// ABOUTME: Links this device to a coven-gateway and sets up local config

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "coven-link", about = "Link this device to a coven-gateway")]
struct Cli {
    /// Gateway URL (e.g., https://coven.example.com or http://localhost:8080)
    gateway: String,

    /// Device name (defaults to hostname)
    #[arg(long, short = 'n')]
    name: Option<String>,

    /// Path to SSH key (defaults to ~/.config/coven/device_key)
    #[arg(long)]
    key: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    coven_log::init();

    let cli = Cli::parse();

    coven_link::run(cli.gateway, cli.name, cli.key).await
}
