// ABOUTME: Data models for coven-client
// ABOUTME: Agent, Message, StreamEvent, and related types with proto conversion

use coven_proto::{AgentInfo, Event};

/// Represents an AI agent available through the gateway
#[derive(Debug, Clone)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub working_dir: String,
    pub connected: bool,
}

impl Agent {
    /// Convert from proto AgentInfo
    pub fn from_proto(proto: AgentInfo) -> Self {
        Self {
            id: proto.id,
            name: proto.name,
            backend: proto.backend,
            working_dir: proto.working_dir,
            connected: proto.connected,
        }
    }
}

/// A chat message (user or agent)
#[derive(Debug, Clone)]
pub struct Message {
    pub id: String,
    pub sender: String,
    pub content: String,
    pub timestamp: i64,
    pub is_user: bool,
}

impl Message {
    pub fn user(content: String) -> Self {
        Self {
            id: format!("user-{}", chrono::Utc::now().timestamp_millis()),
            sender: "You".into(),
            content,
            timestamp: chrono::Utc::now().timestamp_millis(),
            is_user: true,
        }
    }

    pub fn agent(sender: String, content: String) -> Self {
        Self {
            id: format!("agent-{}", chrono::Utc::now().timestamp_millis()),
            sender,
            content,
            timestamp: chrono::Utc::now().timestamp_millis(),
            is_user: false,
        }
    }

    pub fn system(content: String) -> Self {
        Self {
            id: format!("sys-{}", chrono::Utc::now().timestamp_millis()),
            sender: "System".into(),
            content,
            timestamp: chrono::Utc::now().timestamp_millis(),
            is_user: false,
        }
    }

    /// Convert from proto Event (ledger event)
    pub fn from_event(event: Event, agent_name: &str) -> Option<Self> {
        // Only convert message-type events with text content
        if event.r#type != "message" {
            return None;
        }

        let text = event.text?;
        if text.is_empty() {
            return None;
        }

        let is_user = event.direction == "inbound_to_agent";
        let timestamp = chrono::DateTime::parse_from_rfc3339(&event.timestamp)
            .map(|dt| dt.timestamp_millis())
            .unwrap_or_else(|_| chrono::Utc::now().timestamp_millis());

        Some(Self {
            id: event.id,
            sender: if is_user {
                "You".into()
            } else {
                agent_name.into()
            },
            content: text,
            timestamp,
            is_user,
        })
    }
}

/// Token usage information
#[derive(Debug, Clone, Default)]
pub struct UsageInfo {
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cache_read_tokens: i32,
    pub cache_write_tokens: i32,
    pub thinking_tokens: i32,
}

impl UsageInfo {
    pub fn accumulate(&mut self, other: &UsageInfo) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.thinking_tokens += other.thinking_tokens;
    }
}

/// Events received during streaming response
#[derive(Debug, Clone)]
pub enum StreamEvent {
    Text { content: String },
    Thinking { content: String },
    ToolUse { name: String, input: String },
    ToolResult { tool_id: String, result: String },
    ToolState { state: String, detail: String },
    Usage { info: UsageInfo },
    Done,
    Error { message: String },
}

/// Gateway connection status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Connecting,
    Connected,
    Disconnected,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_from_proto() {
        let proto = AgentInfo {
            id: "agent-123".to_string(),
            name: "TestAgent".to_string(),
            backend: "claude".to_string(),
            working_dir: "/home/user".to_string(),
            connected: true,
            metadata: None,
        };

        let agent = Agent::from_proto(proto);

        assert_eq!(agent.id, "agent-123");
        assert_eq!(agent.name, "TestAgent");
        assert_eq!(agent.backend, "claude");
        assert_eq!(agent.working_dir, "/home/user");
        assert!(agent.connected);
    }

    #[test]
    fn test_agent_from_proto_disconnected() {
        let proto = AgentInfo {
            id: "agent-456".to_string(),
            name: "OfflineAgent".to_string(),
            backend: "gemini".to_string(),
            working_dir: "/tmp".to_string(),
            connected: false,
            metadata: None,
        };

        let agent = Agent::from_proto(proto);

        assert_eq!(agent.id, "agent-456");
        assert!(!agent.connected);
    }

    #[test]
    fn test_message_user() {
        let msg = Message::user("Hello, world!".to_string());

        assert!(msg.id.starts_with("user-"));
        assert_eq!(msg.sender, "You");
        assert_eq!(msg.content, "Hello, world!");
        assert!(msg.is_user);
        assert!(msg.timestamp > 0);
    }

    #[test]
    fn test_message_agent() {
        let msg = Message::agent("Claude".to_string(), "Hi there!".to_string());

        assert!(msg.id.starts_with("agent-"));
        assert_eq!(msg.sender, "Claude");
        assert_eq!(msg.content, "Hi there!");
        assert!(!msg.is_user);
        assert!(msg.timestamp > 0);
    }

    #[test]
    fn test_message_system() {
        let msg = Message::system("Connection lost".to_string());

        assert!(msg.id.starts_with("sys-"));
        assert_eq!(msg.sender, "System");
        assert_eq!(msg.content, "Connection lost");
        assert!(!msg.is_user);
        assert!(msg.timestamp > 0);
    }

    #[test]
    fn test_message_from_event_inbound() {
        let event = Event {
            id: "evt-001".to_string(),
            r#type: "message".to_string(),
            direction: "inbound_to_agent".to_string(),
            timestamp: "2024-01-15T10:30:00Z".to_string(),
            text: Some("User message".to_string()),
            ..Default::default()
        };

        let msg = Message::from_event(event, "TestAgent");

        assert!(msg.is_some());
        let msg = msg.unwrap();
        assert_eq!(msg.id, "evt-001");
        assert_eq!(msg.sender, "You");
        assert_eq!(msg.content, "User message");
        assert!(msg.is_user);
    }

    #[test]
    fn test_message_from_event_outbound() {
        let event = Event {
            id: "evt-002".to_string(),
            r#type: "message".to_string(),
            direction: "outbound_from_agent".to_string(),
            timestamp: "2024-01-15T10:31:00Z".to_string(),
            text: Some("Agent response".to_string()),
            ..Default::default()
        };

        let msg = Message::from_event(event, "Claude");

        assert!(msg.is_some());
        let msg = msg.unwrap();
        assert_eq!(msg.id, "evt-002");
        assert_eq!(msg.sender, "Claude");
        assert_eq!(msg.content, "Agent response");
        assert!(!msg.is_user);
    }

    #[test]
    fn test_message_from_event_non_message_type() {
        let event = Event {
            id: "evt-003".to_string(),
            r#type: "tool_use".to_string(),
            direction: "outbound_from_agent".to_string(),
            timestamp: "2024-01-15T10:32:00Z".to_string(),
            text: Some("Some text".to_string()),
            ..Default::default()
        };

        let msg = Message::from_event(event, "Agent");
        assert!(msg.is_none());
    }

    #[test]
    fn test_message_from_event_no_text() {
        let event = Event {
            id: "evt-004".to_string(),
            r#type: "message".to_string(),
            direction: "inbound_to_agent".to_string(),
            timestamp: "2024-01-15T10:33:00Z".to_string(),
            text: None,
            ..Default::default()
        };

        let msg = Message::from_event(event, "Agent");
        assert!(msg.is_none());
    }

    #[test]
    fn test_message_from_event_empty_text() {
        let event = Event {
            id: "evt-005".to_string(),
            r#type: "message".to_string(),
            direction: "inbound_to_agent".to_string(),
            timestamp: "2024-01-15T10:34:00Z".to_string(),
            text: Some("".to_string()),
            ..Default::default()
        };

        let msg = Message::from_event(event, "Agent");
        assert!(msg.is_none());
    }

    #[test]
    fn test_message_from_event_invalid_timestamp() {
        let event = Event {
            id: "evt-006".to_string(),
            r#type: "message".to_string(),
            direction: "inbound_to_agent".to_string(),
            timestamp: "not-a-valid-timestamp".to_string(),
            text: Some("Test".to_string()),
            ..Default::default()
        };

        let msg = Message::from_event(event, "Agent");
        assert!(msg.is_some());
        // Should use current time as fallback
        let msg = msg.unwrap();
        assert!(msg.timestamp > 0);
    }

    #[test]
    fn test_usage_info_default() {
        let usage = UsageInfo::default();

        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_write_tokens, 0);
        assert_eq!(usage.thinking_tokens, 0);
    }

    #[test]
    fn test_usage_info_accumulate() {
        let mut usage = UsageInfo {
            input_tokens: 10,
            output_tokens: 20,
            cache_read_tokens: 5,
            cache_write_tokens: 3,
            thinking_tokens: 2,
        };

        let other = UsageInfo {
            input_tokens: 15,
            output_tokens: 25,
            cache_read_tokens: 8,
            cache_write_tokens: 4,
            thinking_tokens: 6,
        };

        usage.accumulate(&other);

        assert_eq!(usage.input_tokens, 25);
        assert_eq!(usage.output_tokens, 45);
        assert_eq!(usage.cache_read_tokens, 13);
        assert_eq!(usage.cache_write_tokens, 7);
        assert_eq!(usage.thinking_tokens, 8);
    }

    #[test]
    fn test_usage_info_accumulate_multiple() {
        let mut usage = UsageInfo::default();

        let u1 = UsageInfo {
            input_tokens: 100,
            output_tokens: 200,
            cache_read_tokens: 50,
            cache_write_tokens: 25,
            thinking_tokens: 10,
        };

        let u2 = UsageInfo {
            input_tokens: 150,
            output_tokens: 300,
            cache_read_tokens: 75,
            cache_write_tokens: 30,
            thinking_tokens: 15,
        };

        usage.accumulate(&u1);
        usage.accumulate(&u2);

        assert_eq!(usage.input_tokens, 250);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 125);
        assert_eq!(usage.cache_write_tokens, 55);
        assert_eq!(usage.thinking_tokens, 25);
    }

    #[test]
    fn test_stream_event_variants() {
        // Test that all variants can be created
        let text = StreamEvent::Text {
            content: "hello".to_string(),
        };
        let thinking = StreamEvent::Thinking {
            content: "thinking...".to_string(),
        };
        let tool_use = StreamEvent::ToolUse {
            name: "search".to_string(),
            input: "{}".to_string(),
        };
        let tool_result = StreamEvent::ToolResult {
            tool_id: "tool-1".to_string(),
            result: "found".to_string(),
        };
        let tool_state = StreamEvent::ToolState {
            state: "running".to_string(),
            detail: "50%".to_string(),
        };
        let usage = StreamEvent::Usage {
            info: UsageInfo::default(),
        };
        let done = StreamEvent::Done;
        let error = StreamEvent::Error {
            message: "oops".to_string(),
        };

        // Verify Debug is implemented
        assert!(format!("{:?}", text).contains("Text"));
        assert!(format!("{:?}", thinking).contains("Thinking"));
        assert!(format!("{:?}", tool_use).contains("ToolUse"));
        assert!(format!("{:?}", tool_result).contains("ToolResult"));
        assert!(format!("{:?}", tool_state).contains("ToolState"));
        assert!(format!("{:?}", usage).contains("Usage"));
        assert!(format!("{:?}", done).contains("Done"));
        assert!(format!("{:?}", error).contains("Error"));
    }

    #[test]
    fn test_connection_status_equality() {
        assert_eq!(ConnectionStatus::Connecting, ConnectionStatus::Connecting);
        assert_eq!(ConnectionStatus::Connected, ConnectionStatus::Connected);
        assert_eq!(
            ConnectionStatus::Disconnected,
            ConnectionStatus::Disconnected
        );
        assert_ne!(ConnectionStatus::Connecting, ConnectionStatus::Connected);
        assert_ne!(ConnectionStatus::Connected, ConnectionStatus::Disconnected);
    }

    #[test]
    fn test_connection_status_debug() {
        assert!(format!("{:?}", ConnectionStatus::Connecting).contains("Connecting"));
        assert!(format!("{:?}", ConnectionStatus::Connected).contains("Connected"));
        assert!(format!("{:?}", ConnectionStatus::Disconnected).contains("Disconnected"));
    }

    #[test]
    fn test_connection_status_clone() {
        let status = ConnectionStatus::Connected;
        let cloned = status;
        assert_eq!(status, cloned);
    }

    #[test]
    fn test_agent_debug_and_clone() {
        let agent = Agent {
            id: "test".to_string(),
            name: "Test".to_string(),
            backend: "claude".to_string(),
            working_dir: "/tmp".to_string(),
            connected: true,
        };

        // Test Debug
        let debug_str = format!("{:?}", agent);
        assert!(debug_str.contains("test"));
        assert!(debug_str.contains("Test"));

        // Test Clone
        let cloned = agent.clone();
        assert_eq!(cloned.id, agent.id);
        assert_eq!(cloned.name, agent.name);
    }

    #[test]
    fn test_message_debug_and_clone() {
        let msg = Message::user("test".to_string());

        // Test Debug
        let debug_str = format!("{:?}", msg);
        assert!(debug_str.contains("test"));

        // Test Clone
        let cloned = msg.clone();
        assert_eq!(cloned.content, msg.content);
    }

    #[test]
    fn test_usage_info_debug_and_clone() {
        let usage = UsageInfo {
            input_tokens: 100,
            output_tokens: 200,
            cache_read_tokens: 50,
            cache_write_tokens: 25,
            thinking_tokens: 10,
        };

        // Test Debug
        let debug_str = format!("{:?}", usage);
        assert!(debug_str.contains("100"));

        // Test Clone
        let cloned = usage.clone();
        assert_eq!(cloned.input_tokens, usage.input_tokens);
    }

    #[test]
    fn test_stream_event_clone() {
        let event = StreamEvent::Text {
            content: "hello".to_string(),
        };
        let cloned = event.clone();

        if let StreamEvent::Text { content } = cloned {
            assert_eq!(content, "hello");
        } else {
            panic!("Expected Text variant");
        }
    }
}
