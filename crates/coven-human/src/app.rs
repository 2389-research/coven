// ABOUTME: Main application state and logic for the human agent TUI.
// ABOUTME: Manages the connection to coven gateway and handles user interactions.

use crate::messages::{InputMode, Message};
use crate::ui;
use crate::HumanConfig;
use anyhow::{Context, Result};
use chrono::Utc;
use coven_link::config::CovenConfig;
use coven_proto::client::CovenControlClient;
use coven_proto::{agent_message, message_response, server_message, AgentMessage, RegisterAgent};
use coven_ssh::{load_or_generate_key, SshAuthCredentials};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use ratatui::prelude::*;
use std::io::{self, Stdout};
use std::time::Duration;
use tokio::sync::mpsc;
use tonic::transport::Channel;

/// Application state for the human agent TUI
pub struct App {
    /// Current input mode
    pub mode: InputMode,
    /// Connection status
    pub connected: bool,
    /// Agent ID (assigned after registration)
    pub agent_id: String,
    /// Server ID from welcome message
    pub server_id: String,
    /// Messages received from gateway
    pub messages: Vec<Message>,
    /// Current compose buffer
    pub input: String,
    /// Scroll offset for message viewport
    pub scroll: u16,
    /// Status bar message
    pub status: String,
    /// Whether app should quit
    pub should_quit: bool,
    /// Request ID of the message being replied to
    pub reply_to_request_id: Option<String>,
    /// Thread ID of the message being replied to
    pub reply_to_thread_id: Option<String>,
}

impl App {
    /// Create a new App with default state
    pub fn new(agent_id: String) -> Self {
        Self {
            mode: InputMode::Viewing,
            connected: false,
            agent_id,
            server_id: String::new(),
            messages: Vec::new(),
            input: String::new(),
            scroll: 0,
            status: "Connecting...".to_string(),
            should_quit: false,
            reply_to_request_id: None,
            reply_to_thread_id: None,
        }
    }

    /// Enter composing mode (if there's a message to reply to)
    pub fn enter_compose(&mut self) {
        if self.reply_to_request_id.is_some() {
            self.mode = InputMode::Composing;
            self.status = "Composing reply... (Enter to send, Esc to cancel)".to_string();
        } else {
            self.status = "No message to reply to".to_string();
        }
    }

    /// Cancel composing and return to viewing mode
    pub fn cancel_compose(&mut self) {
        self.mode = InputMode::Viewing;
        self.input.clear();
        self.status = "Cancelled".to_string();
    }

    /// Add a received message and set it as the reply target
    pub fn add_message(&mut self, msg: Message) {
        self.reply_to_request_id = Some(msg.id.clone());
        self.reply_to_thread_id = Some(msg.thread_id.clone());
        self.messages.push(msg);
        self.status = "New message received — press 'r' to reply".to_string();
    }

    /// Take the composed reply text and clear the buffer
    pub fn take_reply(&mut self) -> Option<(String, String, String)> {
        if self.input.is_empty() {
            return None;
        }
        let request_id = self.reply_to_request_id.clone()?;
        let thread_id = self.reply_to_thread_id.clone().unwrap_or_default();
        let text = std::mem::take(&mut self.input);
        self.mode = InputMode::Viewing;
        self.reply_to_request_id = None;
        self.reply_to_thread_id = None;
        self.status = "Reply sent".to_string();
        Some((request_id, thread_id, text))
    }

    /// Handle a key event, returning true if the app should continue
    pub fn handle_key(&mut self, key: KeyEvent) {
        match self.mode {
            InputMode::Viewing => self.handle_viewing_key(key),
            InputMode::Composing => self.handle_composing_key(key),
        }
    }

    fn handle_viewing_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('r') => self.enter_compose(),
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll = self.scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll = self.scroll.saturating_add(1);
            }
            _ => {}
        }
    }

    fn handle_composing_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.cancel_compose(),
            KeyCode::Enter => {
                // Reply will be taken by the event loop
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => {
                self.input.push(c);
            }
            _ => {}
        }
    }
}

/// Resolve gateway URL from config or CLI arg
fn resolve_gateway(gateway_arg: Option<&str>) -> Result<String> {
    if let Some(gw) = gateway_arg {
        let url = if gw.starts_with("http://") || gw.starts_with("https://") {
            gw.to_string()
        } else {
            format!("http://{}", gw)
        };
        return Ok(url);
    }

    let config = CovenConfig::load()
        .context("No gateway specified and no coven config found. Run 'coven link' first.")?;
    let gateway = &config.gateway;
    if gateway.starts_with("http://") || gateway.starts_with("https://") {
        Ok(gateway.clone())
    } else {
        Ok(format!("http://{}", gateway))
    }
}

/// Resolve agent name from CLI arg or hostname
fn resolve_name(name_arg: Option<&str>) -> String {
    if let Some(name) = name_arg {
        return name.to_string();
    }
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .map(|h| format!("human-{}", h))
        .unwrap_or_else(|| "human-agent".to_string())
}

/// Resolve agent ID from CLI arg or generate a UUID
fn resolve_id(id_arg: Option<&str>) -> String {
    id_arg
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("human-{}", uuid::Uuid::new_v4()))
}

/// Main entry point: connect to gateway and run the TUI
pub async fn run(config: HumanConfig) -> Result<()> {
    let gateway_url = resolve_gateway(config.gateway.as_deref())?;
    let agent_name = resolve_name(config.name.as_deref());
    let agent_id = resolve_id(config.id.as_deref());

    // Load SSH key (same path as coven-tui-v2)
    let key_path = CovenConfig::key_path()
        .context("Could not determine SSH key path. Run 'coven link' first.")?;
    let private_key = load_or_generate_key(&key_path)
        .with_context(|| format!("Failed to load SSH key from {}", key_path.display()))?;

    eprintln!("Connecting to gateway at {}...", gateway_url);
    eprintln!("Agent: {} ({})", agent_name, agent_id);

    // Connect to gateway
    let channel = Channel::from_shared(gateway_url.clone())?
        .connect()
        .await
        .with_context(|| format!("Failed to connect to gateway at {}", gateway_url))?;

    // Create SSH auth interceptor (same pattern as coven-agent)
    let private_key_clone = private_key.clone();
    let ssh_auth_interceptor =
        move |mut req: tonic::Request<()>| -> std::result::Result<tonic::Request<()>, tonic::Status> {
            match SshAuthCredentials::new(&private_key_clone) {
                Ok(creds) => {
                    if let Err(e) = creds.apply_to_request(&mut req) {
                        return Err(tonic::Status::internal(format!(
                            "failed to apply SSH auth: {}",
                            e
                        )));
                    }
                }
                Err(e) => {
                    return Err(tonic::Status::internal(format!(
                        "failed to create SSH auth credentials: {}",
                        e
                    )));
                }
            }
            Ok(req)
        };

    let mut client = CovenControlClient::with_interceptor(channel, ssh_auth_interceptor);

    // Create bidirectional stream
    let (tx, rx) = mpsc::channel::<AgentMessage>(100);
    let outbound = tokio_stream::wrappers::ReceiverStream::new(rx);

    eprintln!("Opening bidirectional stream...");
    let response = client
        .agent_stream(outbound)
        .await
        .context("Failed to open agent stream")?;
    let mut inbound = response.into_inner();

    // Send registration
    tx.send(AgentMessage {
        payload: Some(agent_message::Payload::Register(RegisterAgent {
            agent_id: agent_id.clone(),
            name: agent_name.clone(),
            capabilities: vec!["human".to_string()],
            metadata: None,
            protocol_features: vec![],
        })),
    })
    .await
    .context("Failed to send registration")?;

    eprintln!("Registration sent, waiting for welcome...");

    // Wait for Welcome message
    let mut app = App::new(agent_id);
    loop {
        match inbound.next().await {
            Some(Ok(server_msg)) => {
                if let Some(payload) = server_msg.payload {
                    match payload {
                        server_message::Payload::Welcome(welcome) => {
                            app.connected = true;
                            app.server_id = welcome.server_id.clone();
                            app.agent_id = welcome.agent_id.clone();
                            app.status = format!(
                                "Connected to {} as {}",
                                welcome.server_id, welcome.agent_id
                            );
                            eprintln!(
                                "Welcome! server={}, agent={}, instance={}",
                                welcome.server_id, welcome.agent_id, welcome.instance_id
                            );
                            break;
                        }
                        server_message::Payload::RegistrationError(err) => {
                            anyhow::bail!("Registration failed: {}", err.reason);
                        }
                        _ => {
                            tracing::warn!("Unexpected message before welcome");
                        }
                    }
                }
            }
            Some(Err(e)) => {
                anyhow::bail!("Stream error waiting for welcome: {}", e);
            }
            None => {
                anyhow::bail!("Stream closed before receiving welcome");
            }
        }
    }

    // Set up terminal
    let mut terminal = setup_terminal()?;

    // Install panic hook to restore terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal_basic();
        original_hook(panic_info);
    }));

    // Run the main TUI loop
    let result = run_main_loop(&mut terminal, &mut app, &tx, &mut inbound).await;

    // Restore terminal
    restore_terminal(&mut terminal)?;

    result
}

/// The main TUI event loop
async fn run_main_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    tx: &mpsc::Sender<AgentMessage>,
    inbound: &mut tonic::Streaming<coven_proto::ServerMessage>,
) -> Result<()> {
    // Spawn keyboard input reader
    let (key_tx, mut key_rx) = mpsc::channel::<KeyEvent>(32);
    let _input_handle = spawn_input_task(key_tx);

    // Tick interval for UI refresh
    let mut tick_interval = tokio::time::interval(Duration::from_millis(100));

    loop {
        // Render UI
        terminal.draw(|f| ui::render(f, app))?;

        if app.should_quit {
            break;
        }

        tokio::select! {
            // Keyboard input
            Some(key) = key_rx.recv() => {
                // Check if Enter was pressed in compose mode (send reply)
                let was_composing = app.mode == InputMode::Composing;
                let was_enter = key.code == KeyCode::Enter;

                app.handle_key(key);

                if was_composing && was_enter {
                    if let Some((request_id, _thread_id, text)) = app.take_reply() {
                        // Send Text event
                        let text_clone = text.clone();
                        tx.send(AgentMessage {
                            payload: Some(agent_message::Payload::Response(
                                coven_proto::MessageResponse {
                                    request_id: request_id.clone(),
                                    event: Some(message_response::Event::Text(text_clone)),
                                },
                            )),
                        })
                        .await?;

                        // Send Done event
                        tx.send(AgentMessage {
                            payload: Some(agent_message::Payload::Response(
                                coven_proto::MessageResponse {
                                    request_id,
                                    event: Some(message_response::Event::Done(
                                        coven_proto::Done {
                                            full_response: text,
                                        },
                                    )),
                                },
                            )),
                        })
                        .await?;
                    }
                }
            }

            // Incoming messages from gateway
            msg = inbound.next() => {
                match msg {
                    Some(Ok(server_msg)) => {
                        if let Some(payload) = server_msg.payload {
                            match payload {
                                server_message::Payload::SendMessage(send_msg) => {
                                    let message = Message::new(
                                        send_msg.request_id,
                                        send_msg.thread_id,
                                        send_msg.sender,
                                        send_msg.content,
                                        Utc::now(),
                                    );
                                    app.add_message(message);
                                }
                                server_message::Payload::Shutdown(_) => {
                                    app.status = "Gateway shutting down".to_string();
                                    app.connected = false;
                                    app.should_quit = true;
                                }
                                _ => {
                                    tracing::debug!("Unhandled server message");
                                }
                            }
                        }
                    }
                    Some(Err(e)) => {
                        app.connected = false;
                        app.status = format!("Stream error: {}", e);
                        tracing::error!("gRPC stream error: {}", e);
                    }
                    None => {
                        app.connected = false;
                        app.status = "Disconnected from gateway".to_string();
                        app.should_quit = true;
                    }
                }
            }

            // Tick for UI refresh
            _ = tick_interval.tick() => {}
        }
    }

    Ok(())
}

/// Spawn a blocking task to read crossterm key events
fn spawn_input_task(key_tx: mpsc::Sender<KeyEvent>) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || loop {
        if event::poll(Duration::from_millis(50)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                if key_tx.blocking_send(key).is_err() {
                    break;
                }
            }
        }
        if key_tx.is_closed() {
            break;
        }
    })
}

/// Set up the terminal for TUI rendering
fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("Failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend).context("Failed to create terminal")?;
    Ok(terminal)
}

/// Restore terminal to normal state
fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode().context("Failed to disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("Failed to leave alternate screen")?;
    terminal.show_cursor().context("Failed to show cursor")?;
    Ok(())
}

/// Basic terminal restoration for panic handler
fn restore_terminal_basic() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_initial_state() {
        let app = App::new("test-agent".to_string());
        assert_eq!(app.mode, InputMode::Viewing);
        assert!(!app.connected);
        assert_eq!(app.agent_id, "test-agent");
        assert!(app.messages.is_empty());
        assert!(app.input.is_empty());
        assert!(!app.should_quit);
        assert!(app.reply_to_request_id.is_none());
    }

    #[test]
    fn test_enter_compose_without_message() {
        let mut app = App::new("test".to_string());
        app.enter_compose();
        // Should stay in viewing mode since no message to reply to
        assert_eq!(app.mode, InputMode::Viewing);
    }

    #[test]
    fn test_enter_compose_with_message() {
        let mut app = App::new("test".to_string());
        let msg = Message::new(
            "req-1".to_string(),
            "thread-1".to_string(),
            "sender".to_string(),
            "Hello".to_string(),
            Utc::now(),
        );
        app.add_message(msg);
        app.enter_compose();
        assert_eq!(app.mode, InputMode::Composing);
    }

    #[test]
    fn test_cancel_compose() {
        let mut app = App::new("test".to_string());
        let msg = Message::new(
            "req-1".to_string(),
            "thread-1".to_string(),
            "sender".to_string(),
            "Hello".to_string(),
            Utc::now(),
        );
        app.add_message(msg);
        app.enter_compose();
        app.input = "draft reply".to_string();
        app.cancel_compose();
        assert_eq!(app.mode, InputMode::Viewing);
        assert!(app.input.is_empty());
    }

    #[test]
    fn test_add_message() {
        let mut app = App::new("test".to_string());
        assert!(app.messages.is_empty());

        let msg = Message::new(
            "req-1".to_string(),
            "thread-1".to_string(),
            "sender".to_string(),
            "Hello".to_string(),
            Utc::now(),
        );
        app.add_message(msg);
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.reply_to_request_id.as_deref(), Some("req-1"));
        assert_eq!(app.reply_to_thread_id.as_deref(), Some("thread-1"));
    }

    #[test]
    fn test_take_reply_empty() {
        let mut app = App::new("test".to_string());
        assert!(app.take_reply().is_none());
    }

    #[test]
    fn test_take_reply_with_content() {
        let mut app = App::new("test".to_string());
        let msg = Message::new(
            "req-1".to_string(),
            "thread-1".to_string(),
            "sender".to_string(),
            "Hello".to_string(),
            Utc::now(),
        );
        app.add_message(msg);
        app.enter_compose();
        app.input = "My reply".to_string();

        let reply = app.take_reply();
        assert!(reply.is_some());
        let (request_id, thread_id, text) = reply.unwrap();
        assert_eq!(request_id, "req-1");
        assert_eq!(thread_id, "thread-1");
        assert_eq!(text, "My reply");
        assert_eq!(app.mode, InputMode::Viewing);
        assert!(app.input.is_empty());
        assert!(app.reply_to_request_id.is_none());
    }

    #[test]
    fn test_handle_quit_key() {
        let mut app = App::new("test".to_string());
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit);
    }

    #[test]
    fn test_handle_ctrl_c() {
        let mut app = App::new("test".to_string());
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
    }

    #[test]
    fn test_handle_scroll() {
        let mut app = App::new("test".to_string());
        assert_eq!(app.scroll, 0);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.scroll, 1);
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.scroll, 0);
        // Should not underflow
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn test_compose_typing() {
        let mut app = App::new("test".to_string());
        let msg = Message::new(
            "req-1".to_string(),
            "thread-1".to_string(),
            "sender".to_string(),
            "Hello".to_string(),
            Utc::now(),
        );
        app.add_message(msg);
        app.enter_compose();

        app.handle_key(KeyEvent::new(KeyCode::Char('H'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(app.input, "Hi");

        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.input, "H");
    }

    #[test]
    fn test_resolve_name_default() {
        let name = resolve_name(None);
        assert!(name.starts_with("human-") || name == "human-agent");
    }

    #[test]
    fn test_resolve_name_explicit() {
        let name = resolve_name(Some("my-human"));
        assert_eq!(name, "my-human");
    }

    #[test]
    fn test_resolve_id_explicit() {
        let id = resolve_id(Some("agent-xyz"));
        assert_eq!(id, "agent-xyz");
    }

    #[test]
    fn test_resolve_id_generated() {
        let id = resolve_id(None);
        assert!(id.starts_with("human-"));
    }

    #[test]
    fn test_resolve_gateway_explicit() {
        let gw = resolve_gateway(Some("http://localhost:9999")).unwrap();
        assert_eq!(gw, "http://localhost:9999");
    }

    #[test]
    fn test_resolve_gateway_adds_scheme() {
        let gw = resolve_gateway(Some("localhost:9999")).unwrap();
        assert_eq!(gw, "http://localhost:9999");
    }
}
