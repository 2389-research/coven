// ABOUTME: Status bar widgets for top and bottom of screen.
// ABOUTME: Shows branding, agent info, connection status, token usage, and keybindings.

use fold_client::{Agent, ConnectionStatus, UsageInfo};
use ratatui::prelude::*;

use crate::app::Focus;
use crate::theme::Theme;

/// Extended info for the top status bar (Go TUI style).
pub struct TopBarInfo<'a> {
    pub agents: &'a [Agent],
    pub current_agent_id: Option<&'a str>,
    pub connection_status: ConnectionStatus,
    pub is_streaming: bool,
    pub session_usage: &'a UsageInfo,
    pub gateway_url: &'a str,
}

/// Extended info for the bottom status bar (Go TUI style).
pub struct BottomBarInfo {
    pub connection_status: ConnectionStatus,
    pub is_streaming: bool,
    pub focus: Focus,
    /// Number of messages queued while streaming
    pub queue_count: usize,
}

pub struct StatusBar;

/// Format a token count for display (e.g., 1234 -> "1.2k", 1234567 -> "1.2m")
fn format_token_count(count: i32) -> String {
    if count >= 1_000_000 {
        format!("{:.1}m", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

/// Shorten gateway URL for display (remove http://, show host:port only)
fn shorten_gateway(url: &str) -> String {
    url.strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url)
        .to_string()
}

/// Shorten a path for display by replacing home directory with ~ and truncating if needed.
/// If the path is longer than max_width, it will be truncated from the beginning with "..." prefix.
pub fn shorten_path(path: &str, max_width: usize) -> String {
    if path.is_empty() {
        return String::new();
    }

    // Replace home directory with ~
    let shortened = if let Ok(home) = std::env::var("HOME") {
        if path.starts_with(&home) {
            format!("~{}", &path[home.len()..])
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    };

    // If path fits within max_width, return it
    if shortened.len() <= max_width {
        return shortened;
    }

    // Truncate from the beginning with ellipsis prefix
    let ellipsis = "...";
    let available = max_width.saturating_sub(ellipsis.len());
    if available == 0 {
        return ellipsis[..max_width].to_string();
    }

    // Take the last `available` characters
    let start = shortened.len().saturating_sub(available);
    format!("{}{}", ellipsis, &shortened[start..])
}

impl StatusBar {
    /// Render the top status bar with branding, agent info, connection status, and token usage.
    ///
    /// Layout: "⬡ FOLD │ ◆ Agent Name (backend) │ tokens ─── N agents · gateway"
    pub fn render_top(area: Rect, buf: &mut Buffer, theme: &Theme, info: &TopBarInfo) {
        // Background fill
        let style = Style::default().bg(theme.surface);
        buf.set_style(area, style);

        // Find current agent
        let current_agent = info
            .current_agent_id
            .and_then(|id| info.agents.iter().find(|a| a.id == id));

        // Connection indicator (diamond style, per Go TUI)
        let (connection_icon, connection_color) = if info.is_streaming {
            ("◎", theme.accent) // Streaming: target symbol
        } else {
            match info.connection_status {
                ConnectionStatus::Connected => ("◆", theme.success), // Connected: filled diamond
                ConnectionStatus::Connecting => ("◌", theme.warning), // Connecting: dotted circle
                ConnectionStatus::Disconnected => ("◇", theme.error), // Disconnected: empty diamond
            }
        };

        // Build left side: "⬡ FOLD │ ◆ Agent Name (backend) │ tokens"
        let mut left_spans = vec![
            // Hexagon + FOLD branding
            Span::styled(" ⬡ ", Style::default().fg(theme.success).bold()),
            Span::styled("FOLD", Style::default().fg(theme.success).bold()),
            // Separator
            Span::styled(" │ ", Style::default().fg(theme.text_muted)),
            // Connection indicator
            Span::styled(connection_icon, Style::default().fg(connection_color)),
            Span::raw(" "),
        ];

        // Agent name and backend
        if let Some(agent) = current_agent {
            left_spans.push(Span::styled(
                agent.name.clone(),
                Style::default().fg(theme.accent),
            ));
            left_spans.push(Span::styled(
                format!(" ({})", agent.backend),
                Style::default().fg(theme.text_muted),
            ));
        } else {
            left_spans.push(Span::styled(
                "No agent",
                Style::default().fg(theme.text_muted),
            ));
        }

        // Token count if available
        let total_tokens = info.session_usage.input_tokens + info.session_usage.output_tokens;
        if total_tokens > 0 {
            left_spans.push(Span::styled(" │ ", Style::default().fg(theme.text_muted)));
            left_spans.push(Span::styled(
                format!(
                    "{}↑ {}↓",
                    format_token_count(info.session_usage.input_tokens),
                    format_token_count(info.session_usage.output_tokens)
                ),
                Style::default().fg(theme.text_muted),
            ));
        }

        let left_line = Line::from(left_spans);
        let left_width = left_line.width();

        // Build right side: "N/M agents · gateway"
        let connected_count = info.agents.iter().filter(|a| a.connected).count();
        let total_count = info.agents.len();
        let gateway_short = shorten_gateway(info.gateway_url);

        let right_text = format!(
            "{}/{} agents · {} ",
            connected_count, total_count, gateway_short
        );
        let right_spans = vec![Span::styled(
            &right_text,
            Style::default().fg(theme.text_muted),
        )];
        let right_line = Line::from(right_spans);
        let right_width = right_line.width();

        // Render left side
        buf.set_line(area.x, area.y, &left_line, area.width);

        // Render right side if there's room
        if area.width as usize > left_width + right_width + 2 {
            let right_x = area.x + area.width - right_width as u16;
            buf.set_line(right_x, area.y, &right_line, right_width as u16);
        }
    }

    /// Render the bottom status bar with connection status and keybindings.
    ///
    /// Layout: "◉ connected                    PgUp/Dn · ⌃Space · ⌃Q"
    pub fn render_bottom(area: Rect, buf: &mut Buffer, theme: &Theme, info: &BottomBarInfo) {
        // Background fill
        let style = Style::default().bg(theme.surface_dim);
        buf.set_style(area, style);

        // Status indicator and text (left side)
        let (status_icon, status_color, status_text) = if info.is_streaming {
            ("◎", theme.accent, "streaming")
        } else {
            match info.connection_status {
                ConnectionStatus::Connected => ("◉", theme.success, "connected"),
                ConnectionStatus::Connecting => ("◌", theme.warning, "connecting"),
                ConnectionStatus::Disconnected => ("○", theme.error, "offline"),
            }
        };

        let mut left_spans = vec![
            Span::raw(" "),
            Span::styled(status_icon, Style::default().fg(status_color)),
            Span::raw(" "),
            Span::styled(status_text, Style::default().fg(theme.text_muted)),
        ];

        // Show queue count if messages are queued while streaming
        if info.queue_count > 0 {
            left_spans.push(Span::styled(" │ ", Style::default().fg(theme.text_muted)));
            left_spans.push(Span::styled(
                format!("{} queued", info.queue_count),
                Style::default().fg(theme.warning),
            ));
        }

        let left_line = Line::from(left_spans);
        let left_width = left_line.width();

        // Keybindings (right side) - more compact format
        let hints: Vec<(&str, &str)> = match info.focus {
            Focus::Input => vec![("PgUp/Dn", ""), ("⌃Space", ""), ("⌃Q", "")],
            Focus::Picker => vec![("Enter", ""), ("Esc", ""), ("↑↓", "")],
        };

        // Build right side spans
        let mut right_spans: Vec<Span> = Vec::new();
        for (i, (key, _)) in hints.iter().enumerate() {
            if i > 0 {
                right_spans.push(Span::styled(" · ", Style::default().fg(theme.text_muted)));
            }
            right_spans.push(Span::styled(*key, Style::default().fg(theme.text_muted)));
        }
        right_spans.push(Span::raw(" "));

        let right_line = Line::from(right_spans);
        let right_width = right_line.width();

        // Render left side
        buf.set_line(area.x, area.y, &left_line, area.width);

        // Render right side if there's room
        if area.width as usize > left_width + right_width + 2 {
            let right_x = area.x + area.width - right_width as u16;
            buf.set_line(right_x, area.y, &right_line, right_width as u16);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::DEFAULT_THEME;

    fn make_test_agent(id: &str, name: &str, backend: &str, connected: bool) -> Agent {
        Agent {
            id: id.to_string(),
            name: name.to_string(),
            backend: backend.to_string(),
            working_dir: "/tmp".to_string(),
            connected,
        }
    }

    #[test]
    fn test_render_top_no_agent() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        let info = TopBarInfo {
            agents: &[],
            current_agent_id: None,
            connection_status: ConnectionStatus::Disconnected,
            is_streaming: false,
            session_usage: &UsageInfo::default(),
            gateway_url: "http://localhost:50051",
        };
        StatusBar::render_top(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("FOLD"));
        assert!(content.contains("No agent"));
        assert!(content.contains("◇")); // Disconnected diamond
    }

    #[test]
    fn test_render_top_with_agent_connected() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 1));
        let agents = vec![make_test_agent("agent-1", "test-agent", "claude", true)];
        let info = TopBarInfo {
            agents: &agents,
            current_agent_id: Some("agent-1"),
            connection_status: ConnectionStatus::Connected,
            is_streaming: false,
            session_usage: &UsageInfo::default(),
            gateway_url: "http://localhost:50051",
        };
        StatusBar::render_top(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("FOLD"));
        assert!(content.contains("test-agent"));
        assert!(content.contains("(claude)"));
        assert!(content.contains("◆")); // Connected diamond
        assert!(content.contains("1/1 agents"));
    }

    #[test]
    fn test_render_top_streaming() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 1));
        let agents = vec![make_test_agent("agent-1", "test-agent", "claude", true)];
        let info = TopBarInfo {
            agents: &agents,
            current_agent_id: Some("agent-1"),
            connection_status: ConnectionStatus::Connected,
            is_streaming: true,
            session_usage: &UsageInfo::default(),
            gateway_url: "http://localhost:50051",
        };
        StatusBar::render_top(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("◎")); // Streaming target symbol
    }

    #[test]
    fn test_render_top_connecting() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        let info = TopBarInfo {
            agents: &[],
            current_agent_id: None,
            connection_status: ConnectionStatus::Connecting,
            is_streaming: false,
            session_usage: &UsageInfo::default(),
            gateway_url: "http://localhost:50051",
        };
        StatusBar::render_top(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("◌")); // Connecting dotted circle
    }

    #[test]
    fn test_render_top_with_token_usage() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 1));
        let agents = vec![make_test_agent("agent-1", "test-agent", "claude", true)];
        let usage = UsageInfo {
            input_tokens: 12500,
            output_tokens: 8200,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            thinking_tokens: 0,
        };
        let info = TopBarInfo {
            agents: &agents,
            current_agent_id: Some("agent-1"),
            connection_status: ConnectionStatus::Connected,
            is_streaming: false,
            session_usage: &usage,
            gateway_url: "http://localhost:50051",
        };
        StatusBar::render_top(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("12.5k↑"));
        assert!(content.contains("8.2k↓"));
    }

    #[test]
    fn test_render_top_shows_agent_count() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 1));
        let agents = vec![
            make_test_agent("agent-1", "agent-one", "claude", true),
            make_test_agent("agent-2", "agent-two", "openai", false),
            make_test_agent("agent-3", "agent-three", "claude", true),
        ];
        let info = TopBarInfo {
            agents: &agents,
            current_agent_id: Some("agent-1"),
            connection_status: ConnectionStatus::Connected,
            is_streaming: false,
            session_usage: &UsageInfo::default(),
            gateway_url: "http://localhost:50051",
        };
        StatusBar::render_top(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("2/3 agents")); // 2 connected out of 3
    }

    #[test]
    fn test_render_top_shows_gateway() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 1));
        let info = TopBarInfo {
            agents: &[],
            current_agent_id: None,
            connection_status: ConnectionStatus::Disconnected,
            is_streaming: false,
            session_usage: &UsageInfo::default(),
            gateway_url: "http://myserver.local:8080",
        };
        StatusBar::render_top(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("myserver.local:8080"));
    }

    #[test]
    fn test_render_top_no_tokens_displayed_when_zero() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 1));
        let agents = vec![make_test_agent("agent-1", "test-agent", "claude", true)];
        let info = TopBarInfo {
            agents: &agents,
            current_agent_id: Some("agent-1"),
            connection_status: ConnectionStatus::Connected,
            is_streaming: false,
            session_usage: &UsageInfo::default(),
            gateway_url: "http://localhost:50051",
        };
        StatusBar::render_top(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        // Token arrows should not appear when zero
        assert!(!content.contains("↑"));
        assert!(!content.contains("↓"));
    }

    #[test]
    fn test_format_token_count() {
        assert_eq!(format_token_count(0), "0");
        assert_eq!(format_token_count(999), "999");
        assert_eq!(format_token_count(1000), "1.0k");
        assert_eq!(format_token_count(1500), "1.5k");
        assert_eq!(format_token_count(12345), "12.3k");
        assert_eq!(format_token_count(999999), "1000.0k");
        assert_eq!(format_token_count(1000000), "1.0m");
        assert_eq!(format_token_count(1500000), "1.5m");
    }

    #[test]
    fn test_shorten_gateway() {
        assert_eq!(shorten_gateway("http://localhost:50051"), "localhost:50051");
        assert_eq!(
            shorten_gateway("https://example.com:8080"),
            "example.com:8080"
        );
        assert_eq!(shorten_gateway("myserver:1234"), "myserver:1234");
    }

    #[test]
    fn test_render_bottom_input_focus_connected() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        let info = BottomBarInfo {
            connection_status: ConnectionStatus::Connected,
            is_streaming: false,
            focus: Focus::Input,
            queue_count: 0,
        };
        StatusBar::render_bottom(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("◉")); // Connected icon
        assert!(content.contains("connected"));
        assert!(content.contains("PgUp/Dn"));
        assert!(content.contains("⌃Space"));
        assert!(content.contains("⌃Q"));
    }

    #[test]
    fn test_render_bottom_input_focus_streaming() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        let info = BottomBarInfo {
            connection_status: ConnectionStatus::Connected,
            is_streaming: true,
            focus: Focus::Input,
            queue_count: 0,
        };
        StatusBar::render_bottom(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("◎")); // Streaming icon
        assert!(content.contains("streaming"));
    }

    #[test]
    fn test_render_bottom_picker_focus() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        let info = BottomBarInfo {
            connection_status: ConnectionStatus::Connected,
            is_streaming: false,
            focus: Focus::Picker,
            queue_count: 0,
        };
        StatusBar::render_bottom(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("Enter"));
        assert!(content.contains("Esc"));
        assert!(content.contains("↑↓"));
    }

    #[test]
    fn test_render_bottom_disconnected() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        let info = BottomBarInfo {
            connection_status: ConnectionStatus::Disconnected,
            is_streaming: false,
            focus: Focus::Input,
            queue_count: 0,
        };
        StatusBar::render_bottom(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("○")); // Offline icon
        assert!(content.contains("offline"));
    }

    #[test]
    fn test_render_bottom_connecting() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        let info = BottomBarInfo {
            connection_status: ConnectionStatus::Connecting,
            is_streaming: false,
            focus: Focus::Input,
            queue_count: 0,
        };
        StatusBar::render_bottom(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("◌")); // Connecting icon
        assert!(content.contains("connecting"));
    }

    #[test]
    fn test_render_bottom_with_queued_messages() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        let info = BottomBarInfo {
            connection_status: ConnectionStatus::Connected,
            is_streaming: true,
            focus: Focus::Input,
            queue_count: 3,
        };
        StatusBar::render_bottom(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(content.contains("◎")); // Streaming icon
        assert!(content.contains("streaming"));
        assert!(content.contains("3 queued"));
    }

    #[test]
    fn test_render_bottom_no_queue_when_count_zero() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        let info = BottomBarInfo {
            connection_status: ConnectionStatus::Connected,
            is_streaming: true,
            focus: Focus::Input,
            queue_count: 0,
        };
        StatusBar::render_bottom(buf.area, &mut buf, &DEFAULT_THEME, &info);

        let content = buf.content().iter().map(|c| c.symbol()).collect::<String>();
        assert!(!content.contains("queued"));
    }

    #[test]
    fn test_shorten_path_replaces_home_with_tilde() {
        // Test basic home directory replacement
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/testuser".to_string());
        let path = format!("{}/projects/myapp", home);
        let result = shorten_path(&path, 80);
        assert!(
            result.starts_with("~/"),
            "Expected path to start with ~/, got: {}",
            result
        );
        assert!(result.contains("projects/myapp"));
    }

    #[test]
    fn test_shorten_path_truncates_long_paths() {
        // Test that very long paths get truncated with ellipsis prefix
        let long_path =
            "/very/long/path/that/goes/on/and/on/and/on/forever/to/some/deeply/nested/directory";
        let result = shorten_path(long_path, 30);
        assert!(result.len() <= 30, "Path should be truncated to max_width");
        assert!(
            result.starts_with("..."),
            "Truncated path should start with ellipsis"
        );
    }

    #[test]
    fn test_shorten_path_preserves_short_paths() {
        // Short paths should not be modified (except home replacement)
        let short_path = "/tmp/foo";
        let result = shorten_path(short_path, 80);
        assert_eq!(result, "/tmp/foo");
    }

    #[test]
    fn test_shorten_path_empty_returns_empty() {
        let result = shorten_path("", 80);
        assert_eq!(result, "");
    }

    #[test]
    fn test_shorten_path_with_home_and_truncation() {
        // When path with ~ is still too long, truncate it
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/testuser".to_string());
        let path = format!(
            "{}/very/long/nested/directory/structure/that/keeps/going",
            home
        );
        let result = shorten_path(&path, 25);
        assert!(result.len() <= 25);
        // Should still use ellipsis for truncated paths
        assert!(
            result.starts_with("...") || result.starts_with("~"),
            "Truncated path should start with ... or ~, got: {}",
            result
        );
    }
}
