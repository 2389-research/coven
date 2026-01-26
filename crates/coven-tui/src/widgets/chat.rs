// ABOUTME: Chat display widget with streaming support.
// ABOUTME: Renders messages and active stream with throbber.

use coven_client::Message;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::theme::Theme;

/// Braille dot characters for throbber animation.
const THROBBER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Represents an active streaming response from an agent.
pub struct ActiveStream {
    pub text_buffer: String,
    pub thinking_buffer: String,
    pub tool_lines: Vec<String>,
    /// Tool state updates: (state, detail) pairs for visualization.
    pub tool_states: Vec<(String, String)>,
    pub throbber_frame: usize,
}

impl ActiveStream {
    pub fn new() -> Self {
        Self {
            text_buffer: String::new(),
            thinking_buffer: String::new(),
            tool_lines: Vec::new(),
            tool_states: Vec::new(),
            throbber_frame: 0,
        }
    }

    /// Get the current throbber character for animation.
    pub fn throbber_char(&self) -> char {
        THROBBER_CHARS[self.throbber_frame % THROBBER_CHARS.len()]
    }

    /// Advance the throbber animation by one frame.
    pub fn tick_throbber(&mut self) {
        self.throbber_frame = self.throbber_frame.wrapping_add(1);
    }
}

impl Default for ActiveStream {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ChatWidget {
    messages: Vec<Message>,
    active_stream: Option<ActiveStream>,
    scroll_offset: u16,
    viewport_height: u16,
    theme: &'static Theme,
}

impl ChatWidget {
    pub fn new(theme: &'static Theme) -> Self {
        Self {
            messages: Vec::new(),
            active_stream: None,
            scroll_offset: 0,
            viewport_height: 0,
            theme,
        }
    }

    pub fn set_messages(&mut self, messages: Vec<Message>) {
        self.messages = messages;
        // Reset scroll to bottom when messages change
        self.scroll_offset = 0;
    }

    /// Get read-only access to the messages.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn set_active_stream(&mut self, stream: ActiveStream) {
        self.active_stream = Some(stream);
    }

    pub fn clear_active_stream(&mut self) {
        self.active_stream = None;
    }

    pub fn active_stream_mut(&mut self) -> Option<&mut ActiveStream> {
        self.active_stream.as_mut()
    }

    /// Advance the throbber animation if there's an active stream.
    pub fn tick_throbber(&mut self) {
        if let Some(stream) = self.active_stream.as_mut() {
            stream.tick_throbber();
        }
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn page_up(&mut self) {
        let page_size = self.viewport_height.saturating_sub(2).max(1);
        self.scroll_offset = self.scroll_offset.saturating_add(page_size);
    }

    pub fn page_down(&mut self) {
        let page_size = self.viewport_height.saturating_sub(2).max(1);
        self.scroll_offset = self.scroll_offset.saturating_sub(page_size);
    }

    /// Format a timestamp in HH:MM format using local timezone.
    fn format_timestamp(timestamp: i64) -> String {
        use std::time::{Duration, UNIX_EPOCH};
        let secs = (timestamp / 1000) as u64;
        let datetime = UNIX_EPOCH + Duration::from_secs(secs);
        let local_time = chrono::DateTime::<chrono::Local>::from(datetime);
        local_time.format("%H:%M").to_string()
    }

    /// Wrap text to fit within the given width, preserving words where possible.
    fn wrap_text(text: &str, width: usize) -> Vec<String> {
        if width == 0 {
            return vec![];
        }

        let mut lines = Vec::new();

        for line in text.lines() {
            if line.is_empty() {
                lines.push(String::new());
                continue;
            }

            let mut current_line = String::with_capacity(width);
            let mut current_line_chars = 0usize;
            for word in line.split_whitespace() {
                let word_chars = word.chars().count();
                if current_line.is_empty() {
                    // If word is longer than width, split it
                    if word_chars > width {
                        Self::split_long_word(word, width, &mut lines);
                    } else {
                        current_line.push_str(word);
                        current_line_chars = word_chars;
                    }
                } else if current_line_chars + 1 + word_chars <= width {
                    current_line.push(' ');
                    current_line.push_str(word);
                    current_line_chars += 1 + word_chars;
                } else {
                    lines.push(std::mem::take(&mut current_line));
                    current_line_chars = 0;
                    // If word is longer than width, split it
                    if word_chars > width {
                        Self::split_long_word(word, width, &mut lines);
                    } else {
                        current_line.push_str(word);
                        current_line_chars = word_chars;
                    }
                }
            }
            if !current_line.is_empty() {
                lines.push(current_line);
            }
        }

        if lines.is_empty() {
            lines.push(String::new());
        }

        lines
    }

    /// Split a long word into chunks of the given width without intermediate allocation.
    fn split_long_word(word: &str, width: usize, lines: &mut Vec<String>) {
        let mut chunk = String::with_capacity(width * 4); // 4 bytes max per UTF-8 char
        let mut char_count = 0;

        for c in word.chars() {
            if char_count >= width {
                lines.push(std::mem::take(&mut chunk));
                chunk.reserve(width * 4);
                char_count = 0;
            }
            chunk.push(c);
            char_count += 1;
        }

        if !chunk.is_empty() {
            lines.push(chunk);
        }
    }

    /// Render a single message into styled lines.
    fn render_message(&self, msg: &Message, width: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        // Header line: [HH:MM] Sender
        let time = Self::format_timestamp(msg.timestamp);
        let header_color = if msg.is_user {
            self.theme.user_message
        } else {
            self.theme.agent_message
        };

        let header = Line::from(vec![
            Span::styled(
                format!("[{}] ", time),
                Style::default().fg(self.theme.text_muted),
            ),
            Span::styled(msg.sender.clone(), Style::default().fg(header_color).bold()),
        ]);
        lines.push(header);

        // Content lines with word wrapping
        let content_width = width.saturating_sub(2); // Indent content slightly
        let wrapped = Self::wrap_text(&msg.content, content_width);
        for line_text in wrapped {
            let line = Line::from(vec![
                Span::raw("  "), // Indent
                Span::styled(line_text, Style::default().fg(self.theme.text)),
            ]);
            lines.push(line);
        }

        // Add blank line after message
        lines.push(Line::from(""));

        lines
    }

    /// Render the active stream into styled lines.
    fn render_active_stream(&self, width: usize) -> Vec<Line<'static>> {
        let Some(stream) = &self.active_stream else {
            return vec![];
        };

        let mut lines = Vec::new();
        let throbber = stream.throbber_char();
        let content_width = width.saturating_sub(2);

        // 1. Agent badge line with throbber
        let badge_line = Line::from(vec![
            Span::styled(
                "[AGENT] ",
                Style::default()
                    .fg(self.theme.agent_message)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}", throbber),
                Style::default().fg(self.theme.accent),
            ),
        ]);
        lines.push(badge_line);

        // 2. Thinking section with brain emoji
        if !stream.thinking_buffer.is_empty() {
            let thinking_header = Line::from(vec![
                Span::styled(
                    "\u{1F9E0} ", // brain emoji
                    Style::default().fg(self.theme.thinking),
                ),
                Span::styled(
                    "Thinking...",
                    Style::default()
                        .fg(self.theme.thinking)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]);
            lines.push(thinking_header);

            let wrapped = Self::wrap_text(&stream.thinking_buffer, content_width);
            for line_text in wrapped {
                let line = Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        line_text,
                        Style::default()
                            .fg(self.theme.thinking)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ]);
                lines.push(line);
            }
        }

        // 3. Tool use lines with gear emoji
        for tool_line in &stream.tool_lines {
            let line = Line::from(vec![
                Span::styled(
                    "\u{2699} ", // gear emoji
                    Style::default().fg(self.theme.tool_use),
                ),
                Span::styled(tool_line.clone(), Style::default().fg(self.theme.tool_use)),
            ]);
            lines.push(line);
        }

        // 4. Tool state lines with state-specific emojis
        for (state, detail) in &stream.tool_states {
            let emoji = tool_state_emoji(state);
            let display_text = if detail.is_empty() {
                state.clone()
            } else {
                format!("{}: {}", state, detail)
            };
            let line = Line::from(vec![
                Span::styled(
                    format!("{} ", emoji),
                    Style::default().fg(self.theme.tool_use),
                ),
                Span::styled(display_text, Style::default().fg(self.theme.tool_use)),
            ]);
            lines.push(line);
        }

        // 5. Text buffer content
        if !stream.text_buffer.is_empty() {
            let wrapped = Self::wrap_text(&stream.text_buffer, content_width);
            for line_text in wrapped {
                let line = Line::from(vec![
                    Span::raw("  "),
                    Span::styled(line_text, Style::default().fg(self.theme.text)),
                ]);
                lines.push(line);
            }
        } else if stream.thinking_buffer.is_empty()
            && stream.tool_lines.is_empty()
            && stream.tool_states.is_empty()
        {
            // If all buffers are empty, show waiting indicator
            let waiting = Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Waiting for response...",
                    Style::default()
                        .fg(self.theme.text_muted)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]);
            lines.push(waiting);
        }

        lines
    }
}

impl Widget for &ChatWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Clear the area with background color using set_style for efficiency
        buf.set_style(area, Style::default().bg(self.theme.background));

        if area.width < 3 || area.height < 1 {
            return;
        }

        let width = (area.width.saturating_sub(2)) as usize;

        // Collect all lines to render
        let mut all_lines: Vec<Line<'static>> = Vec::new();

        // Render all messages
        for msg in &self.messages {
            all_lines.extend(self.render_message(msg, width));
        }

        // Render active stream if present
        all_lines.extend(self.render_active_stream(width));

        // Calculate visible window based on scroll offset (from bottom)
        let total_lines = all_lines.len();
        let visible_height = area.height as usize;

        // scroll_offset of 0 means we're at the bottom (most recent)
        // scroll_offset of N means we've scrolled up N lines
        let scroll_offset = self.scroll_offset as usize;

        // Calculate the range of lines to display
        let end_line = total_lines.saturating_sub(scroll_offset);
        let start_line = end_line.saturating_sub(visible_height);

        // Extract visible lines - use drain to avoid cloning
        let visible_lines: Vec<Line<'static>> =
            if start_line < end_line && end_line <= all_lines.len() {
                all_lines.drain(start_line..end_line).collect()
            } else {
                vec![]
            };

        // Create paragraph and render
        let paragraph = Paragraph::new(visible_lines);
        paragraph.render(area, buf);
    }
}

/// Mutable widget trait for updating viewport height during render.
impl ChatWidget {
    pub fn render_and_update_viewport(&mut self, area: Rect, buf: &mut Buffer) {
        self.viewport_height = area.height;
        (&*self).render(area, buf);
    }
}

/// Maps tool state strings to emoji icons for display.
/// This matches the Go TUI implementation for consistency.
pub fn tool_state_emoji(state: &str) -> &'static str {
    match state {
        "pending" => "⏳",
        "awaiting_approval" => "🔐",
        "running" => "▶️",
        "completed" => "✅",
        "failed" => "❌",
        "denied" => "🚫",
        "timeout" => "⏰",
        "cancelled" => "🛑",
        _ => "⚙️",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::DEFAULT_THEME;

    fn make_test_message(content: &str, is_user: bool) -> Message {
        Message {
            id: "test-id".to_string(),
            sender: if is_user { "You" } else { "Agent" }.to_string(),
            content: content.to_string(),
            timestamp: 1705350000000, // Some fixed timestamp
            is_user,
        }
    }

    #[test]
    fn test_chat_widget_new() {
        let widget = ChatWidget::new(&DEFAULT_THEME);
        assert!(widget.messages.is_empty());
        assert!(widget.active_stream.is_none());
        assert_eq!(widget.scroll_offset, 0);
    }

    #[test]
    fn test_set_messages() {
        let mut widget = ChatWidget::new(&DEFAULT_THEME);
        widget.scroll_offset = 10; // Simulate some scrolling

        let messages = vec![
            make_test_message("Hello", true),
            make_test_message("Hi there!", false),
        ];
        widget.set_messages(messages);

        assert_eq!(widget.messages.len(), 2);
        assert_eq!(widget.scroll_offset, 0); // Should reset to bottom
    }

    #[test]
    fn test_scroll_operations() {
        let mut widget = ChatWidget::new(&DEFAULT_THEME);
        widget.viewport_height = 10;

        widget.scroll_up();
        assert_eq!(widget.scroll_offset, 1);

        widget.scroll_up();
        assert_eq!(widget.scroll_offset, 2);

        widget.scroll_down();
        assert_eq!(widget.scroll_offset, 1);

        widget.scroll_down();
        assert_eq!(widget.scroll_offset, 0);

        // Should not go below 0
        widget.scroll_down();
        assert_eq!(widget.scroll_offset, 0);
    }

    #[test]
    fn test_page_operations() {
        let mut widget = ChatWidget::new(&DEFAULT_THEME);
        widget.viewport_height = 20;

        widget.page_up();
        assert_eq!(widget.scroll_offset, 18); // viewport_height - 2

        widget.page_down();
        assert_eq!(widget.scroll_offset, 0);
    }

    #[test]
    fn test_active_stream() {
        let mut widget = ChatWidget::new(&DEFAULT_THEME);

        assert!(widget.active_stream.is_none());

        widget.set_active_stream(ActiveStream::new());
        assert!(widget.active_stream.is_some());

        if let Some(stream) = widget.active_stream_mut() {
            stream.text_buffer.push_str("Hello");
        }
        assert_eq!(widget.active_stream.as_ref().unwrap().text_buffer, "Hello");

        widget.clear_active_stream();
        assert!(widget.active_stream.is_none());
    }

    #[test]
    fn test_wrap_text() {
        // Short text that fits
        let wrapped = ChatWidget::wrap_text("Hello", 20);
        assert_eq!(wrapped, vec!["Hello"]);

        // Text that needs wrapping
        let wrapped = ChatWidget::wrap_text("Hello world how are you", 10);
        assert_eq!(wrapped, vec!["Hello", "world how", "are you"]);

        // Empty text
        let wrapped = ChatWidget::wrap_text("", 20);
        assert_eq!(wrapped, vec![""]);

        // Multiple lines
        let wrapped = ChatWidget::wrap_text("Line one\nLine two", 20);
        assert_eq!(wrapped, vec!["Line one", "Line two"]);

        // Long word that needs to be split
        let wrapped = ChatWidget::wrap_text("abcdefghij", 5);
        assert_eq!(wrapped, vec!["abcde", "fghij"]);
    }

    #[test]
    fn test_wrap_text_multibyte_utf8() {
        // CJK characters (3 bytes each) should split by character count, not byte count
        let wrapped = ChatWidget::wrap_text("日日日日日", 3);
        assert_eq!(wrapped, vec!["日日日", "日日"]);

        // Emoji (4 bytes each) should also split by character count
        let wrapped = ChatWidget::wrap_text("🎉🎊🎁🎂🎈", 2);
        assert_eq!(wrapped, vec!["🎉🎊", "🎁🎂", "🎈"]);

        // Mixed ASCII and multi-byte characters
        let wrapped = ChatWidget::wrap_text("ab日cd日ef", 4);
        assert_eq!(wrapped, vec!["ab日c", "d日ef"]);
    }

    #[test]
    fn test_wrap_text_utf8_edge_cases() {
        // Edge case from bug report: "日日" (2 chars, 6 bytes) with width=5
        // Before fix: 6 > 5 would incorrectly consider it "too long"
        // After fix: 2 <= 5, so it should fit on one line
        let wrapped = ChatWidget::wrap_text("日日", 5);
        assert_eq!(wrapped, vec!["日日"]);

        // Edge case: "日" (1 char, 3 bytes) with width=2
        // Before fix: 3 > 2 would incorrectly try to split it
        // After fix: 1 <= 2, so it should fit without splitting
        let wrapped = ChatWidget::wrap_text("日", 2);
        assert_eq!(wrapped, vec!["日"]);

        // Multiple words with CJK characters should wrap on word boundaries
        let wrapped = ChatWidget::wrap_text("日日 月月 火火", 4);
        assert_eq!(wrapped, vec!["日日", "月月", "火火"]);

        // Word boundary test: "日日" and "月月" fit together with space (2+1+2=5)
        let wrapped = ChatWidget::wrap_text("日日 月月", 5);
        assert_eq!(wrapped, vec!["日日 月月"]);

        // Word boundary test: doesn't fit (2+1+3=6 > 5)
        let wrapped = ChatWidget::wrap_text("日日 月月月", 5);
        assert_eq!(wrapped, vec!["日日", "月月月"]);
    }

    #[test]
    fn test_format_timestamp() {
        // Test a known timestamp (2024-01-15 14:32:00 UTC would be different in local time)
        // Just verify it produces something in HH:MM format
        let formatted = ChatWidget::format_timestamp(1705329120000);
        assert_eq!(formatted.len(), 5);
        assert!(formatted.contains(':'));
    }

    #[test]
    fn test_throbber_animation() {
        let mut stream = ActiveStream::new();

        // Verify initial state
        assert_eq!(stream.throbber_frame, 0);
        assert_eq!(stream.throbber_char(), '⠋'); // First char

        // Tick through all frames
        let expected_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        for (i, expected) in expected_chars.iter().enumerate() {
            assert_eq!(stream.throbber_char(), *expected, "frame {}", i);
            stream.tick_throbber();
        }

        // After 10 ticks, should wrap back to first char
        assert_eq!(stream.throbber_frame, 10);
        assert_eq!(stream.throbber_char(), '⠋');
    }

    #[test]
    fn test_chat_widget_tick_throbber() {
        let mut widget = ChatWidget::new(&DEFAULT_THEME);

        // No stream active, tick should be a no-op
        widget.tick_throbber();
        assert!(widget.active_stream.is_none());

        // With stream active, tick should advance frame
        widget.set_active_stream(ActiveStream::new());
        assert_eq!(widget.active_stream.as_ref().unwrap().throbber_frame, 0);

        widget.tick_throbber();
        assert_eq!(widget.active_stream.as_ref().unwrap().throbber_frame, 1);

        widget.tick_throbber();
        assert_eq!(widget.active_stream.as_ref().unwrap().throbber_frame, 2);
    }

    #[test]
    fn test_tool_state_emoji_known_states() {
        assert_eq!(tool_state_emoji("pending"), "⏳");
        assert_eq!(tool_state_emoji("awaiting_approval"), "🔐");
        assert_eq!(tool_state_emoji("running"), "▶️");
        assert_eq!(tool_state_emoji("completed"), "✅");
        assert_eq!(tool_state_emoji("failed"), "❌");
        assert_eq!(tool_state_emoji("denied"), "🚫");
        assert_eq!(tool_state_emoji("timeout"), "⏰");
        assert_eq!(tool_state_emoji("cancelled"), "🛑");
    }

    #[test]
    fn test_tool_state_emoji_unknown_state() {
        // Unknown states should return default gear emoji
        assert_eq!(tool_state_emoji("unknown"), "⚙️");
        assert_eq!(tool_state_emoji(""), "⚙️");
        assert_eq!(tool_state_emoji("unspecified"), "⚙️");
        assert_eq!(tool_state_emoji("some_random_state"), "⚙️");
    }

    #[test]
    fn test_active_stream_tool_states() {
        let mut stream = ActiveStream::new();
        assert!(stream.tool_states.is_empty());

        // Add tool states
        stream.tool_states.push((
            "pending".to_string(),
            "Reading file config.json".to_string(),
        ));
        stream
            .tool_states
            .push(("running".to_string(), "Executing bash command".to_string()));

        assert_eq!(stream.tool_states.len(), 2);
        assert_eq!(stream.tool_states[0].0, "pending");
        assert_eq!(stream.tool_states[0].1, "Reading file config.json");
        assert_eq!(stream.tool_states[1].0, "running");
    }
}
