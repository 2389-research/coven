// ABOUTME: Defines the AppEvent enum for inter-component communication.
// ABOUTME: All business logic events flow through this enum.

#![allow(dead_code)]

use fold_client::{Agent, ConnectionStatus, Message, StreamEvent};

#[derive(Debug)]
pub enum AppEvent {
    // Client Events
    ConnectionStatus(ConnectionStatus),
    AgentsLoaded(Vec<Agent>),
    MessagesChanged {
        agent_id: String,
    },
    StreamEvent {
        agent_id: String,
        event: StreamEvent,
    },
    StreamingChanged {
        agent_id: String,
        is_streaming: bool,
    },
    QueueChanged {
        agent_id: String,
        count: u32,
    },
    UnreadChanged {
        agent_id: String,
        count: u32,
    },

    // User Actions
    SendMessage {
        content: String,
    },
    SelectAgent {
        agent_id: String,
    },
    OpenPicker,
    ClosePicker,
    CancelStream,

    // Navigation
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,

    // App Lifecycle
    RequestQuit,
    ConfirmQuit,
    ForceQuit,

    // Background Tasks
    HistoryLoaded {
        agent_id: String,
        messages: Vec<Message>,
    },
    HistoryLoadFailed {
        agent_id: String,
        error: String,
    },
    HealthCheckComplete {
        ok: bool,
    },

    // UI
    RequestRedraw,
    ThrobberTick,
}
