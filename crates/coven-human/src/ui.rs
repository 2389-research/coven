// ABOUTME: User interface rendering for the human agent TUI.
// ABOUTME: Handles drawing the terminal UI with ratatui.

use crate::app::App;
use crate::messages::InputMode;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

/// Render the full TUI frame
pub fn render(frame: &mut Frame, app: &App) {
    // Layout: header (3 lines) | messages (flex) | compose (if composing, 5 lines) | status (1 line)
    let chunks = if app.mode == InputMode::Composing {
        Layout::vertical([
            Constraint::Length(3), // header
            Constraint::Min(5),    // messages
            Constraint::Length(5), // compose area
            Constraint::Length(1), // status bar
        ])
        .split(frame.area())
    } else {
        Layout::vertical([
            Constraint::Length(3), // header
            Constraint::Min(5),    // messages
            Constraint::Length(1), // status bar
        ])
        .split(frame.area())
    };

    render_header(frame, app, chunks[0]);
    render_messages(frame, app, chunks[1]);

    if app.mode == InputMode::Composing {
        render_compose(frame, app, chunks[2]);
        render_status(frame, app, chunks[3]);
    } else {
        render_status(frame, app, chunks[2]);
    }
}

/// Render the header bar with connection status and agent info
fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let status_icon = if app.connected { "●" } else { "○" };
    let connection_status = if app.connected {
        "Connected"
    } else {
        "Disconnected"
    };

    let header_text = format!(
        " {} {} | Agent: {} | Server: {}",
        status_icon,
        connection_status,
        app.agent_id,
        if app.server_id.is_empty() {
            "---"
        } else {
            &app.server_id
        }
    );

    let header = Paragraph::new(header_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" coven human "),
    );
    frame.render_widget(header, area);
}

/// Render the scrollable message viewport
fn render_messages(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .messages
        .iter()
        .map(|msg| {
            let content = format!(
                "[{}] From {}: {}",
                msg.format_timestamp(),
                msg.sender,
                msg.content
            );
            ListItem::new(content)
        })
        .collect();

    let messages = if items.is_empty() {
        List::new(vec![ListItem::new("  Waiting for messages...")])
    } else {
        List::new(items)
    };

    let messages = messages.block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Messages ({}) ", app.messages.len())),
    );

    frame.render_widget(messages, area);
}

/// Render the compose area for typing replies
fn render_compose(frame: &mut Frame, app: &App, area: Rect) {
    let input = Paragraph::new(app.input.as_str())
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Reply (Enter to send, Esc to cancel) "),
        );
    frame.render_widget(input, area);

    // Position cursor at end of input
    let cursor_x = area.x + 1 + app.input.len() as u16;
    let cursor_y = area.y + 1;
    frame.set_cursor_position(Position::new(cursor_x.min(area.right() - 2), cursor_y));
}

/// Render the status bar
fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let mode_str = match app.mode {
        InputMode::Viewing => "[VIEW] q:quit  r:reply  j/k:scroll",
        InputMode::Composing => "[COMPOSE] Enter:send  Esc:cancel",
    };

    let status_text = format!("{} | {}", mode_str, app.status);
    let status =
        Paragraph::new(status_text).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(status, area);
}
