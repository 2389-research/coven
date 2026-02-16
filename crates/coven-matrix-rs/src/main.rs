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

    /// Run interactive setup wizard
    #[arg(long)]
    setup: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Run setup wizard if requested (no logging needed)
    if cli.setup {
        return coven_matrix_rs::setup::run_setup().map_err(Into::into);
    }

    // Initialize logging for normal operation
    coven_log::init_for("coven_matrix_rs");

    coven_matrix_rs::run(cli.config).await
}
