// ABOUTME: Central application state and event handling
// ABOUTME: Single struct holds all state, mutations happen in handle_* methods

use crate::client::Response;
use crate::types::{
    Agent, Message, Mode, PendingApproval, PersistedState, Role, SessionMetadata, StreamBlock,
    StreamingMessage, ToolStatus, ToolUse,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Style};
use std::collections::VecDeque;
use std::path::Path;
use std::time::{Duration, Instant};
use tui_textarea::TextArea;

const MAX_HISTORY: usize = 100;

/// Create a TextArea with black background styling
fn styled_textarea() -> TextArea<'static> {
    let mut ta = TextArea::default();
    let bg = Style::default().bg(Color::Rgb(0, 0, 0));
    ta.set_style(bg);
    ta.set_cursor_line_style(bg);
    ta
}

/// Actions that need async handling (returned from handle_key)
pub enum Action {
    Quit,
    SendMessage(String),
    RefreshAgents,
    /// Load conversation history for an agent from the gateway
    LoadHistory(String),
    /// Approve the currently selected tool approval request
    ApproveSelected,
    /// Deny the currently selected tool approval request
    DenySelected,
    /// Approve all future uses of this tool from this agent
    ApproveAllSelected,
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

    // Queued messages to send after current response completes
    pub pending_messages: VecDeque<String>,

    // Action to execute after response handling (for queued message drain)
    pub queued_action: Option<Action>,

    // Throbber animation frame
    pub throbber_frame: usize,

    // Tool approval state
    pub pending_approvals: Vec<crate::types::PendingApproval>,
    pub selected_approval: Option<usize>,
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
            input: styled_textarea(),
            input_history: vec![],
            history_index: None,
            picker_filter: String::new(),
            picker_index: 0,
            session: SessionMetadata::default(),
            connected: false,
            error: None,
            last_ctrl_c: None,
            pending_messages: VecDeque::new(),
            queued_action: None,
            throbber_frame: 0,
            pending_approvals: vec![],
            selected_approval: None,
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

        // If there are pending approvals, handle approval keys first
        if !self.pending_approvals.is_empty() {
            if let Some(action) = self.handle_approval_key(key) {
                return Some(action);
            }
        }

        match self.mode {
            Mode::Picker => self.handle_picker_key(key),
            Mode::Chat => self.handle_chat_key(key),
            Mode::Sending => self.handle_sending_key(key),
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
                    let agent_id = agent.id.clone();
                    let agent_model = agent.model.clone().unwrap_or_default();
                    drop(filtered);
                    self.selected_agent = Some(agent_id.clone());
                    self.session.model = agent_model;
                    self.mode = Mode::Chat;
                    self.messages.clear();
                    return Some(Action::LoadHistory(agent_id));
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
                    self.input = styled_textarea();
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

    fn handle_sending_key(&mut self, key: KeyEvent) -> Option<Action> {
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
            // Queue message for sending after current response completes
            KeyCode::Enter if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                let content = self.input.lines().join("\n").trim().to_string();
                if !content.is_empty() {
                    self.input_history.push(content.clone());
                    self.history_index = None;
                    self.pending_messages.push_back(content);
                    self.input = styled_textarea();
                }
            }
            // Pass to textarea for typing
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
        self.input = styled_textarea();
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
                    // Append to last text block, or create a new one
                    if let Some(StreamBlock::Text(ref mut s)) = streaming.blocks.last_mut() {
                        s.push_str(&text);
                    } else {
                        streaming.blocks.push(StreamBlock::Text(text));
                    }
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
            Response::ToolStart { name, input } => {
                if let Some(streaming) = &mut self.streaming {
                    streaming.blocks.push(StreamBlock::Tool(ToolUse {
                        name,
                        input,
                        result: None,
                        status: ToolStatus::Running,
                    }));
                }
            }
            Response::ToolResult(result) => {
                if let Some(streaming) = &mut self.streaming {
                    // Attach result to the most recent running tool
                    for block in streaming.blocks.iter_mut().rev() {
                        if let StreamBlock::Tool(ref mut tool) = block {
                            if tool.status == ToolStatus::Running {
                                tool.result = Some(result);
                                tool.status = ToolStatus::Complete;
                                break;
                            }
                        }
                    }
                }
            }
            Response::ToolComplete(_detail) => {
                if let Some(streaming) = &mut self.streaming {
                    // Mark the most recent running tool as complete
                    for block in streaming.blocks.iter_mut().rev() {
                        if let StreamBlock::Tool(ref mut tool) = block {
                            if tool.status == ToolStatus::Running {
                                tool.status = ToolStatus::Complete;
                                break;
                            }
                        }
                    }
                }
            }
            Response::ToolError(_name, error) => {
                if let Some(streaming) = &mut self.streaming {
                    // Mark the most recent running tool as errored
                    for block in streaming.blocks.iter_mut().rev() {
                        if let StreamBlock::Tool(ref mut tool) = block {
                            if tool.status == ToolStatus::Running {
                                tool.result = Some(error);
                                tool.status = ToolStatus::Error;
                                break;
                            }
                        }
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
                        blocks: streaming.blocks,
                        thinking: streaming.thinking,
                        timestamp: chrono::Utc::now(),
                        tokens: None,
                    });
                }
                // Check for queued messages before returning to Chat mode
                if let Some(queued) = self.pending_messages.pop_front() {
                    self.mode = Mode::Sending;
                    self.streaming = Some(StreamingMessage::default());
                    self.scroll_offset = 0;
                    self.messages.push(Message::user(queued.clone()));
                    self.queued_action = Some(Action::SendMessage(queued));
                } else {
                    self.mode = Mode::Chat;
                }
                self.error = None;
            }
            Response::Error(err) => {
                self.error = Some(err);
                self.streaming = None;
                self.mode = Mode::Chat;
            }
            Response::ToolApprovalRequest {
                agent_id,
                request_id,
                tool_id,
                tool_name,
                input_json,
            } => {
                const MAX_PENDING_APPROVALS: usize = 100;
                if self.pending_approvals.len() >= MAX_PENDING_APPROVALS {
                    self.pending_approvals.remove(0);
                    if let Some(idx) = self.selected_approval {
                        self.selected_approval = Some(idx.saturating_sub(1));
                    }
                }

                let approval = PendingApproval {
                    agent_id,
                    request_id,
                    tool_id,
                    tool_name,
                    input_json,
                    timestamp: chrono::Utc::now(),
                };
                self.pending_approvals.push(approval);
                if self.selected_approval.is_none() {
                    self.selected_approval = Some(0);
                }
            }
        }
    }

    /// Take a queued action (from message queue drain) if one exists
    pub fn take_queued_action(&mut self) -> Option<Action> {
        self.queued_action.take()
    }

    /// Check if Ctrl+C hint should be shown
    pub fn show_ctrl_c_hint(&self) -> bool {
        self.last_ctrl_c
            .map(|t| t.elapsed() < Duration::from_millis(500))
            .unwrap_or(false)
    }

    /// Handle key events when approval dialog is shown
    fn handle_approval_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            // Approve selected
            KeyCode::Char('y') | KeyCode::Enter => {
                return Some(Action::ApproveSelected);
            }
            // Deny selected
            KeyCode::Char('n') | KeyCode::Esc => {
                return Some(Action::DenySelected);
            }
            // Approve all from this agent
            KeyCode::Char('a') => {
                return Some(Action::ApproveAllSelected);
            }
            // Navigate between approvals
            KeyCode::Up => {
                if let Some(idx) = self.selected_approval {
                    self.selected_approval = Some(idx.saturating_sub(1));
                }
            }
            KeyCode::Down => {
                if let Some(idx) = self.selected_approval {
                    let max = self.pending_approvals.len().saturating_sub(1);
                    self.selected_approval = Some((idx + 1).min(max));
                }
            }
            _ => {}
        }
        None
    }

    /// Get the currently selected pending approval
    pub fn get_selected_approval(&self) -> Option<&PendingApproval> {
        self.selected_approval
            .and_then(|idx| self.pending_approvals.get(idx))
    }

    /// Remove an approval from the pending list by tool_id and update selection
    pub fn remove_approval(&mut self, tool_id: &str) {
        if let Some(pos) = self
            .pending_approvals
            .iter()
            .position(|a| a.tool_id == tool_id)
        {
            self.pending_approvals.remove(pos);
            // Update selection
            if self.pending_approvals.is_empty() {
                self.selected_approval = None;
            } else if let Some(idx) = self.selected_approval {
                // Keep within bounds
                self.selected_approval = Some(idx.min(self.pending_approvals.len() - 1));
            }
        }
    }

    /// Check if there are pending approvals
    pub fn has_pending_approvals(&self) -> bool {
        !self.pending_approvals.is_empty()
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
        // Text should be in a single block
        let streaming = app.streaming.as_ref().unwrap();
        assert_eq!(streaming.blocks.len(), 1);
        if let StreamBlock::Text(ref text) = streaming.blocks[0] {
            assert_eq!(text, "hello world");
        } else {
            panic!("Expected Text block");
        }
    }

    #[test]
    fn test_handle_response_done() {
        let mut app = App::new(Some("agent-1".to_string()));
        app.mode = Mode::Sending;
        app.streaming = Some(StreamingMessage {
            blocks: vec![StreamBlock::Text("test response".to_string())],
            thinking: None,
        });
        app.handle_response(Response::Done);
        assert!(app.streaming.is_none());
        assert_eq!(app.mode, Mode::Chat);
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.messages[0].content(), "test response");
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

    #[test]
    fn test_handle_response_tool_approval_request() {
        let mut app = App::new(None);
        assert!(app.pending_approvals.is_empty());
        assert!(app.selected_approval.is_none());

        app.handle_response(Response::ToolApprovalRequest {
            agent_id: "agent-1".to_string(),
            request_id: "req-1".to_string(),
            tool_id: "tool-1".to_string(),
            tool_name: "bash".to_string(),
            input_json: r#"{"command": "ls"}"#.to_string(),
        });

        assert_eq!(app.pending_approvals.len(), 1);
        assert_eq!(app.selected_approval, Some(0));
        assert_eq!(app.pending_approvals[0].tool_name, "bash");
    }

    #[test]
    fn test_get_selected_approval() {
        let mut app = App::new(None);
        assert!(app.get_selected_approval().is_none());

        app.pending_approvals.push(PendingApproval {
            agent_id: "agent-1".to_string(),
            request_id: "req-1".to_string(),
            tool_id: "tool-1".to_string(),
            tool_name: "bash".to_string(),
            input_json: "{}".to_string(),
            timestamp: chrono::Utc::now(),
        });
        app.selected_approval = Some(0);

        let approval = app.get_selected_approval();
        assert!(approval.is_some());
        assert_eq!(approval.unwrap().tool_name, "bash");
    }

    #[test]
    fn test_remove_approval() {
        let mut app = App::new(None);
        app.pending_approvals.push(PendingApproval {
            agent_id: "agent-1".to_string(),
            request_id: "req-1".to_string(),
            tool_id: "tool-1".to_string(),
            tool_name: "bash".to_string(),
            input_json: "{}".to_string(),
            timestamp: chrono::Utc::now(),
        });
        app.pending_approvals.push(PendingApproval {
            agent_id: "agent-1".to_string(),
            request_id: "req-2".to_string(),
            tool_id: "tool-2".to_string(),
            tool_name: "read".to_string(),
            input_json: "{}".to_string(),
            timestamp: chrono::Utc::now(),
        });
        app.selected_approval = Some(1);

        // Remove the second one (selected)
        app.remove_approval("tool-2");
        assert_eq!(app.pending_approvals.len(), 1);
        assert_eq!(app.selected_approval, Some(0));

        // Remove the last one
        app.remove_approval("tool-1");
        assert!(app.pending_approvals.is_empty());
        assert!(app.selected_approval.is_none());
    }

    #[test]
    fn test_has_pending_approvals() {
        let mut app = App::new(None);
        assert!(!app.has_pending_approvals());

        app.pending_approvals.push(PendingApproval {
            agent_id: "agent-1".to_string(),
            request_id: "req-1".to_string(),
            tool_id: "tool-1".to_string(),
            tool_name: "bash".to_string(),
            input_json: "{}".to_string(),
            timestamp: chrono::Utc::now(),
        });
        assert!(app.has_pending_approvals());
    }

    #[test]
    fn test_pending_messages_queue() {
        let mut app = App::new(Some("agent-1".to_string()));
        app.mode = Mode::Sending;

        // Type some text and press Enter while in Sending mode
        app.input.insert_str("queued message");
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let action = app.handle_sending_key(key);
        assert!(action.is_none()); // No action returned - message is queued
        assert_eq!(app.pending_messages.len(), 1);
        assert_eq!(app.pending_messages[0], "queued message");
        assert!(app.input.is_empty()); // Input cleared
    }

    #[test]
    fn test_queued_message_drains_on_done() {
        let mut app = App::new(Some("agent-1".to_string()));
        app.mode = Mode::Sending;
        app.streaming = Some(StreamingMessage::default());
        app.pending_messages.push_back("queued msg".to_string());

        app.handle_response(Response::Done);

        // Should stay in Sending mode and have a queued action
        assert_eq!(app.mode, Mode::Sending);
        assert!(app.queued_action.is_some());
        assert!(app.pending_messages.is_empty());
    }

    #[test]
    fn test_handle_approval_key_approve() {
        let mut app = App::new(None);
        app.pending_approvals.push(PendingApproval {
            agent_id: "agent-1".to_string(),
            request_id: "req-1".to_string(),
            tool_id: "tool-1".to_string(),
            tool_name: "bash".to_string(),
            input_json: "{}".to_string(),
            timestamp: chrono::Utc::now(),
        });
        app.selected_approval = Some(0);

        let key = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
        let action = app.handle_approval_key(key);
        assert!(matches!(action, Some(Action::ApproveSelected)));
    }

    #[test]
    fn test_handle_approval_key_deny() {
        let mut app = App::new(None);
        app.pending_approvals.push(PendingApproval {
            agent_id: "agent-1".to_string(),
            request_id: "req-1".to_string(),
            tool_id: "tool-1".to_string(),
            tool_name: "bash".to_string(),
            input_json: "{}".to_string(),
            timestamp: chrono::Utc::now(),
        });
        app.selected_approval = Some(0);

        let key = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE);
        let action = app.handle_approval_key(key);
        assert!(matches!(action, Some(Action::DenySelected)));
    }

    #[test]
    fn test_handle_approval_key_approve_all() {
        let mut app = App::new(None);
        app.pending_approvals.push(PendingApproval {
            agent_id: "agent-1".to_string(),
            request_id: "req-1".to_string(),
            tool_id: "tool-1".to_string(),
            tool_name: "bash".to_string(),
            input_json: "{}".to_string(),
            timestamp: chrono::Utc::now(),
        });
        app.selected_approval = Some(0);

        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let action = app.handle_approval_key(key);
        assert!(matches!(action, Some(Action::ApproveAllSelected)));
    }

    #[test]
    fn test_handle_approval_key_navigation() {
        let mut app = App::new(None);
        app.pending_approvals.push(PendingApproval {
            agent_id: "agent-1".to_string(),
            request_id: "req-1".to_string(),
            tool_id: "tool-1".to_string(),
            tool_name: "bash".to_string(),
            input_json: "{}".to_string(),
            timestamp: chrono::Utc::now(),
        });
        app.pending_approvals.push(PendingApproval {
            agent_id: "agent-1".to_string(),
            request_id: "req-2".to_string(),
            tool_id: "tool-2".to_string(),
            tool_name: "read".to_string(),
            input_json: "{}".to_string(),
            timestamp: chrono::Utc::now(),
        });
        app.selected_approval = Some(0);

        // Navigate down
        let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_approval_key(key);
        assert_eq!(app.selected_approval, Some(1));

        // Navigate down at end (should stay at max)
        app.handle_approval_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected_approval, Some(1));

        // Navigate up
        let key = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        app.handle_approval_key(key);
        assert_eq!(app.selected_approval, Some(0));

        // Navigate up at start (should stay at 0)
        app.handle_approval_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.selected_approval, Some(0));
    }
}
