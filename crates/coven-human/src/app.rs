// ABOUTME: Main application state and logic for the human agent TUI.
// ABOUTME: Manages the connection to coven gateway and handles user interactions.

use crate::messages::{Message, MessageDirection};
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
use ratatui::style::{Color, Style};
use std::io::{self, Stdout};
use std::time::Duration;
use tokio::sync::mpsc;
use tonic::transport::Channel;
use tui_textarea::TextArea;

/// Actions that need async handling (returned from handle_key)
pub enum Action {
    Quit,
    SendReply,
}

/// Create a TextArea with black background styling (matches coven-tui-v2)
fn styled_textarea() -> TextArea<'static> {
    let mut ta = TextArea::default();
    let bg = Style::default().bg(Color::Rgb(0, 0, 0));
    ta.set_style(bg);
    ta.set_cursor_line_style(bg);
    ta
}

/// Application state for the human agent TUI
pub struct App {
    /// Connection status
    pub connected: bool,
    /// Agent ID (assigned after registration)
    pub agent_id: String,
    /// Server ID from welcome message
    pub server_id: String,
    /// Messages received from gateway
    pub messages: Vec<Message>,
    /// Always-active text input area
    pub input: TextArea<'static>,
    /// Scroll offset for chat viewport (0 = bottom)
    pub scroll_offset: usize,
    /// Status bar message
    pub status: String,
    /// Whether app should quit
    pub should_quit: bool,
    /// Request ID of the active (most recent) incoming message
    pub active_request_id: Option<String>,
    /// Thread ID of the active incoming message
    pub active_thread_id: Option<String>,
}

impl App {
    /// Create a new App with default state
    pub fn new(agent_id: String) -> Self {
        Self {
            connected: false,
            agent_id,
            server_id: String::new(),
            messages: Vec::new(),
            input: styled_textarea(),
            scroll_offset: 0,
            status: "Connecting...".to_string(),
            should_quit: false,
            active_request_id: None,
            active_thread_id: None,
        }
    }

    /// Add a received message and set it as the reply target
    pub fn add_message(&mut self, msg: Message) {
        self.active_request_id = Some(msg.id.clone());
        self.active_thread_id = Some(msg.thread_id.clone());
        self.messages.push(msg);
        self.scroll_offset = 0;
        self.status = "New message received".to_string();
    }

    /// Handle a key event, returning an action if one should be taken
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Ctrl+C always quits
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Some(Action::Quit);
        }

        // 'q' quits only when input is empty
        if key.code == KeyCode::Char('q') && self.input_is_empty() {
            return Some(Action::Quit);
        }

        // Enter (no Shift) sends reply when there's an active request and non-empty input.
        // Without an active request, Enter falls through to textarea as newline.
        if key.code == KeyCode::Enter
            && !key.modifiers.contains(KeyModifiers::SHIFT)
            && self.active_request_id.is_some()
            && !self.input_is_empty()
        {
            return Some(Action::SendReply);
        }

        // PgUp/PgDn for scrolling chat
        match key.code {
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_add(10);
                return None;
            }
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
                return None;
            }
            _ => {}
        }

        // Everything else goes to the textarea
        self.input.input(key);
        None
    }

    /// Take the composed reply text, record outgoing message, and reset input
    pub fn take_reply(&mut self) -> Option<(String, String, String)> {
        let text = self.input.lines().join("\n").trim().to_string();
        if text.is_empty() {
            return None;
        }
        let request_id = self.active_request_id.clone()?;
        let thread_id = self.active_thread_id.clone().unwrap_or_default();

        // Record the outgoing message in the chat history
        self.messages.push(Message::outgoing(text.clone()));

        // Reset input and state
        self.input = styled_textarea();
        self.scroll_offset = 0;
        self.active_request_id = None;
        self.active_thread_id = None;
        self.status = "Reply sent".to_string();
        Some((request_id, thread_id, text))
    }

    /// Check if the textarea input is empty
    fn input_is_empty(&self) -> bool {
        self.input.lines().join("").trim().is_empty()
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
                if let Some(action) = app.handle_key(key) {
                    match action {
                        Action::Quit => {
                            app.should_quit = true;
                        }
                        Action::SendReply => {
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
                                        MessageDirection::Incoming,
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
        assert!(!app.connected);
        assert_eq!(app.agent_id, "test-agent");
        assert!(app.messages.is_empty());
        assert!(app.input.is_empty());
        assert!(!app.should_quit);
        assert!(app.active_request_id.is_none());
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
            MessageDirection::Incoming,
        );
        app.add_message(msg);
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.active_request_id.as_deref(), Some("req-1"));
        assert_eq!(app.active_thread_id.as_deref(), Some("thread-1"));
        assert_eq!(app.scroll_offset, 0);
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
            MessageDirection::Incoming,
        );
        app.add_message(msg);
        app.input.insert_str("My reply");

        let reply = app.take_reply();
        assert!(reply.is_some());
        let (request_id, thread_id, text) = reply.unwrap();
        assert_eq!(request_id, "req-1");
        assert_eq!(thread_id, "thread-1");
        assert_eq!(text, "My reply");
        assert!(app.input.is_empty());
        assert!(app.active_request_id.is_none());
    }

    #[test]
    fn test_handle_quit_key() {
        let mut app = App::new("test".to_string());
        // 'q' with empty input should quit
        let action = app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(matches!(action, Some(Action::Quit)));
    }

    #[test]
    fn test_q_doesnt_quit_with_input() {
        let mut app = App::new("test".to_string());
        // Type something first
        app.input.insert_str("hello");
        // Now 'q' should type 'q', not quit
        let action = app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(action.is_none());
        // Verify 'q' was typed into textarea
        let content = app.input.lines().join("");
        assert!(content.contains('q'));
    }

    #[test]
    fn test_handle_ctrl_c() {
        let mut app = App::new("test".to_string());
        let action = app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(matches!(action, Some(Action::Quit)));
    }

    #[test]
    fn test_handle_scroll() {
        let mut app = App::new("test".to_string());
        assert_eq!(app.scroll_offset, 0);
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(app.scroll_offset, 10);
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.scroll_offset, 0);
        // Should not underflow
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn test_typing() {
        let mut app = App::new("test".to_string());
        app.handle_key(KeyEvent::new(KeyCode::Char('H'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        let content = app.input.lines().join("");
        assert_eq!(content, "Hi");
    }

    #[test]
    fn test_enter_sends_with_active_request() {
        let mut app = App::new("test".to_string());
        let msg = Message::new(
            "req-1".to_string(),
            "thread-1".to_string(),
            "sender".to_string(),
            "Hello".to_string(),
            Utc::now(),
            MessageDirection::Incoming,
        );
        app.add_message(msg);
        app.input.insert_str("My reply");

        let action = app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(action, Some(Action::SendReply)));
    }

    #[test]
    fn test_enter_is_newline_without_request() {
        let mut app = App::new("test".to_string());
        app.input.insert_str("some text");

        // Enter without an active request should pass through to textarea (newline)
        let action = app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(action.is_none());
    }

    #[test]
    fn test_outgoing_message_recorded() {
        let mut app = App::new("test".to_string());
        let msg = Message::new(
            "req-1".to_string(),
            "thread-1".to_string(),
            "sender".to_string(),
            "Hello".to_string(),
            Utc::now(),
            MessageDirection::Incoming,
        );
        app.add_message(msg);
        app.input.insert_str("My reply");

        let reply = app.take_reply();
        assert!(reply.is_some());

        // Should now have 2 messages: incoming + outgoing
        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.messages[1].direction, MessageDirection::Outgoing);
        assert_eq!(app.messages[1].content, "My reply");
        assert_eq!(app.messages[1].sender, "you");
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
