// ABOUTME: Entry point for coven-matrix-bridge binary.
// ABOUTME: Loads config, connects to Matrix and gateway, runs bridge loop.

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "coven-matrix-bridge")]
#[command(about = "Matrix bridge for coven-gateway")]
struct Cli {
    /// Config file path
    #[arg(short, long, env = "COVEN_MATRIX_CONFIG")]
    config: Option<std::path::PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("coven_matrix_rs=info".parse()?),
        )
        .init();

    let cli = Cli::parse();
    coven_matrix_rs::run(cli.config).await
}
