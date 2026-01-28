// ABOUTME: Telegram message context abstraction for unified handling.
// ABOUTME: Provides type-safe representation of private chats, groups, and threads.

use crate::config::ResponseMode;

/// Represents the context where a Telegram message originated.
/// Abstracts away the differences between private chats, groups, and reply threads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramContext {
    /// Private 1:1 chat with bot.
    Private { chat_id: i64 },
    /// Group or supergroup message (not a reply to bot).
    Group { chat_id: i64 },
    /// Reply thread in group (reply to bot's message).
    Thread { chat_id: i64, thread_id: i32 },
}

impl TelegramContext {
    /// Create context from Telegram message data.
    ///
    /// # Arguments
    /// * `chat_id` - The Telegram chat ID (positive for private, negative for groups)
    /// * `thread_id` - The thread/topic ID if in a forum topic (None if not a thread)
    /// * `is_private` - Whether this is a private 1:1 chat
    pub fn from_message(chat_id: i64, thread_id: Option<i32>, is_private: bool) -> Self {
        if is_private {
            TelegramContext::Private { chat_id }
        } else if let Some(thread_id) = thread_id {
            TelegramContext::Thread { chat_id, thread_id }
        } else {
            TelegramContext::Group { chat_id }
        }
    }

    /// Get the chat ID for this context.
    pub fn chat_id(&self) -> i64 {
        match self {
            TelegramContext::Private { chat_id } => *chat_id,
            TelegramContext::Group { chat_id } => *chat_id,
            TelegramContext::Thread { chat_id, .. } => *chat_id,
        }
    }

    /// Get the thread ID if this is a thread context.
    pub fn thread_id(&self) -> Option<i32> {
        match self {
            TelegramContext::Thread { thread_id, .. } => Some(*thread_id),
            _ => None,
        }
    }

    /// Determine whether the bot should respond based on context and configuration.
    ///
    /// Response logic:
    /// - Always respond in private chats
    /// - Always respond in threads (maintains conversation flow)
    /// - In groups: respond if @mentioned OR if response_mode is All
    pub fn should_respond(&self, response_mode: ResponseMode, is_mention: bool) -> bool {
        match self {
            // Always respond in private chats
            TelegramContext::Private { .. } => true,
            // Always respond in threads to maintain conversation
            TelegramContext::Thread { .. } => true,
            // In groups: check mention and response mode
            TelegramContext::Group { .. } => is_mention || response_mode == ResponseMode::All,
        }
    }

    /// Check if this is a private chat context.
    pub fn is_private(&self) -> bool {
        matches!(self, TelegramContext::Private { .. })
    }

    /// Check if this is a thread context.
    pub fn is_thread(&self) -> bool {
        matches!(self, TelegramContext::Thread { .. })
    }

    /// Check if this is a group context (not thread).
    pub fn is_group(&self) -> bool {
        matches!(self, TelegramContext::Group { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_from_message_private() {
        let ctx = TelegramContext::from_message(12345, None, true);
        assert!(matches!(ctx, TelegramContext::Private { .. }));
        assert_eq!(ctx.chat_id(), 12345);
        assert!(ctx.thread_id().is_none());
        assert!(ctx.is_private());
        assert!(!ctx.is_group());
        assert!(!ctx.is_thread());
    }

    #[test]
    fn test_context_from_message_group() {
        let ctx = TelegramContext::from_message(-100123456, None, false);
        assert!(matches!(ctx, TelegramContext::Group { .. }));
        assert_eq!(ctx.chat_id(), -100123456);
        assert!(ctx.thread_id().is_none());
        assert!(!ctx.is_private());
        assert!(ctx.is_group());
        assert!(!ctx.is_thread());
    }

    #[test]
    fn test_context_from_message_thread() {
        let ctx = TelegramContext::from_message(-100123456, Some(42), false);
        assert!(matches!(ctx, TelegramContext::Thread { .. }));
        assert_eq!(ctx.chat_id(), -100123456);
        assert_eq!(ctx.thread_id(), Some(42));
        assert!(!ctx.is_private());
        assert!(!ctx.is_group());
        assert!(ctx.is_thread());
    }

    #[test]
    fn test_should_respond_private_always() {
        let ctx = TelegramContext::Private { chat_id: 12345 };
        assert!(ctx.should_respond(ResponseMode::Mention, false));
        assert!(ctx.should_respond(ResponseMode::Mention, true));
        assert!(ctx.should_respond(ResponseMode::All, false));
        assert!(ctx.should_respond(ResponseMode::All, true));
    }

    #[test]
    fn test_should_respond_thread_always() {
        let ctx = TelegramContext::Thread {
            chat_id: -100123456,
            thread_id: 42,
        };
        assert!(ctx.should_respond(ResponseMode::Mention, false));
        assert!(ctx.should_respond(ResponseMode::Mention, true));
        assert!(ctx.should_respond(ResponseMode::All, false));
        assert!(ctx.should_respond(ResponseMode::All, true));
    }

    #[test]
    fn test_should_respond_group_mention_mode() {
        let ctx = TelegramContext::Group {
            chat_id: -100123456,
        };
        // Mention mode: only respond if mentioned
        assert!(!ctx.should_respond(ResponseMode::Mention, false));
        assert!(ctx.should_respond(ResponseMode::Mention, true));
    }

    #[test]
    fn test_should_respond_group_all_mode() {
        let ctx = TelegramContext::Group {
            chat_id: -100123456,
        };
        // All mode: respond regardless of mention
        assert!(ctx.should_respond(ResponseMode::All, false));
        assert!(ctx.should_respond(ResponseMode::All, true));
    }
}
