// ABOUTME: Telegram bot wrapper using teloxide Long Polling.
// ABOUTME: Handles bot initialization, message sending, and mention detection.

use crate::config::TelegramConfig;
use crate::context::TelegramContext;
use crate::error::{BridgeError, Result};
use teloxide::prelude::*;
use teloxide::types::{Chat, ChatKind, Me, MessageId, ParseMode, ReplyParameters};
use tracing::{debug, info};

/// Telegram bot wrapper for Long Polling communication.
pub struct CovenTelegramBot {
    bot: Bot,
    me: Me,
}

impl CovenTelegramBot {
    /// Create a new Telegram bot client and authenticate.
    pub async fn new(config: &TelegramConfig) -> Result<Self> {
        info!("Initializing Telegram bot");

        let bot = Bot::new(&config.bot_token);

        // Test authentication and get bot info
        let me = bot.get_me().await.map_err(|e| {
            BridgeError::Telegram(format!("Failed to authenticate with Telegram: {}", e))
        })?;

        info!(
            bot_id = me.id.0,
            bot_username = ?me.username(),
            "Telegram authentication successful"
        );

        Ok(Self { bot, me })
    }

    /// Get the bot's user ID.
    pub fn bot_id(&self) -> UserId {
        self.me.id
    }

    /// Get the bot's username.
    pub fn bot_username(&self) -> &str {
        self.me.username()
    }

    /// Get a reference to the underlying teloxide Bot.
    pub fn inner(&self) -> &Bot {
        &self.bot
    }

    /// Get bot info.
    pub fn me(&self) -> &Me {
        &self.me
    }

    /// Send a message to a Telegram chat, optionally as a reply.
    pub async fn send_message(
        &self,
        chat_id: ChatId,
        text: &str,
        reply_to: Option<MessageId>,
    ) -> Result<Message> {
        debug!(
            chat_id = chat_id.0,
            reply_to = ?reply_to,
            "Sending message to Telegram"
        );

        let mut request = self.bot.send_message(chat_id, text);

        if let Some(msg_id) = reply_to {
            request = request.reply_parameters(ReplyParameters::new(msg_id));
        }

        // Use MarkdownV2 parsing for formatting (Markdown is deprecated)
        request = request.parse_mode(ParseMode::MarkdownV2);

        let message = request.await?;

        debug!(
            message_id = message.id.0,
            "Message sent successfully"
        );
        Ok(message)
    }

    /// Check if the bot was mentioned in the message text.
    /// Telegram uses @username format for mentions.
    pub fn is_mentioned(&self, text: &str) -> bool {
        let mention_pattern = format!("@{}", self.bot_username());
        text.contains(&mention_pattern)
    }

    /// Remove bot mention from text for cleaner processing.
    pub fn strip_mention(&self, text: &str) -> String {
        let mention_pattern = format!("@{}", self.bot_username());
        text.replace(&mention_pattern, "").trim().to_string()
    }

    /// Check if a chat is a private (1:1) chat based on chat type.
    pub fn is_private_chat(chat: &Chat) -> bool {
        matches!(chat.kind, ChatKind::Private(_))
    }

    /// Build TelegramContext from message details.
    pub fn build_context(&self, chat: &Chat, thread_id: Option<i32>) -> TelegramContext {
        let is_private = Self::is_private_chat(chat);
        TelegramContext::from_message(chat.id.0, thread_id, is_private)
    }
}

/// Information about a received Telegram message.
#[derive(Debug, Clone)]
pub struct TelegramMessageInfo {
    pub chat_id: i64,
    pub user_id: i64,
    pub text: String,
    pub message_id: MessageId,
    pub thread_id: Option<i32>,
    pub is_mention: bool,
    pub is_reply_to_bot: bool,
    pub context: TelegramContext,
}

impl TelegramMessageInfo {
    /// Create message info from a Telegram message.
    pub fn from_message(msg: &Message, bot: &CovenTelegramBot) -> Option<Self> {
        let chat_id = msg.chat.id.0;
        let user_id = msg.from.as_ref()?.id.0 as i64;
        let text = msg.text()?.to_string();
        let message_id = msg.id;
        // thread_id in teloxide is Option<ThreadId> where ThreadId wraps MessageId which wraps i32
        let thread_id: Option<i32> = msg.thread_id.map(|t| t.0.0);

        // Check for bot mention
        let is_mention = bot.is_mentioned(&text);

        // Check if this is a reply to the bot's message
        let is_reply_to_bot = msg
            .reply_to_message()
            .and_then(|reply| reply.from.as_ref())
            .map(|user| user.id == bot.bot_id())
            .unwrap_or(false);

        let is_private = CovenTelegramBot::is_private_chat(&msg.chat);
        let context = TelegramContext::from_message(chat_id, thread_id, is_private);

        Some(Self {
            chat_id,
            user_id,
            text,
            message_id,
            thread_id,
            is_mention,
            is_reply_to_bot,
            context,
        })
    }

    /// Get the message ID to use for replies.
    /// If thread_replies is enabled and not already in a thread, use this message to start one.
    pub fn reply_message_id(&self, thread_replies: bool) -> Option<MessageId> {
        if thread_replies || self.thread_id.is_some() {
            Some(self.message_id)
        } else {
            None
        }
    }

    /// Strip the bot mention from the text.
    pub fn text_without_mention(&self, bot: &CovenTelegramBot) -> String {
        bot.strip_mention(&self.text)
    }

    /// Check if this message should trigger a response (mention or reply to bot).
    pub fn is_bot_interaction(&self) -> bool {
        self.is_mention || self.is_reply_to_bot
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_mention_detection() {
        // This is a simple unit test for the mention pattern logic
        let bot_username = "test_bot";
        let mention_pattern = format!("@{}", bot_username);

        let text_with_mention = "Hello @test_bot how are you?";
        assert!(text_with_mention.contains(&mention_pattern));

        let text_without_mention = "Hello how are you?";
        assert!(!text_without_mention.contains(&mention_pattern));
    }

    #[test]
    fn test_strip_mention() {
        let bot_username = "test_bot";
        let mention_pattern = format!("@{}", bot_username);

        let text = "Hello @test_bot how are you?";
        let stripped = text.replace(&mention_pattern, "").trim().to_string();
        assert_eq!(stripped, "Hello  how are you?");

        // Test with mention at start
        let text2 = "@test_bot hello";
        let stripped2 = text2.replace(&mention_pattern, "").trim().to_string();
        assert_eq!(stripped2, "hello");
    }
}
