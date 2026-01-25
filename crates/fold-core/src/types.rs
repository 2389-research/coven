// ABOUTME: Core types for fold - Thread, IncomingMessage, OutgoingEvent
// ABOUTME: These are the fundamental data structures that flow through the system

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A thread is an independent conversation with its own Claude session.
/// Maps a frontend-specific ID to a Claude session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    /// Opaque identifier from the frontend (e.g., "slack:C123:1234567890.123")
    pub id: String,
    /// Claude SDK session ID for this conversation
    pub claude_session_id: String,
    /// When this thread was created
    pub created_at: DateTime<Utc>,
    /// Last message activity
    pub last_active: DateTime<Utc>,
}

/// A file attachment (downloaded to local temp storage)
#[derive(Debug, Clone)]
pub struct FileAttachment {
    /// Local path to the downloaded file
    pub path: PathBuf,
    /// Original filename from the sender
    pub filename: String,
    /// MIME type (e.g., "image/png", "text/plain")
    pub mime_type: String,
    /// File size in bytes
    pub size: u64,
}

/// A message coming in from any frontend
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    /// Thread identifier (frontend determines format)
    pub thread_id: String,
    /// Who sent this message
    pub sender: String,
    /// The message content
    pub content: String,
    /// Which frontend this came from ("slack", "tui", "matrix")
    pub frontend: String,
    /// Optional file attachments (downloaded to temp storage)
    pub attachments: Vec<FileAttachment>,
}

/// Events sent back to the frontend
#[derive(Debug, Clone)]
pub enum OutgoingEvent {
    /// Claude is thinking (processing the request)
    Thinking,
    /// Streaming text chunk
    Text(String),
    /// Claude is invoking a tool
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
        state: String, // "pending", "awaiting_approval", "running", "completed", "failed", "denied", "timeout", "cancelled"
        detail: Option<String>,
    },
    /// Response complete
    Done { full_response: String },
    /// Something went wrong
    Error(String),
    /// A file to send to the user
    File {
        path: PathBuf,
        filename: String,
        mime_type: String,
    },
    /// Backend session initialized (session_id assigned/confirmed)
    SessionInit { session_id: String },
    /// Backend session was orphaned (expired, needs retry)
    SessionOrphaned,
}
