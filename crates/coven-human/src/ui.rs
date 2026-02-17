// ABOUTME: User interface rendering for the human agent TUI.
// ABOUTME: Three-row chat layout: chat history | always-visible input | status bar.

use crate::app::App;
use crate::messages::MessageDirection;
use chrono::Local;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Render the full TUI frame with 3-row layout: chat | input | status
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Min(1),    // chat area
        Constraint::Length(4), // input area (always visible)
        Constraint::Length(1), // status bar
    ])
    .split(frame.area());

    render_chat(frame, app, chunks[0]);
    render_input(frame, app, chunks[1]);
    render_status(frame, app, chunks[2]);
}

/// Render the chat area with connection info and message history
fn render_chat(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = vec![];

    // Connection info as first line
    let status_icon = if app.connected { "●" } else { "○" };
    let connection_status = if app.connected {
        "Connected"
    } else {
        "Disconnected"
    };
    let server_display = if app.server_id.is_empty() {
        "---"
    } else {
        &app.server_id
    };

    lines.push(Line::from(vec![
        Span::styled(
            format!("{} ", status_icon),
            if app.connected {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            },
        ),
        Span::styled(
            format!(
                "{} | {} | {}",
                connection_status, app.agent_id, server_display
            ),
            Style::default().dim(),
        ),
    ]));
    lines.push(Line::from(""));

    // Messages
    if app.messages.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Waiting for messages...",
            Style::default().dim(),
        )));
    } else {
        for msg in &app.messages {
            let time = msg
                .timestamp
                .with_timezone(&Local)
                .format("%H:%M")
                .to_string();

            match msg.direction {
                MessageDirection::Incoming => {
                    lines.push(Line::from(vec![
                        Span::styled(format!("{} ", time), Style::default().dim()),
                        Span::styled(
                            format!("{}: ", msg.sender),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(&msg.content),
                    ]));
                }
                MessageDirection::Outgoing => {
                    let bg = Style::default().bg(Color::Rgb(40, 40, 40));
                    lines.push(Line::from(vec![
                        Span::styled(format!("{} ", time), bg.dim()),
                        Span::styled("you: ", bg.bold()),
                        Span::styled(&msg.content, bg),
                    ]));
                }
            }
        }
    }

    // Auto-scroll: scroll_offset=0 means bottom, higher values scroll up
    let total_lines = lines.len() as u16;
    let visible_lines = area.height;
    let max_scroll = total_lines.saturating_sub(visible_lines);
    let actual_scroll = max_scroll.saturating_sub(app.scroll_offset as u16);

    let para = Paragraph::new(lines).scroll((actual_scroll, 0));
    frame.render_widget(para, area);
}

/// Render the always-visible input area with TextArea widget
fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let (title, title_style) = if app.active_request_id.is_some() {
        (
            " Reply (Enter to send) ",
            Style::default().fg(Color::Green).bg(Color::Rgb(0, 0, 0)),
        )
    } else {
        (
            " Waiting for request... ",
            Style::default().fg(Color::Yellow).bg(Color::Rgb(0, 0, 0)),
        )
    };

    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray).bg(Color::Rgb(0, 0, 0)))
        .style(Style::default().bg(Color::Rgb(0, 0, 0)))
        .title(title)
        .title_style(title_style);

    // Clear the area so background fills completely
    frame.render_widget(Clear, area);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Render the TextArea widget
    frame.render_widget(&app.input, inner);
}

/// Render the status bar with connection dot, status message, and keybinds
fn render_status(frame: &mut Frame, app: &App, area: Rect) {
    let dot = if app.connected { "●" } else { "○" };
    let dot_style = if app.connected {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Red)
    };

    let status_line = Line::from(vec![
        Span::styled(format!("{} ", dot), dot_style),
        Span::styled(&app.status, Style::default().fg(Color::White)),
        Span::styled(
            " | q:quit  PgUp/PgDn:scroll",
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let status = Paragraph::new(status_line).style(Style::default().bg(Color::Rgb(30, 30, 30)));
    frame.render_widget(status, area);
}
