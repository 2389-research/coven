// ABOUTME: Core types for coven-tui-v2
// ABOUTME: Mode, Agent, Message, StreamingMessage, and metadata types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Application mode / screen state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Selecting an agent from the picker
    Picker,
    /// Normal chat view
    Chat,
    /// Message in flight - input disabled
    Sending,
}

/// Role in a conversation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    System,
}

/// Tool execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Complete,
    Error,
}

/// An agent available through the gateway
#[derive(Debug, Clone)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub model: Option<String>,
    pub working_dir: String,
    pub capabilities: Vec<String>,
    pub connected: bool,
}

impl From<coven_client::Agent> for Agent {
    fn from(a: coven_client::Agent) -> Self {
        Self {
            id: a.id,
            name: a.name,
            backend: a.backend.clone(),
            model: Some(a.backend), // Use backend as model for now
            working_dir: a.working_dir,
            capabilities: vec![],
            connected: a.connected,
        }
    }
}

/// A tool being used by the agent
#[derive(Debug, Clone)]
pub struct ToolUse {
    pub name: String,
    pub status: ToolStatus,
}

/// Token counts for a message
#[derive(Debug, Clone, Default)]
pub struct MessageTokens {
    pub input: u32,
    pub output: u32,
}

/// A completed message in the conversation
#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub thinking: Option<String>,
    pub tool_uses: Vec<ToolUse>,
    pub timestamp: DateTime<Utc>,
    pub tokens: Option<MessageTokens>,
}

impl Message {
    pub fn user(content: String) -> Self {
        Self {
            role: Role::User,
            content,
            thinking: None,
            tool_uses: vec![],
            timestamp: Utc::now(),
            tokens: None,
        }
    }

    pub fn assistant(content: String) -> Self {
        Self {
            role: Role::Assistant,
            content,
            thinking: None,
            tool_uses: vec![],
            timestamp: Utc::now(),
            tokens: None,
        }
    }
}

/// A message currently being streamed
#[derive(Debug, Clone, Default)]
pub struct StreamingMessage {
    pub content: String,
    pub thinking: Option<String>,
    pub tool_uses: Vec<ToolUse>,
}

/// Session-level metadata
#[derive(Debug, Clone, Default)]
pub struct SessionMetadata {
    pub thread_id: String,
    pub model: String,
    pub working_dir: Option<String>,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub total_cost: f64,
}

/// Persisted state between sessions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedState {
    pub last_agent: Option<String>,
    pub input_history: Vec<String>,
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub gateway_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            gateway_url: "http://localhost:7777".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_equality() {
        assert_eq!(Mode::Picker, Mode::Picker);
        assert_ne!(Mode::Picker, Mode::Chat);
    }

    #[test]
    fn test_message_user() {
        let msg = Message::user("hello".to_string());
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "hello");
        assert!(msg.thinking.is_none());
    }

    #[test]
    fn test_message_assistant() {
        let msg = Message::assistant("hi".to_string());
        assert_eq!(msg.role, Role::Assistant);
    }

    #[test]
    fn test_streaming_message_default() {
        let sm = StreamingMessage::default();
        assert!(sm.content.is_empty());
        assert!(sm.thinking.is_none());
        assert!(sm.tool_uses.is_empty());
    }
}
