// ABOUTME: Local gateway server for coven - "super trusted" mode without authentication
// ABOUTME: Implements CovenControl (agents), ClientService (TUI), and PackService (packs)

pub mod server;
pub mod services;
pub mod store;

use anyhow::Result;
use std::path::PathBuf;

/// Configuration for the local gateway server
#[derive(Debug, Clone)]
pub struct ServeConfig {
    /// gRPC listen address (default: 127.0.0.1:50051)
    pub grpc_addr: String,
    /// SQLite database path (default: ~/.coven/local.db)
    pub db_path: PathBuf,
}

impl Default for ServeConfig {
    fn default() -> Self {
        let db_path = dirs::config_dir()
            .map(|p| p.join("coven").join("local.db"))
            .unwrap_or_else(|| PathBuf::from("local.db"));

        Self {
            grpc_addr: "127.0.0.1:50051".to_string(),
            db_path,
        }
    }
}

/// Run the local gateway server
pub async fn run(config: ServeConfig) -> Result<()> {
    server::run(config).await
}
