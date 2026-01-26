// ABOUTME: Central application state and event loop.
// ABOUTME: Holds all UI state and coordinates between components.

use crossterm::event::{KeyCode, KeyModifiers};
use fold_client::{Agent, ConnectionStatus, FoldClient, UsageInfo};
use fold_ssh::default_client_key_path;
use ratatui::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio_stream::StreamExt;

use crate::app_event::AppEvent;
use crate::client_bridge::ClientBridge;
use crate::error::{AppError, Result};
use crate::state::config::Config;
use crate::state::{AppState, PersistedMessage};
use crate::theme::{get_theme, Theme};
use crate::tui::event::TuiEvent;
use crate::tui::Tui;
use crate::widgets::chat::{ActiveStream, ChatWidget};
use crate::widgets::input::InputWidget;
use crate::widgets::picker::PickerWidget;
use crate::widgets::status_bar::{shorten_path, BottomBarInfo, StatusBar, TopBarInfo};

/// Safely truncate a string to at most `max_chars` characters, respecting UTF-8 boundaries.
fn truncate_str(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Input,
    Picker,
}

pub struct App {
    client: Arc<FoldClient>,
    agents: Vec<Agent>,
    current_agent_id: Option<String>,
    connection_status: ConnectionStatus,
    is_streaming: bool,
    focus: Focus,
    chat: ChatWidget,
    input: InputWidget<'static>,
    picker: PickerWidget,
    theme: &'static Theme,
    event_tx: UnboundedSender<AppEvent>,
    event_rx: UnboundedReceiver<AppEvent>,
    should_quit: bool,
    /// Persisted application state (conversations, last agent, etc.)
    app_state: AppState,
    /// Accumulated token usage for the current session
    session_usage: UsageInfo,
    /// Gateway URL for display in status bar
    gateway_url: String,
    /// Messages queued while agent is streaming (agent_id -> queued messages)
    agent_queues: HashMap<String, Vec<String>>,
    /// Unread message counts per agent (agent_id -> unread count)
    unread_counts: HashMap<String, u32>,
    /// Timestamp of last Ctrl+C press for double-tap quit
    last_ctrl_c: Option<Instant>,
}

impl App {
    pub fn new(config: &Config) -> Result<Self> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let gateway_url = config.gateway.url();

        let key_path = default_client_key_path().ok_or_else(|| {
            AppError::Config("Could not determine SSH key path (HOME not set?)".into())
        })?;

        let client = Arc::new(
            FoldClient::new_with_auth(gateway_url.clone(), &key_path)
                .map_err(|e| AppError::Config(format!("Failed to initialize SSH auth: {}", e)))?,
        );

        // Set up client callbacks
        let bridge = ClientBridge::new(event_tx.clone());
        let (stream_cb, state_cb) = bridge.into_callbacks();
        client.set_stream_callback(Box::new(ArcStreamCallback(stream_cb)));
        client.set_state_callback(Box::new(ArcStateCallback(state_cb)));

        let theme = get_theme(&config.appearance.theme);

        // Load persisted state (defaults to empty if file doesn't exist)
        let app_state = AppState::load().unwrap_or_else(|e| {
            tracing::warn!("Failed to load app state: {}, using defaults", e);
            AppState::new()
        });

        // Restore last agent ID from persisted state
        let current_agent_id = app_state.last_agent_id.clone();

        Ok(Self {
            client,
            agents: Vec::new(),
            current_agent_id,
            connection_status: ConnectionStatus::Disconnected,
            is_streaming: false,
            focus: Focus::Input,
            chat: ChatWidget::new(theme),
            input: InputWidget::new(),
            picker: PickerWidget::new(),
            theme,
            event_tx,
            event_rx,
            should_quit: false,
            app_state,
            session_usage: UsageInfo::default(),
            gateway_url,
            agent_queues: HashMap::new(),
            unread_counts: HashMap::new(),
            last_ctrl_c: None,
        })
    }

    pub async fn run(&mut self, tui: &mut Tui) -> Result<()> {
        // Initial fetch of agents
        let client = self.client.clone();
        let tx = self.event_tx.clone();
        tokio::spawn(async move {
            match client.refresh_agents_async().await {
                Ok(agents) => {
                    let _ = tx.send(AppEvent::AgentsLoaded(agents));
                }
                Err(e) => {
                    tracing::error!("Failed to load agents: {}", e);
                }
            }
        });

        let mut tui_events = tui.event_stream();

        loop {
            tui.terminal_mut().draw(|frame| self.render(frame))?;

            tokio::select! {
                Some(event) = self.event_rx.recv() => {
                    self.handle_app_event(event).await;
                }
                Some(event) = tui_events.next() => {
                    self.handle_tui_event(event);
                }
            }

            if self.should_quit {
                break;
            }
        }

        // Save state before exiting
        self.save_state();

        Ok(())
    }

    /// Save the current application state to disk
    fn save_state(&mut self) {
        // Update last agent ID in state
        if let Some(agent_id) = &self.current_agent_id {
            self.app_state.set_last_agent(agent_id);
        }

        // Save to disk
        if let Err(e) = self.app_state.save() {
            tracing::error!("Failed to save app state: {}", e);
        }
    }

    /// Save conversation history for the current agent
    fn save_conversation(&mut self) {
        if let Some(agent_id) = &self.current_agent_id {
            let messages: Vec<PersistedMessage> = self
                .chat
                .messages()
                .iter()
                .map(PersistedMessage::from_client_message)
                .collect();
            self.app_state.update_conversation(agent_id, messages);

            // Persist immediately
            if let Err(e) = self.app_state.save() {
                tracing::error!("Failed to save conversation: {}", e);
            }
        }
    }

    /// Load conversation history for an agent into the chat widget
    fn load_conversation(&mut self, agent_id: &str) {
        let messages = self.app_state.get_messages(agent_id);
        self.chat.set_messages(messages);
    }

    /// Get the number of queued messages for the current agent
    fn current_queue_count(&self) -> usize {
        self.current_agent_id
            .as_ref()
            .and_then(|id| self.agent_queues.get(id))
            .map(|q| q.len())
            .unwrap_or(0)
    }

    /// Get a reference to the currently selected agent, if any.
    fn get_current_agent(&self) -> Option<&Agent> {
        self.current_agent_id
            .as_ref()
            .and_then(|id| self.agents.iter().find(|a| &a.id == id))
    }

    /// Render the working directory line for display.
    /// Returns an empty string if no agent is selected or the working_dir is empty.
    /// Replaces home directory with ~ and truncates long paths.
    fn render_working_dir_line(&self, max_width: usize) -> String {
        let agent = match self.get_current_agent() {
            Some(a) => a,
            None => return String::new(),
        };

        if agent.working_dir.is_empty() {
            return String::new();
        }

        shorten_path(&agent.working_dir, max_width)
    }

    fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Top bar
                Constraint::Min(3),    // Chat
                Constraint::Length(1), // Working dir
                Constraint::Length(3), // Input
                Constraint::Length(1), // Bottom bar
            ])
            .split(area);

        // Top status bar
        let top_info = TopBarInfo {
            agents: &self.agents,
            current_agent_id: self.current_agent_id.as_deref(),
            connection_status: self.connection_status,
            is_streaming: self.is_streaming,
            session_usage: &self.session_usage,
            gateway_url: &self.gateway_url,
        };
        StatusBar::render_top(chunks[0], frame.buffer_mut(), self.theme, &top_info);

        // Chat area
        frame.render_widget(&self.chat, chunks[1]);

        // Working directory line
        self.render_working_dir(chunks[2], frame.buffer_mut());

        // Input area
        frame.render_widget(self.input.textarea(), chunks[3]);

        // Bottom status bar
        let bottom_info = BottomBarInfo {
            connection_status: self.connection_status,
            is_streaming: self.is_streaming,
            focus: self.focus,
            queue_count: self.current_queue_count(),
        };
        StatusBar::render_bottom(chunks[4], frame.buffer_mut(), self.theme, &bottom_info);

        // Picker overlay
        if self.focus == Focus::Picker {
            frame.render_widget(&self.picker, area);
        }
    }

    /// Render the working directory line above the input.
    fn render_working_dir(&self, area: Rect, buf: &mut Buffer) {
        // Background fill
        let style = Style::default().bg(self.theme.background);
        buf.set_style(area, style);

        // Get the working directory text (shortened if needed)
        // Reserve space for icon (folder emoji + space = ~3 chars) and padding
        let max_path_width = area.width.saturating_sub(5) as usize;
        let working_dir = self.render_working_dir_line(max_path_width);

        if working_dir.is_empty() {
            return;
        }

        // Build the line: "  [folder icon] [path]"
        let spans = vec![
            Span::raw("  "),
            Span::styled("\u{1F4C1} ", Style::default().fg(self.theme.text_muted)),
            Span::styled(working_dir, Style::default().fg(self.theme.accent)),
        ];

        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }

    async fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::ConnectionStatus(status) => {
                self.connection_status = status;
            }
            AppEvent::AgentsLoaded(agents) => {
                self.agents = agents;
                self.picker.set_agents(&self.agents);

                // If we have a last_agent_id from state, verify it exists and load conversation
                if let Some(agent_id) = self.current_agent_id.clone() {
                    if self.agents.iter().any(|a| a.id == agent_id) {
                        // Agent exists, load its conversation
                        self.load_conversation(&agent_id);
                    } else {
                        // Agent no longer exists, clear the selection
                        tracing::info!(
                            "Last agent '{}' no longer exists, showing picker",
                            agent_id
                        );
                        self.current_agent_id = None;
                        self.focus = Focus::Picker;
                    }
                } else if !self.agents.is_empty() {
                    // No last agent, show picker
                    self.focus = Focus::Picker;
                }
            }
            AppEvent::SelectAgent { agent_id } => {
                // Save current conversation before switching
                self.save_conversation();

                // Clear unread count for the selected agent
                self.unread_counts.remove(&agent_id);
                self.picker.set_unread_counts(&self.unread_counts);

                // Load conversation history for the selected agent
                self.load_conversation(&agent_id);

                self.current_agent_id = Some(agent_id);
                self.focus = Focus::Input;
            }
            AppEvent::OpenPicker => {
                self.focus = Focus::Picker;
            }
            AppEvent::ClosePicker => {
                self.focus = Focus::Input;
            }
            AppEvent::RequestQuit | AppEvent::ForceQuit => {
                // Save current conversation before quitting
                self.save_conversation();
                self.should_quit = true;
            }
            AppEvent::SendMessage { content } => {
                if let Some(agent_id) = &self.current_agent_id {
                    if self.is_streaming {
                        // Agent is streaming - queue the message for later
                        self.agent_queues
                            .entry(agent_id.clone())
                            .or_default()
                            .push(content);
                        tracing::debug!(
                            "Queued message for agent {}, queue size: {}",
                            agent_id,
                            self.agent_queues
                                .get(agent_id)
                                .map(|q| q.len())
                                .unwrap_or(0)
                        );
                    } else {
                        // Not streaming - send immediately
                        tracing::info!(
                            "Sending message to agent {} ({} bytes)",
                            agent_id,
                            content.len()
                        );
                        tracing::trace!("Message preview: {}", truncate_str(&content, 50));
                        let client = self.client.clone();
                        let agent_id_clone = agent_id.clone();
                        let tx = self.event_tx.clone();

                        // Debug: check if agent exists in client's internal list
                        let client_agents = client.get_agents();
                        tracing::info!(
                            "Client has {} agents, looking for {}",
                            client_agents.len(),
                            agent_id
                        );
                        if let Some(found) = client_agents.iter().find(|a| a.id == *agent_id) {
                            tracing::info!("Found agent: {} ({})", found.name, found.id);
                        } else {
                            tracing::error!("Agent {} NOT found in client's agent list!", agent_id);
                        }

                        tokio::spawn(async move {
                            match client.send_message(agent_id_clone.clone(), content) {
                                Ok(()) => {
                                    tracing::info!(
                                        "Message sent successfully to {}",
                                        agent_id_clone
                                    );
                                }
                                Err(e) => {
                                    tracing::error!("Failed to send message: {}", e);
                                    // Show error in chat
                                    let _ = tx.send(AppEvent::StreamEvent {
                                        agent_id: agent_id_clone,
                                        event: fold_client::StreamEvent::Error {
                                            message: format!("Failed to send: {}", e),
                                        },
                                    });
                                }
                            }
                        });
                    }
                } else {
                    tracing::warn!("Cannot send message: no agent selected");
                }
            }
            AppEvent::StreamEvent { agent_id, event } => {
                let is_current_agent = Some(&agent_id) == self.current_agent_id.as_ref();

                // Handle stream completion for non-current agents (increment unread count)
                if !is_current_agent {
                    if let fold_client::StreamEvent::Done = &event {
                        *self.unread_counts.entry(agent_id.clone()).or_insert(0) += 1;
                        self.picker.set_unread_counts(&self.unread_counts);
                    }
                    return;
                }

                // Handle events for the current agent
                match event {
                    fold_client::StreamEvent::Text { content } => {
                        // Initialize stream if not present
                        if self.chat.active_stream_mut().is_none() {
                            self.chat.set_active_stream(ActiveStream::new());
                        }
                        if let Some(stream) = self.chat.active_stream_mut() {
                            stream.text_buffer.push_str(&content);
                        }
                    }
                    fold_client::StreamEvent::Thinking { content } => {
                        if self.chat.active_stream_mut().is_none() {
                            self.chat.set_active_stream(ActiveStream::new());
                        }
                        if let Some(stream) = self.chat.active_stream_mut() {
                            // Replace thinking buffer (not append) for latest thinking state
                            stream.thinking_buffer = content;
                        }
                    }
                    fold_client::StreamEvent::ToolUse { name, input } => {
                        if self.chat.active_stream_mut().is_none() {
                            self.chat.set_active_stream(ActiveStream::new());
                        }
                        if let Some(stream) = self.chat.active_stream_mut() {
                            // Summarize tool input for display (truncate if too long)
                            let input_summary = if input.len() > 50 {
                                format!("{}...", &input[..50])
                            } else {
                                input
                            };
                            stream
                                .tool_lines
                                .push(format!("{} {}", name, input_summary));
                        }
                    }
                    fold_client::StreamEvent::Done => {
                        self.chat.clear_active_stream();
                    }
                    fold_client::StreamEvent::Error { message } => {
                        tracing::error!("Stream error: {}", message);
                        self.chat.clear_active_stream();
                    }
                    fold_client::StreamEvent::Usage { info } => {
                        self.session_usage.accumulate(&info);
                    }
                    fold_client::StreamEvent::ToolState { state, detail } => {
                        if self.chat.active_stream_mut().is_none() {
                            self.chat.set_active_stream(ActiveStream::new());
                        }
                        if let Some(stream) = self.chat.active_stream_mut() {
                            stream.tool_states.push((state, detail));
                        }
                    }
                    _ => {
                        tracing::debug!("Unhandled stream event: {:?}", event);
                    }
                }
            }
            AppEvent::ScrollUp => {
                self.chat.scroll_up();
            }
            AppEvent::ScrollDown => {
                self.chat.scroll_down();
            }
            AppEvent::PageUp => {
                self.chat.page_up();
            }
            AppEvent::PageDown => {
                self.chat.page_down();
            }
            AppEvent::StreamingChanged {
                agent_id,
                is_streaming,
            } => {
                // Only update streaming status for current agent
                if Some(&agent_id) == self.current_agent_id.as_ref() {
                    self.is_streaming = is_streaming;

                    // When streaming ends, process any queued messages
                    if !is_streaming {
                        if let Some(queued) = self.agent_queues.remove(&agent_id) {
                            if !queued.is_empty() {
                                // Concatenate queued messages with newlines
                                let combined = queued.join("\n");
                                tracing::debug!(
                                    "Processing {} queued message(s) for agent {}",
                                    queued.len(),
                                    agent_id
                                );
                                // Send the combined message
                                let _ = self
                                    .event_tx
                                    .send(AppEvent::SendMessage { content: combined });
                            }
                        }
                    }
                }
            }
            AppEvent::ThrobberTick => {
                self.chat.tick_throbber();
            }
            AppEvent::UnreadChanged { agent_id, count } => {
                // Update unread count from server notification
                if count > 0 {
                    self.unread_counts.insert(agent_id, count);
                } else {
                    self.unread_counts.remove(&agent_id);
                }
                self.picker.set_unread_counts(&self.unread_counts);
            }
            AppEvent::MessagesChanged { agent_id } => {
                // Reload messages from the client for the current agent
                tracing::info!(
                    "MessagesChanged received for agent {}, current_agent_id={:?}",
                    agent_id,
                    self.current_agent_id
                );
                if Some(&agent_id) == self.current_agent_id.as_ref() {
                    // Get live messages from the FoldClient cache (not local persistence)
                    let messages = self.client.get_messages(agent_id.clone());
                    tracing::info!("Loaded {} messages for {}", messages.len(), agent_id);
                    for (i, msg) in messages.iter().enumerate() {
                        tracing::trace!(
                            "  [{}] {}: {}",
                            i,
                            if msg.is_user { "USER" } else { "AGENT" },
                            truncate_str(&msg.content, 50)
                        );
                    }
                    self.chat.set_messages(messages);
                } else {
                    tracing::debug!("MessagesChanged for {} (not current agent)", agent_id);
                }
            }
            _ => {
                tracing::debug!("Unhandled app event: {:?}", event);
            }
        }
    }

    fn handle_tui_event(&mut self, event: TuiEvent) {
        match event {
            TuiEvent::Key(key) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match key.code {
                        KeyCode::Char('q') => {
                            let _ = self.event_tx.send(AppEvent::RequestQuit);
                            return;
                        }
                        KeyCode::Char('c') => {
                            // Double Ctrl+C to quit
                            let now = Instant::now();
                            if let Some(last) = self.last_ctrl_c {
                                if now.duration_since(last).as_millis() < 500 {
                                    let _ = self.event_tx.send(AppEvent::ForceQuit);
                                    return;
                                }
                            }
                            self.last_ctrl_c = Some(now);
                            return;
                        }
                        KeyCode::Char(' ') => {
                            let _ = self.event_tx.send(AppEvent::OpenPicker);
                            return;
                        }
                        _ => {}
                    }
                }
                match self.focus {
                    Focus::Input => self.handle_input_key(key),
                    Focus::Picker => self.handle_picker_key(key),
                }
            }
            TuiEvent::Paste(text) => {
                if self.focus == Focus::Input {
                    self.input.textarea_mut().insert_str(&text);
                }
            }
            TuiEvent::Resize(width, height) => {
                self.picker.resize_starfield(width, height);
            }
            TuiEvent::Tick => {
                self.picker.tick();
                let _ = self.event_tx.send(AppEvent::ThrobberTick);
            }
        }
    }

    fn handle_input_key(&mut self, key: crossterm::event::KeyEvent) {
        // Handle scroll keybindings with modifiers
        let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);

        match key.code {
            KeyCode::Enter => {
                let content = self.input.get_content();
                if !content.trim().is_empty() {
                    self.input.add_to_history(content.clone());
                    let _ = self.event_tx.send(AppEvent::SendMessage { content });
                    self.input.clear();
                }
            }
            KeyCode::Esc => {
                let _ = self.event_tx.send(AppEvent::OpenPicker);
            }
            KeyCode::Up if has_ctrl || has_shift => {
                let _ = self.event_tx.send(AppEvent::ScrollUp);
            }
            KeyCode::Down if has_ctrl || has_shift => {
                let _ = self.event_tx.send(AppEvent::ScrollDown);
            }
            KeyCode::Up => {
                // Plain Up arrow: history navigation
                self.input.history_up();
            }
            KeyCode::Down => {
                // Plain Down arrow: history navigation
                self.input.history_down();
            }
            KeyCode::PageUp => {
                let _ = self.event_tx.send(AppEvent::PageUp);
            }
            KeyCode::PageDown => {
                let _ = self.event_tx.send(AppEvent::PageDown);
            }
            _ => {
                // Exit history mode when typing regular characters
                if self.input.is_in_history_mode() {
                    self.input.exit_history_mode();
                }
                self.input.textarea_mut().input(key);
            }
        }
    }

    fn handle_picker_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                let _ = self.event_tx.send(AppEvent::ClosePicker);
            }
            KeyCode::Enter => {
                if let Some(agent) = self.picker.selected_agent() {
                    let _ = self.event_tx.send(AppEvent::SelectAgent {
                        agent_id: agent.id.clone(),
                    });
                }
            }
            KeyCode::Up => {
                self.picker.select_previous();
            }
            KeyCode::Down => {
                self.picker.select_next();
            }
            KeyCode::Char(c) => {
                self.picker.input_char(c);
            }
            KeyCode::Backspace => {
                self.picker.delete_char();
            }
            _ => {}
        }
    }
}

// Wrapper to adapt Arc<dyn StreamCallback> to Box<dyn StreamCallback>
struct ArcStreamCallback(Arc<dyn fold_client::StreamCallback>);

impl fold_client::StreamCallback for ArcStreamCallback {
    fn on_event(&self, agent_id: String, event: fold_client::StreamEvent) {
        self.0.on_event(agent_id, event);
    }
}

// Wrapper to adapt Arc<dyn StateCallback> to Box<dyn StateCallback>
struct ArcStateCallback(Arc<dyn fold_client::StateCallback>);

impl fold_client::StateCallback for ArcStateCallback {
    fn on_connection_status(&self, status: ConnectionStatus) {
        self.0.on_connection_status(status);
    }

    fn on_messages_changed(&self, agent_id: String) {
        self.0.on_messages_changed(agent_id);
    }

    fn on_queue_changed(&self, agent_id: String, count: u32) {
        self.0.on_queue_changed(agent_id, count);
    }

    fn on_unread_changed(&self, agent_id: String, count: u32) {
        self.0.on_unread_changed(agent_id, count);
    }

    fn on_streaming_changed(&self, agent_id: String, is_streaming: bool) {
        self.0.on_streaming_changed(agent_id, is_streaming);
    }
}
