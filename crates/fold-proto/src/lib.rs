// ABOUTME: Re-exports generated protobuf types for the fold protocol.
// ABOUTME: Single source of truth for fold gRPC services and message types.

#![allow(clippy::derive_partial_eq_without_eq)]

/// Generated protobuf types for the fold protocol.
pub mod fold {
    tonic::include_proto!("fold");
}

// Re-export commonly used types at crate root for convenience
pub use fold::*;

// Re-export client types under a client module
pub mod client {
    pub use super::fold::admin_service_client::AdminServiceClient;
    pub use super::fold::client_service_client::ClientServiceClient;
    pub use super::fold::fold_control_client::FoldControlClient;
}

// Re-export server types under a server module
pub mod server {
    pub use super::fold::admin_service_server::{AdminService, AdminServiceServer};
    pub use super::fold::client_service_server::{ClientService, ClientServiceServer};
    pub use super::fold::fold_control_server::{FoldControl, FoldControlServer};
}
