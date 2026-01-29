// ABOUTME: Thin wrapper around coven-client for TUI use
// ABOUTME: Bridges callback-based API to channels

use crate::types::Agent;
use anyhow::{anyhow, Result};
use coven_client::{ConnectionStatus, CovenClient, StateCallback, StreamCallback, StreamEvent};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Response events sent through channel
#[derive(Debug, Clone)]
pub enum Response {
    Text(String),
    Thinking(String),
    ToolStart(String),
    ToolComplete(String),
    ToolError(String, String),
    Usage { input: u32, output: u32 },
    WorkingDir(String),
    Done,
    Error(String),
}

/// State change events
#[derive(Debug, Clone)]
pub enum StateChange {
    ConnectionStatus(bool),
    StreamingChanged(String, bool),
    MessagesChanged(String),
}

/// Callback bridge that sends to channels
struct CallbackBridge {
    response_tx: mpsc::Sender<Response>,
    state_tx: mpsc::Sender<StateChange>,
}

impl StreamCallback for CallbackBridge {
    fn on_event(&self, _agent_id: String, event: StreamEvent) {
        let response = match event {
            StreamEvent::Text { content } => Response::Text(content),
            StreamEvent::Thinking { content } => Response::Thinking(content),
            StreamEvent::ToolUse { name, .. } => Response::ToolStart(name),
            StreamEvent::ToolResult { .. } => return, // Handled by ToolState
            StreamEvent::ToolState { state, detail: _ } => {
                // Map state string to our enum
                match state.as_str() {
                    "completed" => return, // Will get tool name from ToolUse
                    "failed" => return,
                    _ => return,
                }
            }
            StreamEvent::Usage { info } => Response::Usage {
                input: info.input_tokens as u32,
                output: info.output_tokens as u32,
            },
            StreamEvent::Done => Response::Done,
            StreamEvent::Error { message } => Response::Error(message),
        };
        let _ = self.response_tx.blocking_send(response);
    }
}

impl StateCallback for CallbackBridge {
    fn on_connection_status(&self, status: ConnectionStatus) {
        let connected = matches!(status, ConnectionStatus::Connected);
        let _ = self
            .state_tx
            .blocking_send(StateChange::ConnectionStatus(connected));
    }

    fn on_messages_changed(&self, agent_id: String) {
        let _ = self
            .state_tx
            .blocking_send(StateChange::MessagesChanged(agent_id));
    }

    fn on_queue_changed(&self, _agent_id: String, _count: u32) {}

    fn on_unread_changed(&self, _agent_id: String, _count: u32) {}

    fn on_streaming_changed(&self, agent_id: String, is_streaming: bool) {
        let _ = self
            .state_tx
            .blocking_send(StateChange::StreamingChanged(agent_id, is_streaming));
    }
}

/// TUI client wrapper
pub struct Client {
    inner: Arc<CovenClient>,
}

impl Client {
    pub fn new(gateway_url: &str, ssh_key_path: &Path) -> Result<Self> {
        let inner = CovenClient::new_with_auth(gateway_url.to_string(), ssh_key_path)
            .map_err(|e| anyhow!("Failed to create client: {}", e))?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    pub fn setup_callbacks(
        &self,
        response_tx: mpsc::Sender<Response>,
        state_tx: mpsc::Sender<StateChange>,
    ) {
        let bridge = CallbackBridge {
            response_tx,
            state_tx,
        };
        // Note: CovenClient takes Box<dyn Callback>, so we need to clone for each
        // For now, we'll set up stream callback only
        self.inner.set_stream_callback(Box::new(CallbackBridge {
            response_tx: bridge.response_tx.clone(),
            state_tx: bridge.state_tx.clone(),
        }));
        self.inner.set_state_callback(Box::new(bridge));
    }

    pub async fn list_agents(&self) -> Result<Vec<Agent>> {
        let agents = self
            .inner
            .refresh_agents_async()
            .await
            .map_err(|e| anyhow!("Failed to list agents: {}", e))?;
        Ok(agents.into_iter().map(Agent::from).collect())
    }

    pub fn send_message(&self, agent_id: &str, content: &str) -> Result<()> {
        self.inner
            .send_message(agent_id.to_string(), content.to_string())
            .map_err(|e| anyhow!("Failed to send message: {}", e))
    }

    pub fn get_session_usage(&self) -> (u32, u32) {
        let usage = self.inner.get_session_usage();
        (usage.input_tokens as u32, usage.output_tokens as u32)
    }

    pub fn check_health(&self) -> Result<()> {
        self.inner
            .check_health()
            .map_err(|e| anyhow!("Health check failed: {}", e))
    }
}

impl Clone for Client {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
