// ABOUTME: Application state for single mode TUI
// ABOUTME: Manages messages, input, status, and approval state

#![allow(dead_code)] // Types will be used by later tasks in the implementation

use super::messages::ChatMessage;
use std::collections::HashSet;
use std::time::Instant;

/// Current application status
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppStatus {
    Ready,
    Thinking,
    Streaming,
    AwaitingApproval,
    Error,
}

/// Pending tool approval request
#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub tool_id: String,
    pub tool_name: String,
    pub input_json: String,
}

/// Main application state
pub struct App {
    // Identity
    pub agent_name: String,
    pub agent_id: String,
    pub backend: String,
    pub working_dir: String,

    // Chat state
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub input_cursor: usize,

    // View state
    pub scroll_offset: usize,
    pub follow_mode: bool, // Auto-scroll to bottom

    // Status
    pub status: AppStatus,
    pub error_message: Option<String>,

    // Approval state
    pub pending_approval: Option<PendingApproval>,
    pub auto_approve_all: bool,
    pub approved_tools_session: HashSet<String>,

    // Flags
    pub should_quit: bool,
    pub show_help: bool,

    // Exit confirmation (Ctrl+C twice to exit)
    pub last_ctrl_c: Option<Instant>,
}

impl App {
    pub fn new(name: &str, agent_id: &str, backend: &str, working_dir: &str) -> Self {
        Self {
            agent_name: name.to_string(),
            agent_id: agent_id.to_string(),
            backend: backend.to_string(),
            working_dir: working_dir.to_string(),
            messages: vec![],
            input: String::new(),
            input_cursor: 0,
            scroll_offset: 0,
            follow_mode: true,
            status: AppStatus::Ready,
            error_message: None,
            pending_approval: None,
            auto_approve_all: false,
            approved_tools_session: HashSet::new(),
            should_quit: false,
            show_help: false,
            last_ctrl_c: None,
        }
    }

    /// Check if we're in the "press Ctrl+C again to exit" state
    pub fn pending_exit(&self) -> bool {
        self.last_ctrl_c
            .map(|t| t.elapsed().as_secs_f32() < 2.0)
            .unwrap_or(false)
    }

    /// Check if a tool requires approval
    pub fn needs_approval(&self, tool_name: &str) -> bool {
        if self.auto_approve_all {
            return false;
        }
        // Normalize to lowercase for consistent lookup across backend types
        // (mux uses lowercase, CLI uses PascalCase)
        let name_lower = tool_name.to_lowercase();
        // Skip if already approved this session
        if self.approved_tools_session.contains(&name_lower) {
            return false;
        }
        // Dangerous tools that modify state
        matches!(
            name_lower.as_str(),
            "bash" | "write" | "write_file" | "edit" | "notebookedit" | "todowrite"
        )
    }

    /// Add a character to input at cursor position
    pub fn input_char(&mut self, c: char) {
        debug_assert!(
            self.input.is_char_boundary(self.input_cursor),
            "input_cursor {} is not on char boundary",
            self.input_cursor
        );
        self.input.insert(self.input_cursor, c);
        self.input_cursor += c.len_utf8();
    }

    /// Delete character before cursor
    pub fn input_backspace(&mut self) {
        if self.input_cursor > 0 {
            let prev = self.input[..self.input_cursor]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.input_cursor -= prev;
            self.input.remove(self.input_cursor);
        }
    }

    /// Delete character at cursor
    pub fn input_delete(&mut self) {
        if self.input_cursor < self.input.len() {
            self.input.remove(self.input_cursor);
        }
    }

    /// Move cursor left
    pub fn cursor_left(&mut self) {
        if self.input_cursor > 0 {
            let prev = self.input[..self.input_cursor]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.input_cursor -= prev;
        }
    }

    /// Move cursor right
    pub fn cursor_right(&mut self) {
        if self.input_cursor < self.input.len() {
            let next = self.input[self.input_cursor..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.input_cursor += next;
        }
    }

    /// Take the current input and clear it
    pub fn take_input(&mut self) -> String {
        let input = std::mem::take(&mut self.input);
        self.input_cursor = 0;
        input
    }
}
