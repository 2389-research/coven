// ABOUTME: Entry point for coven-telegram-bridge binary.
// ABOUTME: Loads config, connects to Telegram and gateway, runs Long Polling event loop.

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "coven-telegram-bridge")]
#[command(about = "Telegram bridge for coven-gateway using Long Polling")]
struct Cli {
    /// Config file path
    #[arg(short, long, env = "COVEN_TELEGRAM_CONFIG")]
    config: Option<std::path::PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    coven_log::init_for("coven_telegram_rs");

    let cli = Cli::parse();
    coven_telegram_rs::run(cli.config).await
}
