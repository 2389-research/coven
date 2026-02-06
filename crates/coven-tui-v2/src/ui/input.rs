// ABOUTME: Input area rendering
// ABOUTME: Black background with top/bottom borders

use crate::app::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear};
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray).bg(Color::Rgb(0, 0, 0)))
        .style(Style::default().bg(Color::Rgb(0, 0, 0)));

    // Clear the area first so the background fills completely
    f.render_widget(Clear, area);

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Render textarea
    f.render_widget(&app.input, inner);
}
