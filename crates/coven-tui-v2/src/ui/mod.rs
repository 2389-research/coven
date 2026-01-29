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
