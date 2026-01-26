// ABOUTME: Send command implementation for CLI scripting.
// ABOUTME: Sends a message to an agent and prints the complete response (non-streaming).

use coven_client::{CovenClient, StreamCallback, StreamEvent};
use coven_ssh::default_client_key_path;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

use crate::error::{AppError, Result};
use crate::state::config::Config;

/// Shared state for capturing the streaming response
struct ResponseCapture {
    buffer: String,
    error: Option<String>,
    done_tx: Option<oneshot::Sender<()>>,
}

/// Callback implementation that buffers the response
struct SendCallback {
    state: Arc<Mutex<ResponseCapture>>,
}

impl StreamCallback for SendCallback {
    fn on_event(&self, _agent_id: String, event: StreamEvent) {
        let mut state = self.state.lock().expect("response capture lock poisoned");
        match event {
            StreamEvent::Text { content } => {
                state.buffer.push_str(&content);
            }
            StreamEvent::Error { message } => {
                state.error = Some(message);
                // Signal completion on error
                if let Some(tx) = state.done_tx.take() {
                    let _ = tx.send(());
                }
            }
            StreamEvent::Done => {
                // Signal completion
                if let Some(tx) = state.done_tx.take() {
                    let _ = tx.send(());
                }
            }
            // Ignore other events for CLI output
            _ => {}
        }
    }
}

/// Find an agent by name (case-insensitive, partial match supported)
fn find_agent(agents: &[coven_client::Agent], query: &str) -> Option<coven_client::Agent> {
    let query_lower = query.to_lowercase();

    // First, try exact match (case-insensitive)
    if let Some(agent) = agents.iter().find(|a| a.name.to_lowercase() == query_lower) {
        return Some(agent.clone());
    }

    // Then try ID exact match
    if let Some(agent) = agents.iter().find(|a| a.id == query) {
        return Some(agent.clone());
    }

    // Finally, try partial name match (prefix)
    if let Some(agent) = agents
        .iter()
        .find(|a| a.name.to_lowercase().starts_with(&query_lower))
    {
        return Some(agent.clone());
    }

    None
}

/// Build an agent hint string for error messages
fn build_agent_hint(agents: &[coven_client::Agent]) -> String {
    if agents.is_empty() {
        "No agents are currently available.".to_string()
    } else {
        let mut hint = String::from("Available agents:");
        for agent in agents {
            let status = if agent.connected { "+" } else { "-" };
            hint.push_str(&format!("\n  {} {} ({})", status, agent.name, agent.id));
        }
        hint
    }
}

/// Run the send command - sends a message and prints the response
pub async fn run(config: &Config, agent_query: &str, message: &str) -> Result<()> {
    let key_path = default_client_key_path().ok_or_else(|| {
        AppError::Config("Could not determine SSH key path (HOME not set?)".into())
    })?;

    let client = CovenClient::new_with_auth(config.gateway.url(), &key_path)
        .map_err(|e| AppError::Config(format!("Failed to initialize SSH auth: {}", e)))?;

    // Fetch available agents
    let agents = client
        .refresh_agents_async()
        .await
        .map_err(|e| AppError::GatewayConnection {
            message: e.to_string(),
            url: config.gateway.url(),
        })?;

    // Find the requested agent
    let agent = find_agent(&agents, agent_query).ok_or_else(|| AppError::AgentNotFound {
        name: agent_query.to_string(),
        hint: build_agent_hint(&agents),
    })?;

    // Check if agent is connected
    if !agent.connected {
        return Err(AppError::AgentNotConnected {
            name: agent.name.clone(),
        });
    }

    // Set up response capture
    let (done_tx, done_rx) = oneshot::channel();
    let capture = Arc::new(Mutex::new(ResponseCapture {
        buffer: String::new(),
        error: None,
        done_tx: Some(done_tx),
    }));

    let callback = SendCallback {
        state: capture.clone(),
    };
    client.set_stream_callback(Box::new(callback));

    // Send the message
    client
        .send_message(agent.id.clone(), message.to_string())
        .map_err(|e| AppError::MessageSend(e.to_string()))?;

    // Wait for response to complete
    let _ = done_rx.await;

    // Get the result
    let state = capture.lock().expect("response capture lock poisoned");

    if let Some(ref error) = state.error {
        return Err(AppError::ResponseError(error.clone()));
    }

    if state.buffer.is_empty() {
        eprintln!("(No response received)");
    } else {
        // Print just the response text for scripting compatibility
        print!("{}", state.buffer);
        // Add newline if response doesn't end with one
        if !state.buffer.ends_with('\n') {
            println!();
        }
    }

    Ok(())
}
