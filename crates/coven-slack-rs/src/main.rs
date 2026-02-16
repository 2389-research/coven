// ABOUTME: Entry point for coven-slack-bridge binary.
// ABOUTME: Loads config, connects to Slack and gateway, runs Socket Mode event loop.

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "coven-slack-bridge")]
#[command(about = "Slack bridge for coven-gateway using Socket Mode")]
struct Cli {
    /// Config file path
    #[arg(short, long, env = "COVEN_SLACK_CONFIG")]
    config: Option<std::path::PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    coven_log::init_for("coven_slack_rs");

    let cli = Cli::parse();
    coven_slack_rs::run(cli.config).await
}
