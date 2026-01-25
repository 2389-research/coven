// ABOUTME: Input handling for single mode TUI
// ABOUTME: Processes keyboard events and updates app state

#![allow(dead_code)] // Types and functions will be used by later tasks in the implementation

use super::app::{App, AppStatus};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;

/// Result of handling an input event
pub enum InputResult {
    /// Continue normal operation
    Continue,
    /// User wants to send a message
    SendMessage(String),
    /// User approved a tool
    ApproveTool,
    /// User denied a tool
    DenyTool,
    /// User wants to allow all tools
    ApproveAll,
    /// User wants to cancel current operation
    Cancel,
    /// User wants to quit
    Quit,
}

/// Handle a key event and return the result
pub fn handle_key(app: &mut App, key: KeyEvent) -> InputResult {
    // Help overlay - any key closes it
    if app.show_help {
        app.show_help = false;
        return InputResult::Continue;
    }

    // Approval overlay - handle approval keys
    if app.pending_approval.is_some() {
        return handle_approval_key(app, key);
    }

    // Global shortcuts
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => return InputResult::Quit,
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            // Double Ctrl+C to exit
            if app.pending_exit() {
                return InputResult::Quit;
            }
            app.last_ctrl_c = Some(Instant::now());
            return InputResult::Cancel;
        }
        (KeyModifiers::CONTROL, KeyCode::Char('h')) => {
            app.show_help = true;
            return InputResult::Continue;
        }
        (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
            app.messages.clear();
            return InputResult::Continue;
        }
        (_, KeyCode::Char('?')) if app.input.is_empty() => {
            app.show_help = true;
            return InputResult::Continue;
        }
        _ => {}
    }

    // Navigation when input is empty
    if app.input.is_empty() && app.status == AppStatus::Ready {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                app.scroll_offset = app.scroll_offset.saturating_add(1);
                app.follow_mode = false;
                return InputResult::Continue;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.scroll_offset = app.scroll_offset.saturating_sub(1);
                app.follow_mode = false;
                return InputResult::Continue;
            }
            KeyCode::Char('g') => {
                app.scroll_offset = 0;
                app.follow_mode = false;
                return InputResult::Continue;
            }
            KeyCode::Char('G') => {
                app.follow_mode = true;
                return InputResult::Continue;
            }
            KeyCode::PageUp => {
                app.scroll_offset = app.scroll_offset.saturating_sub(10);
                app.follow_mode = false;
                return InputResult::Continue;
            }
            KeyCode::PageDown => {
                app.scroll_offset = app.scroll_offset.saturating_add(10);
                app.follow_mode = true; // PageDown re-enables follow
                return InputResult::Continue;
            }
            _ => {}
        }
    }

    // Input handling
    if app.status == AppStatus::Ready {
        match (key.modifiers, key.code) {
            // Send message
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if !app.input.is_empty() {
                    let msg = app.take_input();
                    return InputResult::SendMessage(msg);
                }
            }
            // Newline
            (KeyModifiers::SHIFT, KeyCode::Enter) => {
                app.input_char('\n');
            }
            // Character input
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                app.input_char(c);
            }
            // Backspace
            (_, KeyCode::Backspace) => {
                app.input_backspace();
            }
            // Delete
            (_, KeyCode::Delete) => {
                app.input_delete();
            }
            // Cursor movement
            (_, KeyCode::Left) => {
                app.cursor_left();
            }
            (_, KeyCode::Right) => {
                app.cursor_right();
            }
            (_, KeyCode::Home) => {
                app.input_cursor = 0;
            }
            (_, KeyCode::End) => {
                app.input_cursor = app.input.len();
            }
            // Escape clears input
            (_, KeyCode::Esc) => {
                app.input.clear();
                app.input_cursor = 0;
            }
            _ => {}
        }
    }

    InputResult::Continue
}

fn handle_approval_key(app: &mut App, key: KeyEvent) -> InputResult {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.pending_approval = None;
            app.status = AppStatus::Streaming;
            InputResult::ApproveTool
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.pending_approval = None;
            app.status = AppStatus::Ready;
            InputResult::DenyTool
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            app.auto_approve_all = true;
            app.pending_approval = None;
            app.status = AppStatus::Streaming;
            InputResult::ApproveAll
        }
        _ => InputResult::Continue,
    }
}
