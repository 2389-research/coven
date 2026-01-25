// ABOUTME: UI rendering for single mode TUI
// ABOUTME: Renders status bar, messages viewport, and input area

#![allow(dead_code)] // Functions exported via mod.rs pub use

use super::app::{App, AppStatus};
use super::messages::{Role, ToolStatus};
use super::theme;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

/// Main UI render function
pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Status bar
            Constraint::Min(5),    // Messages
            Constraint::Length(5), // Input area
        ])
        .split(f.area());

    render_status_bar(f, app, chunks[0]);
    render_messages(f, app, chunks[1]);
    render_input(f, app, chunks[2]);

    // Render overlays on top
    if app.show_help {
        render_help_overlay(f, f.area());
    }
    if app.pending_approval.is_some() {
        render_approval_overlay(f, app, f.area());
    }
}

fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    // Check for pending exit state (Ctrl+C pressed once)
    let status_text = if app.pending_exit() {
        ("Press Ctrl+C again to exit", theme::WARNING_AMBER)
    } else {
        match app.status {
            AppStatus::Ready => ("Ready", theme::SUCCESS_JADE),
            AppStatus::Thinking => ("Thinking...", theme::WARNING_AMBER),
            AppStatus::Streaming => ("Streaming...", theme::ACCENT_SAGE),
            AppStatus::AwaitingApproval => ("Approval Required", theme::WARNING_AMBER),
            AppStatus::Error => ("Error", theme::ERROR_RUBY),
        }
    };

    let time = chrono::Local::now().format("%H:%M").to_string();

    // Build the left-side content spans
    let title = " fold-agent ";
    let backend_text = format!(" {} ", app.backend);
    let working_dir_text = format!(" {} ", truncate(&app.working_dir, 30));
    let status_fmt = format!(" {} ", status_text.0);
    let time_suffix = format!("{} ", time);

    // Calculate actual content width to determine padding
    // Each "|" separator is 1 char, we have 3 separators
    let content_width = title.chars().count()
        + 1 // |
        + backend_text.chars().count()
        + 1 // |
        + working_dir_text.chars().count()
        + 1 // |
        + status_fmt.chars().count()
        + time_suffix.chars().count();

    let padding_width = (area.width as usize).saturating_sub(content_width);

    let line = Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(theme::ACCENT_SKY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("|", Style::default().fg(theme::DIM_INK)),
        Span::styled(backend_text, Style::default().fg(theme::SOFT_PAPER)),
        Span::styled("|", Style::default().fg(theme::DIM_INK)),
        Span::styled(working_dir_text, Style::default().fg(theme::DIM_INK)),
        Span::styled("|", Style::default().fg(theme::DIM_INK)),
        Span::styled(status_fmt, Style::default().fg(status_text.1)),
        Span::raw(" ".repeat(padding_width)),
        Span::styled(time_suffix, Style::default().fg(theme::DIM_INK)),
    ]);

    let para = Paragraph::new(line).style(Style::default().bg(theme::DEEP_INK));
    f.render_widget(para, area);
}

fn render_messages(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::LEFT | Borders::RIGHT)
        .border_style(Style::default().fg(theme::DIM_INK));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = vec![];

    for msg in &app.messages {
        // Add spacing between messages
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }

        // Message header
        let (role_name, role_color) = match msg.role {
            Role::User => ("You", theme::ACCENT_CORAL),
            Role::Agent => ("Agent", theme::ACCENT_SAGE),
            Role::System => ("System", theme::DIM_INK),
        };

        let timestamp = msg.timestamp.format("%H:%M").to_string();
        lines.push(Line::from(vec![
            Span::styled(
                role_name,
                Style::default().fg(role_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" ({})", timestamp),
                Style::default().fg(theme::DIM_INK),
            ),
        ]));

        // Tool activity
        for tool in &msg.tools {
            let (status_char, status_color) = match tool.status {
                ToolStatus::Pending => ("o", theme::WARNING_AMBER),
                ToolStatus::Executing => ("*", theme::ACCENT_SKY),
                ToolStatus::Completed => ("+", theme::SUCCESS_JADE),
                ToolStatus::Failed => ("x", theme::ERROR_RUBY),
                ToolStatus::Denied => ("-", theme::DIM_INK),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", status_char),
                    Style::default().fg(status_color),
                ),
                Span::styled(&tool.name, Style::default().fg(theme::ACCENT_SKY)),
                Span::styled(
                    format!(": {}", truncate(&tool.input_preview, 50)),
                    Style::default().fg(theme::DIM_INK),
                ),
            ]));
        }

        // Message content
        if !msg.content.is_empty() {
            for content_line in msg.content.lines() {
                lines.push(Line::from(Span::styled(
                    content_line,
                    Style::default().fg(theme::SOFT_PAPER),
                )));
            }
        }

        // Streaming indicator - show while streaming (even with content)
        if msg.is_streaming {
            lines.push(Line::from(Span::styled(
                "* streaming...",
                Style::default().fg(theme::ACCENT_SAGE),
            )));
        }
    }

    // Calculate scroll
    let visible_height = inner.height as usize;
    let total_lines = lines.len();
    let scroll = if app.follow_mode {
        total_lines.saturating_sub(visible_height)
    } else {
        app.scroll_offset
            .min(total_lines.saturating_sub(visible_height))
    };

    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll as u16, 0));
    f.render_widget(para, inner);
}

fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if app.status == AppStatus::Ready {
            theme::ACCENT_SKY
        } else {
            theme::DIM_INK
        }))
        .title(Span::styled(
            " Message ",
            Style::default().fg(theme::DIM_INK),
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Input text with cursor
    let input_text = if app.input.is_empty() {
        vec![Line::from(Span::styled(
            "Type a message... (Shift+Enter for newline)",
            Style::default().fg(theme::DIM_INK),
        ))]
    } else {
        app.input
            .lines()
            .map(|l| Line::from(Span::styled(l, Style::default().fg(theme::SOFT_PAPER))))
            .collect()
    };

    let para = Paragraph::new(input_text);
    f.render_widget(para, inner);

    // Show cursor position
    if app.status == AppStatus::Ready {
        let (cx, cy) = cursor_position(&app.input, app.input_cursor);
        f.set_cursor_position((inner.x + cx as u16, inner.y + cy as u16));
    }
}

fn render_help_overlay(f: &mut Frame, area: Rect) {
    let help_text = vec![
        Line::from(Span::styled(
            "Keyboard Shortcuts",
            Style::default()
                .fg(theme::ACCENT_SKY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Enter        ", Style::default().fg(theme::ACCENT_CORAL)),
            Span::raw("Send message"),
        ]),
        Line::from(vec![
            Span::styled("Shift+Enter  ", Style::default().fg(theme::ACCENT_CORAL)),
            Span::raw("New line"),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+C       ", Style::default().fg(theme::ACCENT_CORAL)),
            Span::raw("Cancel (2x to quit)"),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+D       ", Style::default().fg(theme::ACCENT_CORAL)),
            Span::raw("Quit immediately"),
        ]),
        Line::from(vec![
            Span::styled("Ctrl+L       ", Style::default().fg(theme::ACCENT_CORAL)),
            Span::raw("Clear screen"),
        ]),
        Line::from(vec![
            Span::styled("?/Ctrl+H     ", Style::default().fg(theme::ACCENT_CORAL)),
            Span::raw("Toggle help"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "Up/Down or j/k   ",
                Style::default().fg(theme::ACCENT_CORAL),
            ),
            Span::raw("Scroll messages"),
        ]),
        Line::from(vec![
            Span::styled("g/G          ", Style::default().fg(theme::ACCENT_CORAL)),
            Span::raw("Top/Bottom"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Press any key to close",
            Style::default().fg(theme::DIM_INK),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::ACCENT_SKY))
        .title(Span::styled(
            " Help ",
            Style::default().fg(theme::ACCENT_SKY),
        ));

    let help_area = centered_rect(50, 60, area);
    f.render_widget(ratatui::widgets::Clear, help_area);
    let para = Paragraph::new(help_text).block(block);
    f.render_widget(para, help_area);
}

fn render_approval_overlay(f: &mut Frame, app: &App, area: Rect) {
    let approval = match &app.pending_approval {
        Some(a) => a,
        None => return,
    };

    let lines = vec![
        Line::from(Span::styled(
            "Tool Approval Required",
            Style::default()
                .fg(theme::WARNING_AMBER)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![Span::styled(
            &approval.tool_name,
            Style::default()
                .fg(theme::ACCENT_SKY)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(Span::styled(
            truncate(&approval.input_json, 200),
            Style::default().fg(theme::DIM_INK),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "[Y]",
                Style::default()
                    .fg(theme::SUCCESS_JADE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Allow  "),
            Span::styled(
                "[N]",
                Style::default()
                    .fg(theme::ERROR_RUBY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Deny  "),
            Span::styled(
                "[A]",
                Style::default()
                    .fg(theme::WARNING_AMBER)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Allow All"),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::WARNING_AMBER))
        .title(Span::styled(
            " Approval ",
            Style::default().fg(theme::WARNING_AMBER),
        ));

    let approval_area = centered_rect(60, 40, area);
    f.render_widget(ratatui::widgets::Clear, approval_area);
    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, approval_area);
}

/// Calculate cursor position based on newlines in text.
///
/// Limitation: This only handles explicit newlines, not soft-wrapping. If a line
/// exceeds the input area width, the cursor x-coordinate will extend beyond the
/// visible area. Full wrap-aware positioning would require replicating ratatui's
/// internal line-breaking logic. Since the input Paragraph doesn't use Wrap, this
/// is acceptable for typical input lengths. For very long single-line inputs, the
/// cursor may appear off-screen.
fn cursor_position(text: &str, cursor: usize) -> (usize, usize) {
    let before_cursor = &text[..cursor];
    let lines: Vec<&str> = before_cursor.split('\n').collect();
    let y = lines.len().saturating_sub(1);
    let x = lines.last().map(|l| l.chars().count()).unwrap_or(0);
    (x, y)
}

/// Create a centered rectangle
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Truncate string with ellipsis
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}
