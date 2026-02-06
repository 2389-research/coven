// ABOUTME: UI rendering module for coven-tui-v2
// ABOUTME: Dispatches rendering to widget modules

mod approval;
mod chat;
mod input;
mod picker;
mod status;

use crate::app::App;
use crate::types::Mode;
use ratatui::prelude::*;
use ratatui::Frame;

/// Create a centered rect using percentages of the parent rect
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Min(1),    // Chat area
        Constraint::Length(4), // Input area
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

    // Approval dialog is an overlay (shown on top of everything)
    if app.has_pending_approvals() {
        approval::render(f, app);
    }
}
