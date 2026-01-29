# Coven TUI v2 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a simplified, robust TUI for coven using Ratatui, replacing the existing complex implementation.

**Architecture:** Channel-based async with tokio::select! main loop. Input task and coven-client callbacks feed channels. Single App struct holds all state. No event enum explosion.

**Tech Stack:** Rust, Ratatui, Crossterm, tui-textarea, tokio, coven-client, clap

---

## Task 1: Create New Crate Skeleton

**Files:**
- Create: `crates/coven-tui-v2/Cargo.toml`
- Create: `crates/coven-tui-v2/src/lib.rs`
- Create: `crates/coven-tui-v2/src/main.rs`

**Step 1: Create Cargo.toml**

```toml
[package]
name = "coven-tui-v2"
version.workspace = true
edition.workspace = true

[[bin]]
name = "coven-chat"
path = "src/main.rs"

[dependencies]
# TUI
ratatui.workspace = true
crossterm.workspace = true
tui-textarea = "0.7"

# Async
tokio = { workspace = true, features = ["full", "sync", "macros", "rt-multi-thread"] }

# Client
coven-client.workspace = true
coven-ssh.workspace = true

# CLI
clap = { workspace = true, features = ["derive"] }

# Serialization
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true

# Utils
anyhow.workspace = true
chrono = { workspace = true, features = ["serde"] }
dirs = "5"
tracing.workspace = true
```

**Step 2: Create minimal lib.rs**

```rust
// ABOUTME: Coven TUI v2 - simplified terminal interface for coven agents
// ABOUTME: Channel-based async architecture with Ratatui

pub mod app;
pub mod client;
pub mod types;
pub mod ui;

pub mod cli;
```

**Step 3: Create minimal main.rs that compiles**

```rust
// ABOUTME: Entry point for coven-chat TUI
// ABOUTME: Handles CLI args, config loading, and TUI launch

use clap::Parser;

#[derive(Parser)]
#[command(name = "coven-chat")]
#[command(about = "Terminal chat interface for coven agents")]
struct Args {
    /// Agent to start chatting with (skips picker)
    #[arg(short, long)]
    agent: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Send a message non-interactively
    Send {
        /// The message to send
        message: String,
        /// Print the response to stdout
        #[arg(short, long)]
        print: bool,
    },
    /// First-time setup wizard
    Setup,
}

fn main() {
    let _args = Args::parse();
    println!("coven-chat v2 skeleton");
}
```

**Step 4: Verify it compiles**

Run: `cargo build -p coven-tui-v2`
Expected: Successful build

**Step 5: Commit**

```bash
git add crates/coven-tui-v2/
git commit -m "feat(tui-v2): create new crate skeleton"
```

---

## Task 2: Define Core Types

**Files:**
- Create: `crates/coven-tui-v2/src/types.rs`

**Step 1: Write types with tests**

```rust
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
```

**Step 2: Run tests**

Run: `cargo test -p coven-tui-v2`
Expected: All tests pass

**Step 3: Commit**

```bash
git add crates/coven-tui-v2/src/types.rs
git commit -m "feat(tui-v2): add core types"
```

---

## Task 3: Implement Client Wrapper

**Files:**
- Create: `crates/coven-tui-v2/src/client.rs`

**Step 1: Write client wrapper**

```rust
// ABOUTME: Thin wrapper around coven-client for TUI use
// ABOUTME: Bridges callback-based API to channels

use crate::types::Agent;
use anyhow::{anyhow, Result};
use coven_client::{CovenClient, ConnectionStatus, StateCallback, StreamCallback, StreamEvent};
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
```

**Step 2: Verify it compiles**

Run: `cargo build -p coven-tui-v2`
Expected: Successful build

**Step 3: Commit**

```bash
git add crates/coven-tui-v2/src/client.rs
git commit -m "feat(tui-v2): add client wrapper with channel bridge"
```

---

## Task 4: Implement App State and Actions

**Files:**
- Create: `crates/coven-tui-v2/src/app.rs`

**Step 1: Write App struct and methods**

```rust
// ABOUTME: Central application state and event handling
// ABOUTME: Single struct holds all state, mutations happen in handle_* methods

use crate::client::Response;
use crate::types::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;
use std::time::{Duration, Instant};
use tui_textarea::TextArea;

const MAX_HISTORY: usize = 100;

/// Actions that need async handling (returned from handle_key)
pub enum Action {
    Quit,
    SendMessage(String),
    RefreshAgents,
}

/// Central application state
pub struct App {
    // Mode
    pub mode: Mode,

    // Agents
    pub agents: Vec<Agent>,
    pub selected_agent: Option<String>,

    // Chat state
    pub messages: Vec<Message>,
    pub streaming: Option<StreamingMessage>,
    pub scroll_offset: usize,

    // Input state
    pub input: TextArea<'static>,
    pub input_history: Vec<String>,
    pub history_index: Option<usize>,

    // Picker state
    pub picker_filter: String,
    pub picker_index: usize,

    // Session metadata
    pub session: SessionMetadata,

    // Connection
    pub connected: bool,
    pub error: Option<String>,

    // Quit handling
    pub last_ctrl_c: Option<Instant>,

    // Throbber animation frame
    pub throbber_frame: usize,
}

impl App {
    pub fn new(initial_agent: Option<String>) -> Self {
        Self {
            mode: if initial_agent.is_some() {
                Mode::Chat
            } else {
                Mode::Picker
            },
            agents: vec![],
            selected_agent: initial_agent,
            messages: vec![],
            streaming: None,
            scroll_offset: 0,
            input: TextArea::default(),
            input_history: vec![],
            history_index: None,
            picker_filter: String::new(),
            picker_index: 0,
            session: SessionMetadata::default(),
            connected: false,
            error: None,
            last_ctrl_c: None,
            throbber_frame: 0,
        }
    }

    pub fn load(config_dir: &Path, initial_agent: Option<String>) -> Self {
        let state_path = config_dir.join("state.json");
        let persisted: PersistedState = std::fs::read_to_string(&state_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let mut app = Self::new(initial_agent.or(persisted.last_agent));
        app.input_history = persisted.input_history;
        app
    }

    pub fn save(&self, config_dir: &Path) -> anyhow::Result<()> {
        let state_path = config_dir.join("state.json");

        let persisted = PersistedState {
            last_agent: self.selected_agent.clone(),
            input_history: self
                .input_history
                .iter()
                .rev()
                .take(MAX_HISTORY)
                .rev()
                .cloned()
                .collect(),
        };

        std::fs::create_dir_all(config_dir)?;
        std::fs::write(&state_path, serde_json::to_string_pretty(&persisted)?)?;
        Ok(())
    }

    /// Advance throbber animation
    pub fn tick(&mut self) {
        self.throbber_frame = (self.throbber_frame + 1) % 8;
    }

    /// Get current throbber character
    pub fn throbber_char(&self) -> char {
        const THROBBER: [char; 8] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'];
        THROBBER[self.throbber_frame]
    }

    /// Get agents filtered by picker search
    pub fn filtered_agents(&self) -> Vec<&Agent> {
        self.agents
            .iter()
            .filter(|a| {
                a.name
                    .to_lowercase()
                    .contains(&self.picker_filter.to_lowercase())
            })
            .collect()
    }

    /// Handle a key event, returning an action if needed
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Global keys
        match key.code {
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Some(Action::Quit);
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(last) = self.last_ctrl_c {
                    if last.elapsed() < Duration::from_millis(500) {
                        return Some(Action::Quit);
                    }
                }
                self.last_ctrl_c = Some(Instant::now());
                return None;
            }
            KeyCode::Char(' ') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.mode = Mode::Picker;
                self.picker_filter.clear();
                self.picker_index = 0;
                return None;
            }
            _ => {}
        }

        match self.mode {
            Mode::Picker => self.handle_picker_key(key),
            Mode::Chat => self.handle_chat_key(key),
            Mode::Sending => None,
        }
    }

    fn handle_picker_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                if self.selected_agent.is_some() {
                    self.mode = Mode::Chat;
                }
            }
            KeyCode::Enter => {
                let filtered = self.filtered_agents();
                if let Some(agent) = filtered.get(self.picker_index) {
                    self.selected_agent = Some(agent.id.clone());
                    self.session.model = agent.model.clone().unwrap_or_default();
                    self.mode = Mode::Chat;
                    self.messages.clear();
                }
            }
            KeyCode::Up => {
                self.picker_index = self.picker_index.saturating_sub(1);
            }
            KeyCode::Down => {
                let max = self.filtered_agents().len().saturating_sub(1);
                self.picker_index = (self.picker_index + 1).min(max);
            }
            KeyCode::Char(c) => {
                self.picker_filter.push(c);
                self.picker_index = 0;
            }
            KeyCode::Backspace => {
                self.picker_filter.pop();
                self.picker_index = 0;
            }
            _ => {}
        }
        None
    }

    fn handle_chat_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            // Scroll
            KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_add(10);
            }
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
            }

            // History navigation (when input empty)
            KeyCode::Up if self.input.is_empty() => {
                self.navigate_history(-1);
            }
            KeyCode::Down if self.input.is_empty() => {
                self.navigate_history(1);
            }

            // Send message
            KeyCode::Enter if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                let content = self.input.lines().join("\n").trim().to_string();
                if !content.is_empty() && self.selected_agent.is_some() {
                    self.input_history.push(content.clone());
                    self.history_index = None;
                    self.input = TextArea::default();
                    self.mode = Mode::Sending;
                    self.streaming = Some(StreamingMessage::default());
                    self.scroll_offset = 0;
                    self.messages.push(Message::user(content.clone()));
                    return Some(Action::SendMessage(content));
                }
            }

            // Pass to textarea
            _ => {
                self.input.input(key);
            }
        }
        None
    }

    fn navigate_history(&mut self, direction: i32) {
        if self.input_history.is_empty() {
            return;
        }

        let new_index = match self.history_index {
            None if direction < 0 => Some(self.input_history.len() - 1),
            None => None,
            Some(i) => {
                let new = i as i32 + direction;
                if new < 0 || new >= self.input_history.len() as i32 {
                    None
                } else {
                    Some(new as usize)
                }
            }
        };

        self.history_index = new_index;
        self.input = TextArea::default();
        if let Some(i) = new_index {
            for line in self.input_history[i].lines() {
                self.input.insert_str(line);
                self.input.insert_newline();
            }
            // Remove trailing newline
            self.input.delete_char();
        }
    }

    /// Handle a response from the client
    pub fn handle_response(&mut self, response: Response) {
        match response {
            Response::Text(text) => {
                if let Some(streaming) = &mut self.streaming {
                    streaming.content.push_str(&text);
                }
            }
            Response::Thinking(text) => {
                if let Some(streaming) = &mut self.streaming {
                    match &mut streaming.thinking {
                        Some(existing) => existing.push_str(&text),
                        None => streaming.thinking = Some(text),
                    }
                }
            }
            Response::ToolStart(name) => {
                if let Some(streaming) = &mut self.streaming {
                    streaming.tool_uses.push(ToolUse {
                        name,
                        status: ToolStatus::Running,
                    });
                }
            }
            Response::ToolComplete(name) => {
                if let Some(streaming) = &mut self.streaming {
                    if let Some(tool) = streaming.tool_uses.iter_mut().find(|t| t.name == name) {
                        tool.status = ToolStatus::Complete;
                    }
                }
            }
            Response::ToolError(name, _error) => {
                if let Some(streaming) = &mut self.streaming {
                    if let Some(tool) = streaming.tool_uses.iter_mut().find(|t| t.name == name) {
                        tool.status = ToolStatus::Error;
                    }
                }
            }
            Response::Usage { input, output } => {
                self.session.total_input_tokens += input;
                self.session.total_output_tokens += output;
            }
            Response::WorkingDir(dir) => {
                self.session.working_dir = Some(dir);
            }
            Response::Done => {
                if let Some(streaming) = self.streaming.take() {
                    self.messages.push(Message {
                        role: Role::Assistant,
                        content: streaming.content,
                        thinking: streaming.thinking,
                        tool_uses: streaming.tool_uses,
                        timestamp: chrono::Utc::now(),
                        tokens: None,
                    });
                }
                self.mode = Mode::Chat;
            }
            Response::Error(err) => {
                self.error = Some(err);
                self.streaming = None;
                self.mode = Mode::Chat;
            }
        }
    }

    /// Check if Ctrl+C hint should be shown
    pub fn show_ctrl_c_hint(&self) -> bool {
        self.last_ctrl_c
            .map(|t| t.elapsed() < Duration::from_millis(500))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_new() {
        let app = App::new(None);
        assert_eq!(app.mode, Mode::Picker);
        assert!(app.selected_agent.is_none());
    }

    #[test]
    fn test_app_new_with_agent() {
        let app = App::new(Some("agent-1".to_string()));
        assert_eq!(app.mode, Mode::Chat);
        assert_eq!(app.selected_agent, Some("agent-1".to_string()));
    }

    #[test]
    fn test_throbber_cycles() {
        let mut app = App::new(None);
        let first = app.throbber_char();
        for _ in 0..8 {
            app.tick();
        }
        assert_eq!(app.throbber_char(), first);
    }

    #[test]
    fn test_handle_response_text() {
        let mut app = App::new(None);
        app.streaming = Some(StreamingMessage::default());
        app.handle_response(Response::Text("hello ".to_string()));
        app.handle_response(Response::Text("world".to_string()));
        assert_eq!(app.streaming.as_ref().unwrap().content, "hello world");
    }

    #[test]
    fn test_handle_response_done() {
        let mut app = App::new(Some("agent-1".to_string()));
        app.mode = Mode::Sending;
        app.streaming = Some(StreamingMessage {
            content: "test response".to_string(),
            thinking: None,
            tool_uses: vec![],
        });
        app.handle_response(Response::Done);
        assert!(app.streaming.is_none());
        assert_eq!(app.mode, Mode::Chat);
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.messages[0].content, "test response");
    }

    #[test]
    fn test_filtered_agents() {
        let mut app = App::new(None);
        app.agents = vec![
            Agent {
                id: "1".to_string(),
                name: "Claude".to_string(),
                backend: "anthropic".to_string(),
                model: None,
                working_dir: String::new(),
                capabilities: vec![],
                connected: true,
            },
            Agent {
                id: "2".to_string(),
                name: "GPT".to_string(),
                backend: "openai".to_string(),
                model: None,
                working_dir: String::new(),
                capabilities: vec![],
                connected: true,
            },
        ];
        app.picker_filter = "clau".to_string();
        let filtered = app.filtered_agents();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "Claude");
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p coven-tui-v2`
Expected: All tests pass

**Step 3: Commit**

```bash
git add crates/coven-tui-v2/src/app.rs
git commit -m "feat(tui-v2): add App state and event handling"
```

---

## Task 5: Implement UI Rendering

**Files:**
- Create: `crates/coven-tui-v2/src/ui/mod.rs`
- Create: `crates/coven-tui-v2/src/ui/chat.rs`
- Create: `crates/coven-tui-v2/src/ui/input.rs`
- Create: `crates/coven-tui-v2/src/ui/picker.rs`
- Create: `crates/coven-tui-v2/src/ui/status.rs`

**Step 1: Create ui/mod.rs**

```rust
// ABOUTME: UI rendering module for coven-tui-v2
// ABOUTME: Dispatches rendering to widget modules

mod chat;
mod input;
mod picker;
mod status;

use crate::app::App;
use crate::types::Mode;
use ratatui::prelude::*;
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Min(1),    // Chat area
        Constraint::Length(5), // Input area
        Constraint::Length(1), // Status bar
    ])
    .split(f.area());

    chat::render(f, chunks[0], app);
    input::render(f, chunks[1], app);
    status::render(f, chunks[2], app);

    // Picker is an overlay
    if app.mode == Mode::Picker {
        picker::render(f, app);
    }
}
```

**Step 2: Create ui/chat.rs**

```rust
// ABOUTME: Chat history rendering
// ABOUTME: Displays messages and streaming response

use crate::app::App;
use crate::types::{Mode, Role, ToolStatus};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = vec![];

    // Render past messages
    for msg in &app.messages {
        let time = msg.timestamp.format("%H:%M").to_string();
        let header = match msg.role {
            Role::User => Line::from(vec![
                Span::styled("You", Style::default().bold()),
                Span::styled(format!(" {}", time), Style::default().dim()),
            ]),
            Role::Assistant | Role::System => Line::from(vec![
                Span::styled("Agent", Style::default().cyan().bold()),
                Span::styled(format!(" {}", time), Style::default().dim()),
            ]),
        };
        lines.push(header);

        // Thinking block
        if msg.thinking.is_some() {
            lines.push(Line::from(Span::styled(
                "  [thinking...]",
                Style::default().dim(),
            )));
        }

        // Tool uses
        for tool in &msg.tool_uses {
            let icon = match tool.status {
                ToolStatus::Complete => "✓",
                ToolStatus::Error => "✗",
                ToolStatus::Running => "→",
            };
            lines.push(Line::from(Span::styled(
                format!("  {} {}", icon, tool.name),
                Style::default().dim(),
            )));
        }

        // Content
        for line in msg.content.lines() {
            lines.push(Line::from(format!("  {}", line)));
        }
        lines.push(Line::from(""));
    }

    // Render streaming message
    if let Some(streaming) = &app.streaming {
        let agent_name = app
            .selected_agent
            .as_ref()
            .and_then(|id| app.agents.iter().find(|a| a.id == *id))
            .map(|a| a.name.as_str())
            .unwrap_or("Agent");

        lines.push(Line::from(vec![Span::styled(
            agent_name,
            Style::default().cyan().bold(),
        )]));

        // Thinking
        if streaming.thinking.is_some() {
            lines.push(Line::from(Span::styled(
                "  [thinking...]",
                Style::default().dim(),
            )));
        }

        // Tools
        for tool in &streaming.tool_uses {
            let icon = match tool.status {
                ToolStatus::Running => app.throbber_char().to_string(),
                ToolStatus::Complete => "✓".to_string(),
                ToolStatus::Error => "✗".to_string(),
            };
            lines.push(Line::from(Span::styled(
                format!("  {} {}", icon, tool.name),
                Style::default().dim(),
            )));
        }

        // Content
        for line in streaming.content.lines() {
            lines.push(Line::from(format!("  {}", line)));
        }

        // Streaming cursor
        if app.mode == Mode::Sending {
            lines.push(Line::from(Span::styled(
                format!("  {}", app.throbber_char()),
                Style::default().dim(),
            )));
        }
    }

    // Empty state
    if lines.is_empty() && app.selected_agent.is_some() {
        lines.push(Line::from(Span::styled(
            "Start typing to chat...",
            Style::default().dim(),
        )));
    }

    let para = Paragraph::new(lines).scroll((app.scroll_offset as u16, 0));
    f.render_widget(para, area);
}
```

**Step 3: Create ui/input.rs**

```rust
// ABOUTME: Input area rendering
// ABOUTME: Wraps tui-textarea with border

use crate::app::App;
use crate::types::Mode;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let title = match app.mode {
        Mode::Sending => " Sending... ",
        Mode::Picker => " Select an agent ",
        Mode::Chat => " Message (Enter to send, Shift+Enter for newline) ",
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if app.mode == Mode::Chat {
            Style::default()
        } else {
            Style::default().dim()
        })
        .title(title);

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Render textarea
    let textarea = app.input.widget();
    f.render_widget(textarea, inner);
}
```

**Step 4: Create ui/picker.rs**

```rust
// ABOUTME: Agent picker overlay rendering
// ABOUTME: Centered modal with filterable agent list

use crate::app::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App) {
    // Center overlay: 60% width, 50% height
    let area = centered_rect(60, 50, f.area());

    // Clear background
    f.render_widget(Clear, area);

    // Filter agents
    let filtered = app.filtered_agents();

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(i, agent)| {
            let icon = if agent.connected { "●" } else { "○" };
            let model = agent.model.as_deref().unwrap_or(&agent.backend);
            let text = format!(" {} {} ({})", icon, agent.name, model);

            let style = if i == app.picker_index {
                Style::default().reversed()
            } else if !agent.connected {
                Style::default().dim()
            } else {
                Style::default()
            };

            ListItem::new(text).style(style)
        })
        .collect();

    let title = if app.picker_filter.is_empty() {
        " Select Agent (type to filter) ".to_string()
    } else {
        format!(" Filter: {} ", app.picker_filter)
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().cyan())
            .title(title),
    );

    f.render_widget(list, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}
```

**Step 5: Create ui/status.rs**

```rust
// ABOUTME: Bottom status bar rendering
// ABOUTME: Shows agent, connection, tokens, keybinds

use crate::app::App;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let mut spans: Vec<Span> = vec![];

    // Agent + model
    if let Some(agent_id) = &app.selected_agent {
        if let Some(agent) = app.agents.iter().find(|a| a.id == *agent_id) {
            let model = agent.model.as_deref().unwrap_or(&agent.backend);
            spans.push(Span::styled(
                format!(" {} ({}) ", agent.name, model),
                Style::default().bold(),
            ));
        }
    } else {
        spans.push(Span::styled(" No agent ", Style::default().dim()));
    }

    // Connection status
    let conn = if app.connected { "●" } else { "○" };
    let conn_style = if app.connected {
        Style::default().green()
    } else {
        Style::default().red()
    };
    spans.push(Span::styled(conn, conn_style));
    spans.push(Span::raw(" "));

    // Working directory
    if let Some(dir) = &app.session.working_dir {
        let display = truncate_path(dir, 20);
        spans.push(Span::styled(format!("│ {} ", display), Style::default().dim()));
    }

    // Tokens
    let input_tokens = format_tokens(app.session.total_input_tokens);
    let output_tokens = format_tokens(app.session.total_output_tokens);
    spans.push(Span::styled(
        format!("│ {}↑ {}↓ ", input_tokens, output_tokens),
        Style::default().dim(),
    ));

    // Error or Ctrl+C hint
    if let Some(err) = &app.error {
        spans.push(Span::styled(format!("│ ✗ {} ", err), Style::default().red()));
    } else if app.show_ctrl_c_hint() {
        spans.push(Span::styled(
            "│ Press Ctrl+C again to quit ",
            Style::default().yellow(),
        ));
    }

    // Keybinds (right side - we'll just append for now)
    spans.push(Span::styled(
        "│ Ctrl+Space: agents │ Ctrl+Q: quit ",
        Style::default().dim(),
    ));

    let line = Line::from(spans);
    let para = Paragraph::new(line).style(Style::default().on_dark_gray());
    f.render_widget(para, area);
}

fn format_tokens(n: u32) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        format!("...{}", &path[path.len() - max_len + 3..])
    }
}
```

**Step 6: Verify it compiles**

Run: `cargo build -p coven-tui-v2`
Expected: Successful build

**Step 7: Commit**

```bash
git add crates/coven-tui-v2/src/ui/
git commit -m "feat(tui-v2): add UI rendering modules"
```

---

## Task 6: Implement Main Loop

**Files:**
- Modify: `crates/coven-tui-v2/src/main.rs`

**Step 1: Implement full main.rs**

```rust
// ABOUTME: Entry point for coven-chat TUI
// ABOUTME: Handles CLI args, config loading, and TUI main loop

use anyhow::Result;
use clap::Parser;
use coven_tui_v2::app::{Action, App};
use coven_tui_v2::client::{Client, Response, StateChange};
use coven_tui_v2::ui;
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::DefaultTerminal;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(name = "coven-chat")]
#[command(about = "Terminal chat interface for coven agents")]
struct Args {
    /// Agent to start chatting with (skips picker)
    #[arg(short, long)]
    agent: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Send a message non-interactively
    Send {
        /// The message to send
        message: String,
        /// Print the response to stdout
        #[arg(short, long)]
        print: bool,
    },
    /// First-time setup wizard
    Setup,
}

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .expect("Could not find config directory")
        .join("coven-chat")
}

fn gateway_url() -> String {
    std::env::var("COVEN_GATEWAY_URL").unwrap_or_else(|_| "http://localhost:7777".to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Some(Command::Send { message, print }) => {
            return coven_tui_v2::cli::send::run(&gateway_url(), &message, print).await;
        }
        Some(Command::Setup) => {
            return coven_tui_v2::cli::setup::run().await;
        }
        None => {}
    }

    // Run TUI
    let mut terminal = ratatui::init();
    let result = run_app(&mut terminal, args.agent).await;
    ratatui::restore();

    result
}

async fn run_app(terminal: &mut DefaultTerminal, initial_agent: Option<String>) -> Result<()> {
    // Channels
    let (response_tx, mut response_rx) = mpsc::channel::<Response>(32);
    let (state_tx, mut state_rx) = mpsc::channel::<StateChange>(32);
    let (key_tx, mut key_rx) = mpsc::channel::<event::KeyEvent>(32);

    // Create client
    let ssh_key_path = coven_ssh::default_client_key_path()
        .ok_or_else(|| anyhow::anyhow!("Could not find SSH key path"))?;
    let client = Client::new(&gateway_url(), &ssh_key_path)?;
    client.setup_callbacks(response_tx.clone(), state_tx);

    // Load app state
    let config_dir = config_dir();
    let mut app = App::load(&config_dir, initial_agent);

    // Fetch agents
    match client.list_agents().await {
        Ok(agents) => {
            app.agents = agents;
            app.connected = true;
        }
        Err(e) => {
            app.error = Some(format!("Failed to connect: {}", e));
        }
    }

    // Spawn input task
    let key_tx_clone = key_tx.clone();
    std::thread::spawn(move || {
        loop {
            if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if key.kind == KeyEventKind::Press {
                        if key_tx_clone.blocking_send(key).is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    // Tick interval for animations
    let mut tick = tokio::time::interval(Duration::from_millis(100));

    // Main loop
    loop {
        terminal.draw(|f| ui::render(f, &app))?;

        tokio::select! {
            Some(key) = key_rx.recv() => {
                if let Some(action) = app.handle_key(key) {
                    match action {
                        Action::Quit => break,
                        Action::SendMessage(msg) => {
                            if let Some(agent_id) = &app.selected_agent {
                                if let Err(e) = client.send_message(agent_id, &msg) {
                                    app.error = Some(e.to_string());
                                    app.mode = coven_tui_v2::types::Mode::Chat;
                                    app.streaming = None;
                                }
                            }
                        }
                        Action::RefreshAgents => {
                            if let Ok(agents) = client.list_agents().await {
                                app.agents = agents;
                            }
                        }
                    }
                }
            }
            Some(response) = response_rx.recv() => {
                app.handle_response(response);
            }
            Some(state_change) = state_rx.recv() => {
                match state_change {
                    StateChange::ConnectionStatus(connected) => {
                        app.connected = connected;
                    }
                    StateChange::StreamingChanged(_, _) => {}
                    StateChange::MessagesChanged(_) => {}
                }
            }
            _ = tick.tick() => {
                app.tick();
            }
        }
    }

    // Save state
    let _ = app.save(&config_dir);

    Ok(())
}
```

**Step 2: Verify it compiles**

Run: `cargo build -p coven-tui-v2`
Expected: Successful build (CLI modules not yet created)

**Step 3: Commit**

```bash
git add crates/coven-tui-v2/src/main.rs
git commit -m "feat(tui-v2): implement main loop with tokio::select!"
```

---

## Task 7: Implement CLI Commands

**Files:**
- Create: `crates/coven-tui-v2/src/cli/mod.rs`
- Create: `crates/coven-tui-v2/src/cli/send.rs`
- Create: `crates/coven-tui-v2/src/cli/setup.rs`

**Step 1: Create cli/mod.rs**

```rust
// ABOUTME: CLI subcommands for coven-chat
// ABOUTME: Non-interactive operations (send, setup)

pub mod send;
pub mod setup;
```

**Step 2: Create cli/send.rs**

```rust
// ABOUTME: Send command implementation
// ABOUTME: Non-interactive message sending with streaming output

use crate::client::{Client, Response};
use anyhow::Result;
use std::io::{self, Write};
use tokio::sync::mpsc;

pub async fn run(gateway_url: &str, message: &str, print: bool) -> Result<()> {
    let ssh_key_path = coven_ssh::default_client_key_path()
        .ok_or_else(|| anyhow::anyhow!("Could not find SSH key path"))?;

    let (response_tx, mut response_rx) = mpsc::channel::<Response>(32);
    let (state_tx, _state_rx) = mpsc::channel(32);

    let client = Client::new(gateway_url, &ssh_key_path)?;
    client.setup_callbacks(response_tx, state_tx);

    // Get first connected agent
    let agents = client.list_agents().await?;
    let agent = agents
        .iter()
        .find(|a| a.connected)
        .ok_or_else(|| anyhow::anyhow!("No connected agents"))?;

    // Send message
    client.send_message(&agent.id, message)?;

    // Stream response
    let mut stdout = io::stdout();
    while let Some(response) = response_rx.recv().await {
        match response {
            Response::Text(text) => {
                if print {
                    print!("{}", text);
                    stdout.flush()?;
                }
            }
            Response::Done => break,
            Response::Error(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            _ => {} // Ignore thinking/tools in CLI mode
        }
    }

    if print {
        println!();
    }

    Ok(())
}
```

**Step 3: Create cli/setup.rs**

```rust
// ABOUTME: Setup wizard implementation
// ABOUTME: Interactive first-time configuration

use anyhow::Result;
use std::io::{self, Write};

pub async fn run() -> Result<()> {
    println!("Coven Chat Setup");
    println!("================\n");

    // Gateway URL
    print!("Gateway URL [localhost:7777]: ");
    io::stdout().flush()?;
    let mut gateway = String::new();
    io::stdin().read_line(&mut gateway)?;
    let gateway = gateway.trim();
    let gateway = if gateway.is_empty() {
        "http://localhost:7777"
    } else if !gateway.starts_with("http") {
        &format!("http://{}", gateway)
    } else {
        gateway
    };

    // Test connection
    print!("Testing connection... ");
    io::stdout().flush()?;

    let ssh_key_path = coven_ssh::default_client_key_path()
        .ok_or_else(|| anyhow::anyhow!("Could not find SSH key path"))?;

    match crate::client::Client::new(gateway, &ssh_key_path) {
        Ok(client) => match client.list_agents().await {
            Ok(agents) => {
                println!("✓ Connected ({} agents)", agents.len());
            }
            Err(e) => {
                println!("✗ Failed");
                eprintln!("\nCould not connect to gateway: {}", e);
                std::process::exit(1);
            }
        },
        Err(e) => {
            println!("✗ Failed");
            eprintln!("\nCould not create client: {}", e);
            std::process::exit(1);
        }
    }

    // Save config
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?
        .join("coven-chat");

    std::fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join("config.toml");

    std::fs::write(
        &config_path,
        format!(
            "# Coven Chat Configuration\n\n[gateway]\nurl = \"{}\"\n",
            gateway
        ),
    )?;

    println!("\n✓ Configuration saved to {}", config_path.display());
    println!("\nRun 'coven-chat' to start chatting!");

    Ok(())
}
```

**Step 4: Update lib.rs**

```rust
// ABOUTME: Coven TUI v2 - simplified terminal interface for coven agents
// ABOUTME: Channel-based async architecture with Ratatui

pub mod app;
pub mod client;
pub mod types;
pub mod ui;

pub mod cli;
```

**Step 5: Verify it compiles**

Run: `cargo build -p coven-tui-v2`
Expected: Successful build

**Step 6: Commit**

```bash
git add crates/coven-tui-v2/src/cli/
git add crates/coven-tui-v2/src/lib.rs
git commit -m "feat(tui-v2): add send and setup CLI commands"
```

---

## Task 8: Integration Test

**Files:**
- Modify: `crates/coven-tui-v2/Cargo.toml` (add dev-dependencies if needed)

**Step 1: Run full build**

Run: `cargo build -p coven-tui-v2`
Expected: Successful build with no warnings

**Step 2: Run tests**

Run: `cargo test -p coven-tui-v2`
Expected: All tests pass

**Step 3: Check clippy**

Run: `cargo clippy -p coven-tui-v2 -- -D warnings`
Expected: No warnings

**Step 4: Manual smoke test (if gateway available)**

Run: `cargo run -p coven-tui-v2 -- --help`
Expected: Shows help message

**Step 5: Commit any fixes**

```bash
git add -A
git commit -m "fix(tui-v2): address clippy warnings and test failures"
```

---

## Task 9: Swap Binary Names

**Files:**
- Modify: `crates/coven-tui/Cargo.toml` - rename binary to `coven-chat-old`
- Modify: `crates/coven-tui-v2/Cargo.toml` - ensure binary is `coven-chat`

**Step 1: Update old TUI Cargo.toml**

Change the `[[bin]]` section in `crates/coven-tui/Cargo.toml`:

```toml
[[bin]]
name = "coven-chat-old"
path = "src/main.rs"
```

**Step 2: Verify both build**

Run: `cargo build --workspace`
Expected: Both `coven-chat` and `coven-chat-old` binaries exist

**Step 3: Commit**

```bash
git add crates/coven-tui/Cargo.toml crates/coven-tui-v2/Cargo.toml
git commit -m "feat(tui): swap v2 to be primary coven-chat binary"
```

---

## Summary

| Task | Description | Est. Lines |
|------|-------------|------------|
| 1 | Create crate skeleton | ~50 |
| 2 | Define core types | ~150 |
| 3 | Implement client wrapper | ~120 |
| 4 | Implement App state | ~300 |
| 5 | Implement UI rendering | ~250 |
| 6 | Implement main loop | ~100 |
| 7 | Implement CLI commands | ~100 |
| 8 | Integration test | ~0 |
| 9 | Swap binary names | ~0 |

**Total: ~1,070 lines** (vs ~2,500 in current implementation)
