// ABOUTME: Non-interactive send command for scripting.
// ABOUTME: Streams response text to stdout.

use std::io::Write;
use std::path::Path;
use std::sync::mpsc;

use anyhow::{Context, Result};
use coven_client::{ConnectionStatus, CovenClient, StateCallback, StreamCallback, StreamEvent};
use coven_link::config::CovenConfig;

use crate::types::PersistedState;

/// Bridge to receive stream events on a channel
struct SendCallbackBridge {
    tx: mpsc::Sender<StreamEvent>,
}

impl StreamCallback for SendCallbackBridge {
    fn on_event(&self, _agent_id: String, event: StreamEvent) {
        let _ = self.tx.send(event);
    }
}

/// No-op state callback for non-interactive mode
struct NoOpStateCallback;

impl StateCallback for NoOpStateCallback {
    fn on_connection_status(&self, _status: ConnectionStatus) {}
    fn on_messages_changed(&self, _agent_id: String) {}
    fn on_queue_changed(&self, _agent_id: String, _count: u32) {}
    fn on_unread_changed(&self, _agent_id: String, _count: u32) {}
    fn on_streaming_changed(&self, _agent_id: String, _is_streaming: bool) {}
}

/// Run the send command
pub fn run(gateway_url: &str, key_path: &Path, message: &str, agent: Option<&str>) -> Result<()> {
    // Create client
    let client = CovenClient::new_with_auth(gateway_url.to_string(), key_path)
        .map_err(|e| anyhow::anyhow!("Failed to initialize client: {}", e))?;

    // Determine agent to use
    let agent_name = if let Some(name) = agent {
        name.to_string()
    } else {
        // Load last used agent from state (stored in coven config dir)
        let state_path = CovenConfig::config_dir()
            .map(|d| d.join("tui").join("state.json"))
            .ok();

        if let Some(path) = state_path {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(state) = serde_json::from_str::<PersistedState>(&content) {
                    state
                        .last_agent
                        .context("No agent specified and no last agent saved. Use --agent <name>.")?
                } else {
                    anyhow::bail!("No agent specified. Use --agent <name>.");
                }
            } else {
                anyhow::bail!("No agent specified. Use --agent <name>.");
            }
        } else {
            anyhow::bail!("No agent specified. Use --agent <name>.");
        }
    };

    // Set up channel to receive events
    let (tx, rx) = mpsc::channel();
    client.set_stream_callback(Box::new(SendCallbackBridge { tx }));
    client.set_state_callback(Box::new(NoOpStateCallback));

    // Need to refresh agents first so the client knows about them
    let _agents = client
        .refresh_agents()
        .map_err(|e| anyhow::anyhow!("Failed to connect to gateway: {}", e))?;

    // Send the message (this starts streaming in background)
    client
        .send_message(agent_name.clone(), message.to_string())
        .map_err(|e| anyhow::anyhow!("Failed to send message: {}", e))?;

    // Receive and print text events until done
    let mut stdout = std::io::stdout();
    loop {
        match rx.recv() {
            Ok(event) => match event {
                StreamEvent::Text { content } => {
                    print!("{}", content);
                    stdout.flush().ok();
                }
                StreamEvent::Done => {
                    println!();
                    break;
                }
                StreamEvent::Error { message } => {
                    eprintln!("Error: {}", message);
                    std::process::exit(1);
                }
                // Ignore thinking, tool use, usage
                _ => {}
            },
            Err(_) => {
                // Channel closed unexpectedly
                eprintln!("Error: Connection closed unexpectedly");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
