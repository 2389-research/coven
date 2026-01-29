// ABOUTME: Tool approval dialog rendering
// ABOUTME: Centered modal showing pending tool approvals with y/n/a options

use super::centered_rect;
use crate::app::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

/// Render the tool approval dialog overlay
pub fn render(f: &mut Frame, app: &App) {
    if app.pending_approvals.is_empty() {
        return;
    }

    // Center overlay: 70% width, 50% height
    let area = centered_rect(70, 50, f.area());

    // Clear background
    f.render_widget(Clear, area);

    // Get selected approval
    let approval = match app.get_selected_approval() {
        Some(a) => a,
        None => return,
    };

    // Format input JSON nicely (with indentation)
    let formatted_input = format_json(&approval.input_json);

    // Build the content
    let approval_count = app.pending_approvals.len();
    let current_idx = app.selected_approval.unwrap_or(0) + 1;

    let content = format!(
        "Tool: {}\n\n\
         Input:\n{}\n\n\
         ─────────────────────────────────────────\n\
         [y/Enter] Approve  [n/Esc] Deny  [a] Approve All",
        approval.tool_name, formatted_input
    );

    let title = if approval_count > 1 {
        format!(" Tool Approval Required ({}/{}) ", current_idx, approval_count)
    } else {
        " Tool Approval Required ".to_string()
    };

    let paragraph = Paragraph::new(content)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().yellow())
                .title(title)
                .title_style(Style::default().bold().yellow()),
        );

    f.render_widget(paragraph, area);
}

/// Format JSON string with indentation for display
fn format_json(json_str: &str) -> String {
    // Try to parse and pretty-print
    match serde_json::from_str::<serde_json::Value>(json_str) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| json_str.to_string()),
        Err(_) => json_str.to_string(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn test_format_json_valid() {
        let json = r#"{"command": "ls -la"}"#;
        let formatted = format_json(json);
        assert!(formatted.contains("command"));
        assert!(formatted.contains("ls -la"));
    }

    #[test]
    fn test_format_json_invalid() {
        let invalid = "not json";
        let formatted = format_json(invalid);
        assert_eq!(formatted, "not json");
    }

    #[test]
    fn test_centered_rect() {
        let area = Rect::new(0, 0, 100, 100);
        let centered = centered_rect(50, 50, area);
        // Should be roughly centered
        assert!(centered.x > 0);
        assert!(centered.y > 0);
        assert!(centered.width < 100);
        assert!(centered.height < 100);
    }
}
