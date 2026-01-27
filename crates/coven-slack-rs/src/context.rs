// ABOUTME: Slack message context abstraction for unified handling.
// ABOUTME: Provides type-safe representation of channels, threads, and DMs.

use crate::config::ResponseMode;

/// Represents the context where a Slack message originated.
/// Abstracts away the differences between channels, threads, and DMs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlackContext {
    /// Message in a public or private channel (not in a thread).
    Channel {
        channel_id: String,
    },
    /// Message in a thread within a channel.
    Thread {
        channel_id: String,
        thread_ts: String,
    },
    /// Direct message (1:1 or group DM).
    DirectMessage {
        channel_id: String,
    },
}

impl SlackContext {
    /// Create context from Slack event data.
    ///
    /// # Arguments
    /// * `channel_id` - The Slack channel ID
    /// * `thread_ts` - The thread timestamp (None if not in a thread)
    /// * `is_dm` - Whether this is a direct message channel
    pub fn from_event(channel_id: String, thread_ts: Option<String>, is_dm: bool) -> Self {
        if is_dm {
            SlackContext::DirectMessage { channel_id }
        } else if let Some(thread_ts) = thread_ts {
            SlackContext::Thread {
                channel_id,
                thread_ts,
            }
        } else {
            SlackContext::Channel { channel_id }
        }
    }

    /// Get the channel ID for this context.
    pub fn channel_id(&self) -> &str {
        match self {
            SlackContext::Channel { channel_id } => channel_id,
            SlackContext::Thread { channel_id, .. } => channel_id,
            SlackContext::DirectMessage { channel_id } => channel_id,
        }
    }

    /// Get the thread timestamp if this is a thread context.
    pub fn thread_ts(&self) -> Option<&str> {
        match self {
            SlackContext::Thread { thread_ts, .. } => Some(thread_ts),
            _ => None,
        }
    }

    /// Determine whether the bot should respond based on context and configuration.
    ///
    /// Response logic:
    /// - Always respond in threads (maintains conversation flow)
    /// - Always respond in DMs
    /// - In channels: respond if @mentioned OR if response_mode is All
    pub fn should_respond(&self, response_mode: ResponseMode, is_mention: bool) -> bool {
        match self {
            // Always respond in threads to maintain conversation
            SlackContext::Thread { .. } => true,
            // Always respond in DMs
            SlackContext::DirectMessage { .. } => true,
            // In channels: check mention and response mode
            SlackContext::Channel { .. } => {
                is_mention || response_mode == ResponseMode::All
            }
        }
    }

    /// Check if this is a direct message context.
    pub fn is_dm(&self) -> bool {
        matches!(self, SlackContext::DirectMessage { .. })
    }

    /// Check if this is a thread context.
    pub fn is_thread(&self) -> bool {
        matches!(self, SlackContext::Thread { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_from_event_channel() {
        let ctx = SlackContext::from_event("C123".to_string(), None, false);
        assert!(matches!(ctx, SlackContext::Channel { .. }));
        assert_eq!(ctx.channel_id(), "C123");
        assert!(ctx.thread_ts().is_none());
    }

    #[test]
    fn test_context_from_event_thread() {
        let ctx = SlackContext::from_event("C123".to_string(), Some("1234.5678".to_string()), false);
        assert!(matches!(ctx, SlackContext::Thread { .. }));
        assert_eq!(ctx.channel_id(), "C123");
        assert_eq!(ctx.thread_ts(), Some("1234.5678"));
    }

    #[test]
    fn test_context_from_event_dm() {
        let ctx = SlackContext::from_event("D123".to_string(), None, true);
        assert!(matches!(ctx, SlackContext::DirectMessage { .. }));
        assert_eq!(ctx.channel_id(), "D123");
        assert!(ctx.is_dm());
    }

    #[test]
    fn test_should_respond_thread_always() {
        let ctx = SlackContext::Thread {
            channel_id: "C123".to_string(),
            thread_ts: "1234.5678".to_string(),
        };
        assert!(ctx.should_respond(ResponseMode::Mention, false));
        assert!(ctx.should_respond(ResponseMode::All, false));
    }

    #[test]
    fn test_should_respond_dm_always() {
        let ctx = SlackContext::DirectMessage {
            channel_id: "D123".to_string(),
        };
        assert!(ctx.should_respond(ResponseMode::Mention, false));
        assert!(ctx.should_respond(ResponseMode::All, false));
    }

    #[test]
    fn test_should_respond_channel_mention_mode() {
        let ctx = SlackContext::Channel {
            channel_id: "C123".to_string(),
        };
        assert!(!ctx.should_respond(ResponseMode::Mention, false));
        assert!(ctx.should_respond(ResponseMode::Mention, true));
    }

    #[test]
    fn test_should_respond_channel_all_mode() {
        let ctx = SlackContext::Channel {
            channel_id: "C123".to_string(),
        };
        assert!(ctx.should_respond(ResponseMode::All, false));
        assert!(ctx.should_respond(ResponseMode::All, true));
    }
}
