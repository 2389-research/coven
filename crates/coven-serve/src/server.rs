// ABOUTME: gRPC server setup and lifecycle for local gateway
// ABOUTME: Combines CovenControl, ClientService, and PackService into a single server

use crate::services::client::ClientServiceImpl;
use crate::services::control::{ControlState, CovenControlService};
use crate::services::pack::{PackServiceImpl, PackState};
use crate::store::Store;
use crate::ServeConfig;
use anyhow::{Context, Result};
use coven_proto::server::{ClientServiceServer, CovenControlServer, PackServiceServer};
use tokio::signal;
use tonic::transport::Server;
use tracing::info;

/// Run the local gateway server
pub async fn run(config: ServeConfig) -> Result<()> {
    info!("Starting local gateway server");
    info!("  gRPC address: {}", config.grpc_addr);
    info!("  Database: {}", config.db_path.display());

    // Open database
    let store = Store::open(&config.db_path)
        .await
        .context("opening database")?;

    // Create shared state
    let control_state = ControlState::new(store.clone());
    let pack_state = PackState::new(store.clone());

    // Create services
    let control_service = CovenControlService::new(control_state.clone());
    let client_service = ClientServiceImpl::new(store.clone(), control_state.clone());
    let pack_service = PackServiceImpl::new(pack_state.clone());

    // Parse address
    let addr = config.grpc_addr.parse().context("parsing gRPC address")?;

    info!("Local gateway listening on {}", addr);
    println!();
    println!("Local coven gateway running!");
    println!("  gRPC: {}", config.grpc_addr);
    println!("  Database: {}", config.db_path.display());
    println!();
    println!("Connect agents with:");
    println!("  coven agent run --server http://{}", config.grpc_addr);
    println!();
    println!("Use TUI with:");
    println!("  COVEN_GATEWAY={} coven chat", config.grpc_addr);
    println!();
    println!("Press Ctrl+C to stop");

    // Build and run server with graceful shutdown
    Server::builder()
        .add_service(CovenControlServer::new(control_service))
        .add_service(ClientServiceServer::new(client_service))
        .add_service(PackServiceServer::new(pack_service))
        .serve_with_shutdown(addr, shutdown_signal())
        .await
        .context("running gRPC server")?;

    info!("Server shut down gracefully");
    println!("\nServer stopped.");

    Ok(())
}

/// Wait for shutdown signal (Ctrl+C or SIGTERM)
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C, shutting down...");
        }
        _ = terminate => {
            info!("Received SIGTERM, shutting down...");
        }
    }
}
