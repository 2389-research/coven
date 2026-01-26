// ABOUTME: Input widget wrapping tui-textarea.
// ABOUTME: Adds message history navigation with Up/Down arrows.

use std::collections::VecDeque;
use tui_textarea::TextArea;

/// Maximum number of history entries to prevent unbounded memory growth.
const MAX_HISTORY_SIZE: usize = 1000;

pub struct InputWidget<'a> {
    textarea: TextArea<'a>,
    /// History buffer using VecDeque for O(1) removal from front when at capacity.
    history: VecDeque<String>,
    history_idx: Option<usize>,
    draft: String,
}

impl Default for InputWidget<'static> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> InputWidget<'a> {
    pub fn new() -> Self {
        Self {
            textarea: TextArea::default(),
            history: VecDeque::with_capacity(MAX_HISTORY_SIZE),
            history_idx: None,
            draft: String::new(),
        }
    }

    pub fn textarea(&self) -> &TextArea<'a> {
        &self.textarea
    }

    pub fn textarea_mut(&mut self) -> &mut TextArea<'a> {
        &mut self.textarea
    }

    /// Add a sent message to history.
    /// Messages are stored in chronological order (oldest first, newest last).
    /// History is capped at MAX_HISTORY_SIZE entries; oldest entries are removed first.
    /// Uses VecDeque for O(1) removal from front when at capacity.
    pub fn add_to_history(&mut self, message: String) {
        if !message.trim().is_empty() {
            self.history.push_back(message);
            if self.history.len() > MAX_HISTORY_SIZE {
                self.history.pop_front();
            }
        }
    }

    /// Navigate to an older message in history (Up arrow).
    /// If at a new message, saves the current input as draft.
    /// Returns true if navigation occurred.
    pub fn history_up(&mut self) -> bool {
        if self.history.is_empty() {
            return false;
        }

        match self.history_idx {
            None => {
                // Currently at new message, save draft and go to most recent history
                self.draft = self.get_content();
                let idx = self.history.len() - 1;
                self.history_idx = Some(idx);
                let content = self.history[idx].clone();
                self.set_textarea_content(&content);
                true
            }
            Some(idx) if idx > 0 => {
                // Move to older item
                let new_idx = idx - 1;
                self.history_idx = Some(new_idx);
                let content = self.history[new_idx].clone();
                self.set_textarea_content(&content);
                true
            }
            Some(_) => {
                // Already at oldest item
                false
            }
        }
    }

    /// Navigate to a newer message in history (Down arrow).
    /// At the newest history item, restores the draft.
    /// Returns true if navigation occurred.
    pub fn history_down(&mut self) -> bool {
        match self.history_idx {
            None => {
                // Already at draft, nowhere to go
                false
            }
            Some(idx) if idx < self.history.len() - 1 => {
                // Move to newer item
                let new_idx = idx + 1;
                self.history_idx = Some(new_idx);
                let content = self.history[new_idx].clone();
                self.set_textarea_content(&content);
                true
            }
            Some(_) => {
                // At newest history item, restore draft
                self.history_idx = None;
                let content = self.draft.clone();
                self.set_textarea_content(&content);
                true
            }
        }
    }

    /// Get the current text content of the input.
    pub fn get_content(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Clear the input and reset history navigation state.
    pub fn clear(&mut self) {
        self.textarea.select_all();
        self.textarea.cut();
        self.history_idx = None;
        self.draft.clear();
    }

    /// Exit history mode if currently browsing history.
    /// Called when the user starts typing while in history mode.
    pub fn exit_history_mode(&mut self) {
        if self.history_idx.is_some() {
            self.history_idx = None;
            self.draft.clear();
        }
    }

    /// Check if currently browsing history.
    pub fn is_in_history_mode(&self) -> bool {
        self.history_idx.is_some()
    }

    /// Set the textarea content, clearing existing text first.
    fn set_textarea_content(&mut self, content: &str) {
        self.textarea.select_all();
        self.textarea.cut();
        self.textarea.insert_str(content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_creates_empty_widget() {
        let widget = InputWidget::new();
        assert!(widget.history.is_empty());
        assert!(widget.history_idx.is_none());
        assert!(widget.draft.is_empty());
        assert!(widget.get_content().is_empty());
    }

    #[test]
    fn test_add_to_history() {
        let mut widget = InputWidget::new();
        widget.add_to_history("hello".to_string());
        widget.add_to_history("world".to_string());
        assert_eq!(widget.history.len(), 2);
        assert_eq!(widget.history[0], "hello");
        assert_eq!(widget.history[1], "world");
    }

    #[test]
    fn test_add_to_history_ignores_empty_messages() {
        let mut widget = InputWidget::new();
        widget.add_to_history("".to_string());
        widget.add_to_history("   ".to_string());
        widget.add_to_history("\n\t".to_string());
        assert!(widget.history.is_empty());
    }

    #[test]
    fn test_history_up_from_new_message() {
        let mut widget = InputWidget::new();
        widget.add_to_history("first".to_string());
        widget.add_to_history("second".to_string());

        // Type something as draft
        widget.textarea_mut().insert_str("my draft");

        // Navigate up - should save draft and show most recent
        assert!(widget.history_up());
        assert_eq!(widget.get_content(), "second");
        assert_eq!(widget.draft, "my draft");
        assert_eq!(widget.history_idx, Some(1));
    }

    #[test]
    fn test_history_up_through_history() {
        let mut widget = InputWidget::new();
        widget.add_to_history("first".to_string());
        widget.add_to_history("second".to_string());
        widget.add_to_history("third".to_string());

        // Navigate to history
        assert!(widget.history_up()); // third
        assert_eq!(widget.get_content(), "third");

        assert!(widget.history_up()); // second
        assert_eq!(widget.get_content(), "second");

        assert!(widget.history_up()); // first
        assert_eq!(widget.get_content(), "first");

        // At oldest, should not navigate further
        assert!(!widget.history_up());
        assert_eq!(widget.get_content(), "first");
    }

    #[test]
    fn test_history_down_restores_draft() {
        let mut widget = InputWidget::new();
        widget.add_to_history("first".to_string());
        widget.add_to_history("second".to_string());

        // Type draft and navigate up
        widget.textarea_mut().insert_str("my draft");
        widget.history_up();
        widget.history_up();
        assert_eq!(widget.get_content(), "first");

        // Navigate down
        assert!(widget.history_down());
        assert_eq!(widget.get_content(), "second");

        // Navigate down to restore draft
        assert!(widget.history_down());
        assert_eq!(widget.get_content(), "my draft");
        assert!(widget.history_idx.is_none());
    }

    #[test]
    fn test_history_down_at_draft_does_nothing() {
        let mut widget = InputWidget::new();
        widget.add_to_history("first".to_string());
        widget.textarea_mut().insert_str("draft");

        // Already at draft, should not navigate
        assert!(!widget.history_down());
        assert_eq!(widget.get_content(), "draft");
    }

    #[test]
    fn test_history_up_with_empty_history() {
        let mut widget = InputWidget::new();
        widget.textarea_mut().insert_str("typing...");

        assert!(!widget.history_up());
        assert_eq!(widget.get_content(), "typing...");
    }

    #[test]
    fn test_clear_resets_state() {
        let mut widget = InputWidget::new();
        widget.add_to_history("first".to_string());
        widget.textarea_mut().insert_str("draft");
        widget.history_up();

        widget.clear();

        assert!(widget.get_content().is_empty());
        assert!(widget.history_idx.is_none());
        assert!(widget.draft.is_empty());
        // History should be preserved
        assert_eq!(widget.history.len(), 1);
    }

    #[test]
    fn test_exit_history_mode() {
        let mut widget = InputWidget::new();
        widget.add_to_history("first".to_string());
        widget.textarea_mut().insert_str("draft");
        widget.history_up();

        assert!(widget.is_in_history_mode());
        widget.exit_history_mode();
        assert!(!widget.is_in_history_mode());
        assert!(widget.draft.is_empty());
    }

    #[test]
    fn test_is_in_history_mode() {
        let mut widget = InputWidget::new();
        widget.add_to_history("first".to_string());

        assert!(!widget.is_in_history_mode());
        widget.history_up();
        assert!(widget.is_in_history_mode());
        widget.history_down();
        assert!(!widget.is_in_history_mode());
    }

    #[test]
    fn test_textarea_accessors() {
        let mut widget = InputWidget::new();
        widget.textarea_mut().insert_str("test");
        assert_eq!(widget.textarea().lines().join(""), "test");
    }

    #[test]
    fn test_default_impl() {
        let widget: InputWidget<'static> = InputWidget::default();
        assert!(widget.history.is_empty());
    }

    #[test]
    fn test_history_size_limit() {
        let mut widget = InputWidget::new();

        // Add MAX_HISTORY_SIZE + 10 entries
        for i in 0..MAX_HISTORY_SIZE + 10 {
            widget.add_to_history(format!("message {}", i));
        }

        // History should be capped at MAX_HISTORY_SIZE
        assert_eq!(widget.history.len(), MAX_HISTORY_SIZE);

        // Oldest entries should have been removed; first entry should be "message 10"
        assert_eq!(widget.history[0], "message 10");

        // Most recent entry should be the last one added
        assert_eq!(
            widget.history[MAX_HISTORY_SIZE - 1],
            format!("message {}", MAX_HISTORY_SIZE + 9)
        );
    }
}
