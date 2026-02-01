// ABOUTME: Backend trait defining how coven connects to AI providers
// ABOUTME: Implementations: DirectCli (preferred), Mux (native Rust), ClaudeSdk (legacy)

mod claude_sdk;
mod direct_cli;
mod mux;
mod mux_tools;

pub use claude_sdk::ClaudeSdkBackend;
pub use direct_cli::{DirectCliBackend, DirectCliConfig};
pub use mux::{
    default_dangerous_tools, ApprovalCallback, MuxBackend, MuxConfig, MuxMcpServerConfig,
};

use anyhow::Result;
use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

/// Events emitted by backends during response generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackendEvent {
    /// Model is thinking/processing
    Thinking,
    /// Session initialized with backend's session ID (may differ from requested ID)
    SessionInit { session_id: String },
    /// Session was not found - stored session ID is invalid, will be cleared
    SessionOrphaned,
    /// Text chunk from the model
    Text(String),
    /// Model is invoking a tool
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Result from a tool invocation
    ToolResult {
        id: String,
        output: String,
        is_error: bool,
    },
    /// Request approval before executing a dangerous tool
    ToolApprovalRequest {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Token usage statistics from LLM call
    Usage {
        input_tokens: i32,
        output_tokens: i32,
        cache_read_tokens: i32,
        cache_write_tokens: i32,
        thinking_tokens: i32,
    },
    /// Tool execution state transition
    ToolState {
        id: String,
        state: ToolStateKind,
        detail: Option<String>,
    },
    /// Response complete
    Done { full_response: String },
    /// Error occurred
    Error(String),
}

/// Tool execution lifecycle states
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolStateKind {
    Pending,
    AwaitingApproval,
    Running,
    Completed,
    Failed,
    Denied,
    Timeout,
    Cancelled,
}

/// A backend is an AI provider adapter that handles message processing.
#[async_trait]
pub trait Backend: Send + Sync {
    /// Unique name for this backend
    fn name(&self) -> &'static str;

    /// Send a message and receive a stream of events
    ///
    /// - `session_id`: The session identifier for conversation continuity
    /// - `message`: The user's message content
    /// - `is_new_session`: True if this is the first message in a new session
    async fn send(
        &self,
        session_id: &str,
        message: &str,
        is_new_session: bool,
    ) -> Result<BoxStream<'static, BackendEvent>>;
}
