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
    f.render_widget(&app.input, inner);
}
