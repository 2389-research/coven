// ABOUTME: Workspace agent implementation.
// ABOUTME: Connects to fold-gateway, handles prompts via backend.

pub mod grpc;
pub mod session;

pub use grpc::GatewayClient;
pub use session::Session;
