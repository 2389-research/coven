// ABOUTME: Message handling for incoming agent messages and outgoing human responses.
// ABOUTME: Manages the message queue and communication with the gateway.

use chrono::{DateTime, Utc};
use std::fmt;

/// Events that drive the application state machine
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// Connection status changed
    Connection(ConnectionEvent),
    /// New message received from gateway
    IncomingMessage(IncomingMessageEvent),
    /// Message send completed successfully
    SendComplete(SendCompleteEvent),
    /// Message send failed with error
    SendError(SendErrorEvent),
    /// Terminal input or resize event
    Terminal(TerminalEvent),
}

/// Connection status updates
#[derive(Debug, Clone)]
pub enum ConnectionEvent {
    /// Successfully connected to gateway
    Connected { agent_id: String, server: String },
    /// Disconnected from gateway
    Disconnected { reason: String },
    /// Connection error occurred
    Error { message: String },
}

/// Incoming message from the gateway
#[derive(Debug, Clone)]
pub struct IncomingMessageEvent {
    pub message: Message,
}

/// Message send completed successfully
#[derive(Debug, Clone)]
pub struct SendCompleteEvent {
    /// Thread ID the message was sent to
    pub thread_id: String,
}

/// Message send failed
#[derive(Debug, Clone)]
pub struct SendErrorEvent {
    /// Thread ID that failed
    pub thread_id: String,
    /// Error message
    pub error: String,
}

/// Terminal events (input, resize, etc.)
#[derive(Debug, Clone)]
pub enum TerminalEvent {
    /// Key pressed
    Key(crossterm::event::KeyEvent),
    /// Terminal resized
    Resize { width: u16, height: u16 },
    /// Request redraw
    Redraw,
}

/// Whether a message is incoming from an agent or outgoing from the human
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageDirection {
    Incoming,
    Outgoing,
}

/// Message data structure
#[derive(Debug, Clone)]
pub struct Message {
    /// Unique message ID
    pub id: String,
    /// Thread ID this message belongs to
    pub thread_id: String,
    /// Sender identification
    pub sender: String,
    /// Message content
    pub content: String,
    /// When the message was created
    pub timestamp: DateTime<Utc>,
    /// Whether this message is incoming or outgoing
    pub direction: MessageDirection,
}

impl Message {
    /// Create a new message
    pub fn new(
        id: String,
        thread_id: String,
        sender: String,
        content: String,
        timestamp: DateTime<Utc>,
        direction: MessageDirection,
    ) -> Self {
        Self {
            id,
            thread_id,
            sender,
            content,
            timestamp,
            direction,
        }
    }

    /// Create an outgoing message from the human user
    pub fn outgoing(content: String) -> Self {
        Self {
            id: String::new(),
            thread_id: String::new(),
            sender: "you".to_string(),
            content,
            timestamp: Utc::now(),
            direction: MessageDirection::Outgoing,
        }
    }

    /// Format timestamp for display
    pub fn format_timestamp(&self) -> String {
        self.timestamp.format("%Y-%m-%d %H:%M:%S").to_string()
    }

    /// Format message for display in the UI
    pub fn format_display(&self) -> String {
        match self.direction {
            MessageDirection::Incoming => {
                format!(
                    "[{}] {} > {}",
                    self.format_timestamp(),
                    self.sender,
                    self.content,
                )
            }
            MessageDirection::Outgoing => {
                format!("[{}] you: {}", self.format_timestamp(), self.content,)
            }
        }
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let timestamp = Utc::now();
        let msg = Message::new(
            "msg-123".to_string(),
            "thread-456".to_string(),
            "agent-789".to_string(),
            "Hello, world!".to_string(),
            timestamp,
            MessageDirection::Incoming,
        );

        assert_eq!(msg.id, "msg-123");
        assert_eq!(msg.thread_id, "thread-456");
        assert_eq!(msg.sender, "agent-789");
        assert_eq!(msg.content, "Hello, world!");
        assert_eq!(msg.timestamp, timestamp);
        assert_eq!(msg.direction, MessageDirection::Incoming);
    }

    #[test]
    fn test_message_direction() {
        assert_ne!(MessageDirection::Incoming, MessageDirection::Outgoing);
        assert_eq!(MessageDirection::Incoming, MessageDirection::Incoming);
        assert_eq!(MessageDirection::Outgoing, MessageDirection::Outgoing);
    }

    #[test]
    fn test_message_outgoing() {
        let msg = Message::outgoing("Deploy to staging".to_string());
        assert_eq!(msg.sender, "you");
        assert_eq!(msg.content, "Deploy to staging");
        assert_eq!(msg.direction, MessageDirection::Outgoing);
        assert!(msg.id.is_empty());
    }

    #[test]
    fn test_message_format_timestamp() {
        let timestamp = DateTime::parse_from_rfc3339("2026-02-05T10:23:45Z")
            .unwrap()
            .with_timezone(&Utc);
        let msg = Message::new(
            "msg-1".to_string(),
            "thread-1".to_string(),
            "sender-1".to_string(),
            "Test".to_string(),
            timestamp,
            MessageDirection::Incoming,
        );

        assert_eq!(msg.format_timestamp(), "2026-02-05 10:23:45");
    }

    #[test]
    fn test_message_format_display_incoming() {
        let timestamp = DateTime::parse_from_rfc3339("2026-02-05T10:23:45Z")
            .unwrap()
            .with_timezone(&Utc);
        let msg = Message::new(
            "msg-1".to_string(),
            "thread-xyz".to_string(),
            "agent-x".to_string(),
            "Can you check the deployment?".to_string(),
            timestamp,
            MessageDirection::Incoming,
        );

        let display = msg.format_display();
        assert!(display.contains("[2026-02-05 10:23:45]"));
        assert!(display.contains("agent-x"));
        assert!(display.contains("Can you check the deployment?"));
    }

    #[test]
    fn test_message_format_display_outgoing() {
        let msg = Message::outgoing("Deploy to staging".to_string());
        let display = msg.format_display();
        assert!(display.contains("you:"));
        assert!(display.contains("Deploy to staging"));
    }

    #[test]
    fn test_message_display_trait() {
        let timestamp = Utc::now();
        let msg = Message::new(
            "msg-1".to_string(),
            "thread-1".to_string(),
            "sender-1".to_string(),
            "Test message".to_string(),
            timestamp,
            MessageDirection::Incoming,
        );

        let display_str = format!("{}", msg);
        assert!(display_str.contains("Test message"));
    }

    #[test]
    fn test_connection_event_variants() {
        let connected = ConnectionEvent::Connected {
            agent_id: "agent-123".to_string(),
            server: "gateway-01".to_string(),
        };
        assert!(matches!(connected, ConnectionEvent::Connected { .. }));

        let disconnected = ConnectionEvent::Disconnected {
            reason: "timeout".to_string(),
        };
        assert!(matches!(disconnected, ConnectionEvent::Disconnected { .. }));

        let error = ConnectionEvent::Error {
            message: "connection failed".to_string(),
        };
        assert!(matches!(error, ConnectionEvent::Error { .. }));
    }

    #[test]
    fn test_app_event_variants() {
        let conn_event = AppEvent::Connection(ConnectionEvent::Connected {
            agent_id: "agent-1".to_string(),
            server: "server-1".to_string(),
        });
        assert!(matches!(conn_event, AppEvent::Connection(_)));

        let timestamp = Utc::now();
        let msg = Message::new(
            "msg-1".to_string(),
            "thread-1".to_string(),
            "sender-1".to_string(),
            "content".to_string(),
            timestamp,
            MessageDirection::Incoming,
        );
        let msg_event = AppEvent::IncomingMessage(IncomingMessageEvent { message: msg });
        assert!(matches!(msg_event, AppEvent::IncomingMessage(_)));
    }

    #[test]
    fn test_send_complete_event() {
        let event = SendCompleteEvent {
            thread_id: "thread-123".to_string(),
        };
        assert_eq!(event.thread_id, "thread-123");
    }

    #[test]
    fn test_send_error_event() {
        let event = SendErrorEvent {
            thread_id: "thread-456".to_string(),
            error: "network timeout".to_string(),
        };
        assert_eq!(event.thread_id, "thread-456");
        assert_eq!(event.error, "network timeout");
    }

    #[test]
    fn test_terminal_event_variants() {
        let resize = TerminalEvent::Resize {
            width: 80,
            height: 24,
        };
        assert!(matches!(resize, TerminalEvent::Resize { .. }));

        let redraw = TerminalEvent::Redraw;
        assert!(matches!(redraw, TerminalEvent::Redraw));
    }
}
