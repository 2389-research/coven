// ABOUTME: Entry point for coven-matrix-bridge binary.
// ABOUTME: Loads config, connects to Matrix and gateway, runs bridge loop.

use anyhow::Result;
use clap::Parser;
use coven_matrix_rs::{Bridge, Config};
use tracing::{error, info};

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
    info!("coven-matrix-bridge starting");

    // Load configuration
    let config = Config::load(cli.config)?;
    info!(
        homeserver = %config.matrix.homeserver,
        gateway = %config.gateway.url,
        "Configuration loaded"
    );

    // Create and run the bridge
    let bridge = Bridge::new(config).await?;
    info!("Bridge initialized, starting sync loop");

    // Handle shutdown signals
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install ctrl+c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    // Run bridge until shutdown signal
    tokio::select! {
        result = bridge.run() => {
            if let Err(e) = result {
                error!(error = %e, "Bridge error");
                return Err(e.into());
            }
        }
        _ = ctrl_c => {
            info!("Received Ctrl+C, shutting down");
        }
        _ = terminate => {
            info!("Received terminate signal, shutting down");
        }
    }

    info!("coven-matrix-bridge stopped");
    Ok(())
}
