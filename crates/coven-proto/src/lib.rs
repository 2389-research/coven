// ABOUTME: Re-exports generated protobuf types for the coven protocol.
// ABOUTME: Single source of truth for coven gRPC services and message types.

#![allow(clippy::derive_partial_eq_without_eq)]

/// Generated protobuf types for the coven protocol.
pub mod coven {
    tonic::include_proto!("coven");
}

// Re-export commonly used types at crate root for convenience
pub use coven::*;

// Re-export client types under a client module
pub mod client {
    pub use super::coven::admin_service_client::AdminServiceClient;
    pub use super::coven::client_service_client::ClientServiceClient;
    pub use super::coven::coven_control_client::CovenControlClient;
}

// Re-export server types under a server module
pub mod server {
    pub use super::coven::admin_service_server::{AdminService, AdminServiceServer};
    pub use super::coven::client_service_server::{ClientService, ClientServiceServer};
    pub use super::coven::coven_control_server::{CovenControl, CovenControlServer};
    pub use super::coven::pack_service_server::{PackService, PackServiceServer};
}
