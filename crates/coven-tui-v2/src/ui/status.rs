// ABOUTME: Bottom status bar rendering
// ABOUTME: Shows agent, connection, tokens, keybinds

use crate::app::App;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let mut spans: Vec<Span> = vec![];

    // Agent + model
    if let Some(agent_id) = &app.selected_agent {
        if let Some(agent) = app.agents.iter().find(|a| a.id == *agent_id) {
            let model = agent.model.as_deref().unwrap_or(&agent.backend);
            spans.push(Span::styled(
                format!(" {} ({}) ", agent.name, model),
                Style::default().bold(),
            ));
        }
    } else {
        spans.push(Span::styled(" No agent ", Style::default().dim()));
    }

    // Connection status
    let conn = if app.connected { "●" } else { "○" };
    let conn_style = if app.connected {
        Style::default().green()
    } else {
        Style::default().red()
    };
    spans.push(Span::styled(conn, conn_style));
    spans.push(Span::raw(" "));

    // Working directory
    if let Some(dir) = &app.session.working_dir {
        let display = truncate_path(dir, 20);
        spans.push(Span::styled(format!("│ {} ", display), Style::default().dim()));
    }

    // Tokens
    let input_tokens = format_tokens(app.session.total_input_tokens);
    let output_tokens = format_tokens(app.session.total_output_tokens);
    spans.push(Span::styled(
        format!("│ {}↑ {}↓ ", input_tokens, output_tokens),
        Style::default().dim(),
    ));

    // Error or Ctrl+C hint
    if let Some(err) = &app.error {
        spans.push(Span::styled(format!("│ ✗ {} ", err), Style::default().red()));
    } else if app.show_ctrl_c_hint() {
        spans.push(Span::styled(
            "│ Press Ctrl+C again to quit ",
            Style::default().yellow(),
        ));
    }

    // Keybinds (right side - we'll just append for now)
    spans.push(Span::styled(
        "│ Ctrl+Space: agents │ Ctrl+Q: quit ",
        Style::default().dim(),
    ));

    let line = Line::from(spans);
    let para = Paragraph::new(line).style(Style::default().on_dark_gray());
    f.render_widget(para, area);
}

fn format_tokens(n: u32) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        format!("...{}", &path[path.len() - max_len + 3..])
    }
}
