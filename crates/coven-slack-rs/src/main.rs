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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("coven_slack_rs=info".parse()?),
        )
        .init();

    let cli = Cli::parse();
    coven_slack_rs::run(cli.config).await
}
