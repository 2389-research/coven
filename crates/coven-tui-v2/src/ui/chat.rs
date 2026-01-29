// ABOUTME: Chat history rendering
// ABOUTME: Displays messages and streaming response

use crate::app::App;
use crate::types::{Mode, Role, ToolStatus};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = vec![];

    // Render past messages
    for msg in &app.messages {
        let time = msg.timestamp.format("%H:%M").to_string();
        let header = match msg.role {
            Role::User => Line::from(vec![
                Span::styled("You", Style::default().bold()),
                Span::styled(format!(" {}", time), Style::default().dim()),
            ]),
            Role::Assistant | Role::System => Line::from(vec![
                Span::styled("Agent", Style::default().cyan().bold()),
                Span::styled(format!(" {}", time), Style::default().dim()),
            ]),
        };
        lines.push(header);

        // Thinking block
        if msg.thinking.is_some() {
            lines.push(Line::from(Span::styled(
                "  [thinking...]",
                Style::default().dim(),
            )));
        }

        // Tool uses
        for tool in &msg.tool_uses {
            let icon = match tool.status {
                ToolStatus::Complete => "✓",
                ToolStatus::Error => "✗",
                ToolStatus::Running => "→",
            };
            lines.push(Line::from(Span::styled(
                format!("  {} {}", icon, tool.name),
                Style::default().dim(),
            )));
        }

        // Content
        for line in msg.content.lines() {
            lines.push(Line::from(format!("  {}", line)));
        }
        lines.push(Line::from(""));
    }

    // Render streaming message
    if let Some(streaming) = &app.streaming {
        let agent_name = app
            .selected_agent
            .as_ref()
            .and_then(|id| app.agents.iter().find(|a| a.id == *id))
            .map(|a| a.name.as_str())
            .unwrap_or("Agent");

        lines.push(Line::from(vec![Span::styled(
            agent_name,
            Style::default().cyan().bold(),
        )]));

        // Thinking
        if streaming.thinking.is_some() {
            lines.push(Line::from(Span::styled(
                "  [thinking...]",
                Style::default().dim(),
            )));
        }

        // Tools
        for tool in &streaming.tool_uses {
            let icon = match tool.status {
                ToolStatus::Running => app.throbber_char().to_string(),
                ToolStatus::Complete => "✓".to_string(),
                ToolStatus::Error => "✗".to_string(),
            };
            lines.push(Line::from(Span::styled(
                format!("  {} {}", icon, tool.name),
                Style::default().dim(),
            )));
        }

        // Content
        for line in streaming.content.lines() {
            lines.push(Line::from(format!("  {}", line)));
        }

        // Streaming cursor
        if app.mode == Mode::Sending {
            lines.push(Line::from(Span::styled(
                format!("  {}", app.throbber_char()),
                Style::default().dim(),
            )));
        }
    }

    // Empty state
    if lines.is_empty() && app.selected_agent.is_some() {
        lines.push(Line::from(Span::styled(
            "Start typing to chat...",
            Style::default().dim(),
        )));
    }

    let para = Paragraph::new(lines).scroll((app.scroll_offset as u16, 0));
    f.render_widget(para, area);
}
