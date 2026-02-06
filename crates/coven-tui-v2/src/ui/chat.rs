// ABOUTME: Chat history rendering
// ABOUTME: Displays messages and streaming response with Claude Code-style tool display

use crate::app::App;
use crate::types::{Mode, Role, StreamBlock, ToolStatus, ToolUse};
use chrono::Local;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

const INDENT: &str = "       ";
const MAX_RESULT_LINES: usize = 3;

/// Extract a clean tool name and input summary for display.
/// Handles cases where gateway sends name="tool" with input="tool:Bash input:{...}"
/// or name="Bash" with input="{\"command\":\"...\"}"
fn format_tool_header(name: &str, input: &str) -> (String, String) {
    let (display_name, raw_input) = if name == "tool" {
        // Try to extract real name from input like "tool:Bash input:{...}"
        if let Some(rest) = input.strip_prefix("tool:") {
            if let Some(space_idx) = rest.find(" input:") {
                let real_name = rest[..space_idx].to_string();
                let real_input = rest[space_idx + 7..].to_string();
                (real_name, real_input)
            } else {
                ("tool".to_string(), input.to_string())
            }
        } else {
            ("tool".to_string(), input.to_string())
        }
    } else {
        (name.to_string(), input.to_string())
    };

    // Try to extract a meaningful summary from the input
    let summary = if raw_input.starts_with('{') {
        // JSON input - try to extract a key value like "command"
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw_input) {
            // Look for common keys: command, path, file_path, query, content
            for key in &["command", "path", "file_path", "query", "url"] {
                if let Some(val) = parsed.get(key).and_then(|v| v.as_str()) {
                    let truncated: String = val.chars().take(60).collect();
                    if truncated.len() < val.len() {
                        return (display_name, format!("{}...", truncated));
                    }
                    return (display_name, truncated);
                }
            }
            // Fallback: show first key=value
            if let Some(obj) = parsed.as_object() {
                if let Some((k, v)) = obj.iter().next() {
                    let val_str = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    let truncated: String = val_str.chars().take(50).collect();
                    if truncated.len() < val_str.len() {
                        return (display_name, format!("{}: {}...", k, truncated));
                    }
                    return (display_name, format!("{}: {}", k, truncated));
                }
            }
        }
        // JSON parse failed - just truncate
        let truncated: String = raw_input.chars().take(60).collect();
        if truncated.len() < raw_input.len() {
            format!("{}...", truncated)
        } else {
            truncated
        }
    } else {
        // Non-JSON input - just truncate first line
        let first_line = raw_input.lines().next().unwrap_or("");
        let truncated: String = first_line.chars().take(60).collect();
        if truncated.len() < first_line.len() {
            format!("{}...", truncated)
        } else {
            truncated
        }
    };

    (display_name, summary)
}

/// Render a tool use with same format as messages:
///   HH:MM ⏺ ToolName(input_summary)
///           ⎿  result line 1
///              result line 2
///              … +N lines
fn render_tool<'a>(
    tool: &'a ToolUse,
    time: &str,
    throbber: Option<char>,
    lines: &mut Vec<Line<'a>>,
) {
    let dot_style = match tool.status {
        ToolStatus::Running => Style::default().green(),
        ToolStatus::Complete => Style::default().green(),
        ToolStatus::Error => Style::default().red(),
    };

    let dot = match tool.status {
        ToolStatus::Running => format!("{}", throbber.unwrap_or('⏺')),
        _ => "⏺".to_string(),
    };

    // Parse tool name and input for display
    // The gateway may send name="tool" with input="tool:Bash input:{...}"
    // or name="Bash" with input="{\"command\":\"...\"}"
    let (display_name, input_display) = format_tool_header(&tool.name, &tool.input);

    lines.push(Line::from(vec![
        Span::styled(format!("{} ", time), Style::default().dim()),
        Span::styled(format!("{} ", dot), dot_style),
        Span::styled(display_name, Style::default().bold()),
        Span::styled(format!("({})", input_display), Style::default().dim()),
    ]));

    // Tool result (truncated)
    if let Some(result) = &tool.result {
        let result_lines: Vec<&str> = result.lines().collect();
        let show_count = result_lines.len().min(MAX_RESULT_LINES);
        let remaining = result_lines.len().saturating_sub(MAX_RESULT_LINES);

        for line in result_lines.iter().take(show_count) {
            lines.push(Line::from(vec![
                Span::styled(format!("{}  ⎿  ", INDENT), Style::default().dim()),
                Span::styled(*line, Style::default().dim()),
            ]));
        }

        if remaining > 0 {
            lines.push(Line::from(Span::styled(
                format!("{}  … +{} lines", INDENT, remaining),
                Style::default().dim(),
            )));
        }
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = vec![];

    // Render past messages
    for msg in &app.messages {
        let time = msg
            .timestamp
            .with_timezone(&Local)
            .format("%H:%M")
            .to_string();

        match msg.role {
            Role::User => {
                let bg = Style::default().bg(Color::Rgb(40, 40, 40));
                // User messages are always a single text block
                let text = match msg.blocks.first() {
                    Some(StreamBlock::Text(t)) => t.as_str(),
                    _ => "",
                };
                let mut content_lines: Vec<&str> = text.lines().collect();
                if content_lines.is_empty() {
                    content_lines.push("");
                }
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", time), bg.dim()),
                    Span::styled("❯ ", bg.bold()),
                    Span::styled(content_lines[0], bg),
                ]));
                for line in content_lines.iter().skip(1) {
                    lines.push(Line::from(Span::styled(format!("{}{}", INDENT, line), bg)));
                }
            }
            Role::Assistant | Role::System => {
                let mut first_text_seen = false;

                // Thinking (completed)
                if msg.thinking.is_some() {
                    lines.push(Line::from(vec![
                        Span::styled(format!("{} ", time), Style::default().dim()),
                        Span::styled("thinking", Style::default().fg(Color::Red).italic()),
                    ]));
                }

                // Render blocks in order (preserves tool/text interleaving)
                for block in &msg.blocks {
                    match block {
                        StreamBlock::Text(text) => {
                            let text_lines: Vec<&str> = text.lines().collect();
                            if let Some(first) = text_lines.first() {
                                if !first_text_seen {
                                    lines.push(Line::from(vec![
                                        Span::styled(format!("{} ", time), Style::default().dim()),
                                        Span::styled("⏺ ", Style::default().white()),
                                        Span::raw(*first),
                                    ]));
                                    first_text_seen = true;
                                } else {
                                    lines.push(Line::from(format!("{}{}", INDENT, first)));
                                }
                            }
                            for line in text_lines.iter().skip(1) {
                                lines.push(Line::from(format!("{}{}", INDENT, line)));
                            }
                        }
                        StreamBlock::Tool(tool) => {
                            render_tool(tool, &time, None, &mut lines);
                        }
                    }
                }
            }
        }

        lines.push(Line::from(""));
    }

    // Render streaming message (ordered blocks)
    if let Some(streaming) = &app.streaming {
        let now = Local::now().format("%H:%M").to_string();
        let mut first_text_seen = false;

        // Thinking (red with throbber, shown at top)
        if streaming.thinking.is_some() {
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", now), Style::default().dim()),
                Span::styled(
                    format!("{} ", app.throbber_char()),
                    Style::default().fg(Color::Red),
                ),
                Span::styled("thinking", Style::default().fg(Color::Red).italic()),
            ]));
        }

        if streaming.blocks.is_empty() && app.mode == Mode::Sending {
            // Show throbber when nothing yet (and not thinking)
            if streaming.thinking.is_none() {
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", now), Style::default().dim()),
                    Span::styled("⏺ ", Style::default().white()),
                    Span::styled(format!("{}", app.throbber_char()), Style::default().dim()),
                ]));
            }
        } else {
            for block in &streaming.blocks {
                match block {
                    StreamBlock::Text(text) => {
                        let text_lines: Vec<&str> = text.lines().collect();
                        if let Some(first) = text_lines.first() {
                            if !first_text_seen {
                                // First text block gets timestamp + dot
                                lines.push(Line::from(vec![
                                    Span::styled(format!("{} ", now), Style::default().dim()),
                                    Span::styled("⏺ ", Style::default().white()),
                                    Span::raw(*first),
                                ]));
                                first_text_seen = true;
                            } else {
                                lines.push(Line::from(format!("{}{}", INDENT, first)));
                            }
                        }
                        for line in text_lines.iter().skip(1) {
                            lines.push(Line::from(format!("{}{}", INDENT, line)));
                        }
                    }
                    StreamBlock::Tool(tool) => {
                        render_tool(tool, &now, Some(app.throbber_char()), &mut lines);
                    }
                }
            }

            // Streaming cursor (only after text, not after a running tool)
            let last_is_tool = matches!(streaming.blocks.last(), Some(StreamBlock::Tool(_)));
            if app.mode == Mode::Sending && !last_is_tool {
                lines.push(Line::from(Span::styled(
                    format!("{}{}", INDENT, app.throbber_char()),
                    Style::default().dim(),
                )));
            }
        }
    }

    // Empty state
    if lines.is_empty() && app.selected_agent.is_some() {
        lines.push(Line::from(Span::styled(
            "Start typing to chat...",
            Style::default().dim(),
        )));
    }

    // Auto-scroll to bottom: scroll_offset=0 means "show newest", higher values scroll up
    let total_lines = lines.len() as u16;
    let visible_lines = area.height;
    let max_scroll = total_lines.saturating_sub(visible_lines);
    let actual_scroll = max_scroll.saturating_sub(app.scroll_offset as u16);

    let para = Paragraph::new(lines).scroll((actual_scroll, 0));
    f.render_widget(para, area);
}
