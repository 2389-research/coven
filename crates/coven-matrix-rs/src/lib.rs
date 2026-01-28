// ABOUTME: Library root for coven-matrix-rs.
// ABOUTME: Exports bridge, config, and error modules.

pub mod bridge;
pub mod commands;
pub mod config;
pub mod error;
pub mod gateway;
pub mod matrix;

pub use bridge::Bridge;
pub use config::Config;
pub use error::{BridgeError, Result};
pub use gateway::GatewayClient;
pub use matrix::MatrixClient;

use std::path::PathBuf;
use tracing::{error, info};

/// Run the Matrix bridge with the given config path.
pub async fn run(config_path: Option<PathBuf>) -> anyhow::Result<()> {
    info!("coven-matrix-bridge starting");

    // Load configuration
    let config = Config::load(config_path)?;
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
