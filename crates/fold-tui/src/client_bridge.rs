// ABOUTME: Bridges fold-client callbacks to AppEvent.
// ABOUTME: Implements StreamCallback and StateCallback traits.

#![allow(dead_code)]

use std::sync::Arc;

use fold_client::{ConnectionStatus, StateCallback, StreamCallback, StreamEvent};
use tokio::sync::mpsc::UnboundedSender;

use crate::app_event::AppEvent;

pub struct ClientBridge {
    tx: UnboundedSender<AppEvent>,
}

impl ClientBridge {
    pub fn new(tx: UnboundedSender<AppEvent>) -> Self {
        Self { tx }
    }

    /// Convert the bridge into callback trait objects for fold-client.
    /// Returns (StreamCallback, StateCallback) pair.
    pub fn into_callbacks(self) -> (Arc<dyn StreamCallback>, Arc<dyn StateCallback>) {
        let bridge = Arc::new(self);
        (
            bridge.clone() as Arc<dyn StreamCallback>,
            bridge as Arc<dyn StateCallback>,
        )
    }
}

impl StreamCallback for ClientBridge {
    fn on_event(&self, agent_id: String, event: StreamEvent) {
        let _ = self.tx.send(AppEvent::StreamEvent { agent_id, event });
    }
}

impl StateCallback for ClientBridge {
    fn on_connection_status(&self, status: ConnectionStatus) {
        let _ = self.tx.send(AppEvent::ConnectionStatus(status));
    }

    fn on_messages_changed(&self, agent_id: String) {
        let _ = self.tx.send(AppEvent::MessagesChanged { agent_id });
    }

    fn on_queue_changed(&self, agent_id: String, count: u32) {
        let _ = self.tx.send(AppEvent::QueueChanged { agent_id, count });
    }

    fn on_unread_changed(&self, agent_id: String, count: u32) {
        let _ = self.tx.send(AppEvent::UnreadChanged { agent_id, count });
    }

    fn on_streaming_changed(&self, agent_id: String, is_streaming: bool) {
        let _ = self.tx.send(AppEvent::StreamingChanged {
            agent_id,
            is_streaming,
        });
    }
}
