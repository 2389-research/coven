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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("coven_telegram_rs=info".parse()?),
        )
        .init();

    let cli = Cli::parse();
    coven_telegram_rs::run(cli.config).await
}
