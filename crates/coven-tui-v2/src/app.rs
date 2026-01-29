// ABOUTME: Central application state and event handling
// ABOUTME: Single struct holds all state, mutations happen in handle_* methods

use crate::client::Response;
use crate::types::{
    Agent, Message, MessageTokens, Mode, PersistedState, PendingApproval, Role, SessionMetadata,
    StreamingMessage, ToolStatus, ToolUse,
};
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
                    let agent_id = agent.id.clone();
                    let agent_model = agent.model.clone().unwrap_or_default();
                    drop(filtered);
                    self.selected_agent = Some(agent_id);
                    self.session.model = agent_model;
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
            Response::ToolApprovalRequest {
                agent_id,
                request_id,
                tool_id,
                tool_name,
                input_json,
            } => {
                let approval = PendingApproval {
                    agent_id,
                    request_id,
                    tool_id,
                    tool_name,
                    input_json,
                    timestamp: chrono::Utc::now(),
                };
                self.pending_approvals.push(approval);
                // Select the first approval if none selected
                if self.selected_approval.is_none() {
                    self.selected_approval = Some(0);
                }
            }
        }
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
