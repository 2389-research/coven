// ABOUTME: Fold gateway client library shared between iOS and TUI
// ABOUTME: Provides gateway communication, streaming, and state management

mod client;
mod error;
mod models;

pub use client::CovenClient;
pub use error::CovenError;
pub use models::*;

// UniFFI scaffolding
uniffi::include_scaffolding!("coven_client");

// ============================================================================
// Callback Traits
// ============================================================================

/// Callback for streaming events from agents
pub trait StreamCallback: Send + Sync {
    fn on_event(&self, agent_id: String, event: StreamEvent);
}

/// Callback for state changes (for UI updates)
pub trait StateCallback: Send + Sync {
    fn on_connection_status(&self, status: ConnectionStatus);
    fn on_messages_changed(&self, agent_id: String);
    fn on_queue_changed(&self, agent_id: String, count: u32);
    fn on_unread_changed(&self, agent_id: String, count: u32);
    fn on_streaming_changed(&self, agent_id: String, is_streaming: bool);
}
