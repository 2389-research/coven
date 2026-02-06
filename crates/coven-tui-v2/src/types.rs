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
    pub input: String,
    pub result: Option<String>,
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
    /// Ordered blocks of text and tool uses (preserves interleaving)
    pub blocks: Vec<StreamBlock>,
    pub thinking: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub tokens: Option<MessageTokens>,
}

impl Message {
    pub fn user(content: String) -> Self {
        Self {
            role: Role::User,
            blocks: vec![StreamBlock::Text(content)],
            thinking: None,
            timestamp: Utc::now(),
            tokens: None,
        }
    }

    pub fn assistant(content: String) -> Self {
        Self {
            role: Role::Assistant,
            blocks: vec![StreamBlock::Text(content)],
            thinking: None,
            timestamp: Utc::now(),
            tokens: None,
        }
    }

    /// Get all text content concatenated
    pub fn content(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                StreamBlock::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

impl From<coven_client::Message> for Message {
    fn from(m: coven_client::Message) -> Self {
        let role = if m.is_user {
            Role::User
        } else {
            Role::Assistant
        };
        let timestamp = DateTime::from_timestamp_millis(m.timestamp).unwrap_or_else(Utc::now);
        Self {
            role,
            blocks: vec![StreamBlock::Text(m.content)],
            thinking: None,
            timestamp,
            tokens: None,
        }
    }
}

/// A block in a streaming message (text or tool use, in order)
#[derive(Debug, Clone)]
pub enum StreamBlock {
    Text(String),
    Tool(ToolUse),
}

/// A message currently being streamed
#[derive(Debug, Clone, Default)]
pub struct StreamingMessage {
    pub blocks: Vec<StreamBlock>,
    pub thinking: Option<String>,
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

/// A pending tool approval request from an agent
#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub agent_id: String,
    pub request_id: String,
    pub tool_id: String,
    pub tool_name: String,
    pub input_json: String,
    pub timestamp: DateTime<Utc>,
}

/// Decision for a tool approval request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// Approve this single tool use
    Approve,
    /// Deny this tool use
    Deny,
    /// Approve all future uses of this tool from this agent
    ApproveAll,
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
        assert_eq!(msg.content(), "hello");
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
        assert!(sm.blocks.is_empty());
        assert!(sm.thinking.is_none());
    }

    #[test]
    fn test_pending_approval() {
        let approval = PendingApproval {
            agent_id: "agent-1".to_string(),
            request_id: "req-1".to_string(),
            tool_id: "tool-1".to_string(),
            tool_name: "bash".to_string(),
            input_json: r#"{"command": "ls"}"#.to_string(),
            timestamp: Utc::now(),
        };
        assert_eq!(approval.agent_id, "agent-1");
        assert_eq!(approval.tool_name, "bash");
    }

    #[test]
    fn test_approval_decision_equality() {
        assert_eq!(ApprovalDecision::Approve, ApprovalDecision::Approve);
        assert_eq!(ApprovalDecision::Deny, ApprovalDecision::Deny);
        assert_eq!(ApprovalDecision::ApproveAll, ApprovalDecision::ApproveAll);
        assert_ne!(ApprovalDecision::Approve, ApprovalDecision::Deny);
        assert_ne!(ApprovalDecision::Approve, ApprovalDecision::ApproveAll);
    }
}
