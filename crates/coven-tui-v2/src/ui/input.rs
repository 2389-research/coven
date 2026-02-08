// ABOUTME: Input area rendering
// ABOUTME: Black background with top/bottom borders

use crate::app::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear};
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let queue_count = app.pending_messages.len();
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray).bg(Color::Rgb(0, 0, 0)))
        .style(Style::default().bg(Color::Rgb(0, 0, 0)));

    let block = if queue_count > 0 {
        block
            .title(format!(" [{} queued] ", queue_count))
            .title_style(Style::default().fg(Color::Yellow).bg(Color::Rgb(0, 0, 0)))
    } else {
        block
    };

    // Clear the area first so the background fills completely
    f.render_widget(Clear, area);

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Render textarea
    f.render_widget(&app.input, inner);
}
