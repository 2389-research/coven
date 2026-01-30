// ABOUTME: Workspace agent implementation.
// ABOUTME: Connects to coven-gateway, handles prompts via backend.

pub mod grpc;
pub mod pack_tool;
pub mod session;

pub use grpc::GatewayClient;
pub use pack_tool::{handle_pack_tool_result, new_pending_pack_tools, PackTool, PendingPackTools};
pub use session::Session;
