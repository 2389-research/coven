// ABOUTME: Slack client wrapper using slack-morphism Socket Mode.
// ABOUTME: Handles WebSocket connection, event listening, and message posting.

use crate::config::SlackConfig;
use crate::context::SlackContext;
use crate::error::{BridgeError, Result};
use slack_morphism::prelude::*;
use std::sync::Arc;
use tracing::{debug, info};

/// Slack client wrapper for Socket Mode communication.
pub struct CovenSlackClient {
    client: Arc<SlackHyperClient>,
    bot_token: SlackApiToken,
    bot_user_id: SlackUserId,
}

impl CovenSlackClient {
    /// Create a new Slack client and authenticate.
    pub async fn new(config: &SlackConfig) -> Result<Self> {
        info!("Initializing Slack client");

        let connector = SlackClientHyperConnector::new()
            .map_err(|e| BridgeError::Slack(format!("Failed to create Slack connector: {}", e)))?;
        let client = Arc::new(slack_morphism::SlackClient::new(connector));

        let bot_token_value: SlackApiTokenValue = config.bot_token.clone().into();
        let bot_token = SlackApiToken::new(bot_token_value);

        // Test authentication and get bot user ID
        let session = client.open_session(&bot_token);
        let auth_response = session
            .auth_test()
            .await
            .map_err(|e| BridgeError::Slack(format!("Auth test failed: {}", e)))?;

        let bot_user_id = auth_response.user_id;
        info!(bot_user_id = %bot_user_id, "Slack authentication successful");

        Ok(Self {
            client,
            bot_token,
            bot_user_id,
        })
    }

    /// Get the bot's user ID.
    pub fn bot_user_id(&self) -> &SlackUserId {
        &self.bot_user_id
    }

    /// Get a reference to the underlying Slack client.
    pub fn inner(&self) -> &Arc<SlackHyperClient> {
        &self.client
    }

    /// Get the bot token for API calls.
    pub fn bot_token(&self) -> &SlackApiToken {
        &self.bot_token
    }

    /// Post a message to a Slack channel, optionally in a thread.
    pub async fn post_message(
        &self,
        channel_id: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<SlackTs> {
        debug!(channel_id = %channel_id, thread_ts = ?thread_ts, "Posting message to Slack");

        let session = self.client.open_session(&self.bot_token);

        let mut request = SlackApiChatPostMessageRequest::new(
            SlackChannelId::new(channel_id.to_string()),
            SlackMessageContent::new().with_text(text.to_string()),
        );

        if let Some(ts) = thread_ts {
            request = request.with_thread_ts(SlackTs::new(ts.to_string()));
        }

        let response = session.chat_post_message(&request).await?;

        debug!(message_ts = %response.ts, "Message posted successfully");
        Ok(response.ts)
    }

    /// Post a message with Block Kit formatting.
    pub async fn post_blocks(
        &self,
        channel_id: &str,
        blocks: Vec<SlackBlock>,
        fallback_text: &str,
        thread_ts: Option<&str>,
    ) -> Result<SlackTs> {
        debug!(channel_id = %channel_id, thread_ts = ?thread_ts, "Posting blocks to Slack");

        let session = self.client.open_session(&self.bot_token);

        let mut request = SlackApiChatPostMessageRequest::new(
            SlackChannelId::new(channel_id.to_string()),
            SlackMessageContent::new()
                .with_text(fallback_text.to_string())
                .with_blocks(blocks),
        );

        if let Some(ts) = thread_ts {
            request = request.with_thread_ts(SlackTs::new(ts.to_string()));
        }

        let response = session.chat_post_message(&request).await?;

        debug!(message_ts = %response.ts, "Blocks posted successfully");
        Ok(response.ts)
    }

    /// Check if a channel is a DM (direct message) channel.
    /// DM channels start with 'D' and group DMs start with 'G'.
    pub fn is_dm_channel(channel_id: &str) -> bool {
        channel_id.starts_with('D') || channel_id.starts_with('G')
    }

    /// Check if the bot was mentioned in the message text.
    pub fn is_mentioned(&self, text: &str) -> bool {
        let mention_pattern = format!("<@{}>", self.bot_user_id);
        text.contains(&mention_pattern)
    }

    /// Remove bot mention from text for cleaner processing.
    pub fn strip_mention(&self, text: &str) -> String {
        let mention_pattern = format!("<@{}>", self.bot_user_id);
        text.replace(&mention_pattern, "").trim().to_string()
    }

    /// Build SlackContext from message event details.
    pub fn build_context(&self, channel_id: &str, thread_ts: Option<&str>) -> SlackContext {
        let is_dm = Self::is_dm_channel(channel_id);
        SlackContext::from_event(
            channel_id.to_string(),
            thread_ts.map(|s| s.to_string()),
            is_dm,
        )
    }
}

/// Information about a received Slack message.
#[derive(Debug, Clone)]
pub struct SlackMessageInfo {
    pub channel_id: String,
    pub user_id: String,
    pub text: String,
    pub message_ts: String,
    pub thread_ts: Option<String>,
    pub is_mention: bool,
    pub context: SlackContext,
}

impl SlackMessageInfo {
    /// Create message info from a push event.
    pub fn from_message_event(
        event: &SlackMessageEvent,
        bot_user_id: &SlackUserId,
    ) -> Option<Self> {
        let channel_id = event.origin.channel.as_ref()?.to_string();
        let user_id = event.sender.user.as_ref()?.to_string();
        let text = event.content.as_ref()?.text.as_ref()?.clone();
        let message_ts = event.origin.ts.to_string();
        let thread_ts = event.origin.thread_ts.as_ref().map(|ts| ts.to_string());

        // Check for bot mention
        let mention_pattern = format!("<@{}>", bot_user_id);
        let is_mention = text.contains(&mention_pattern);

        let is_dm = channel_id.starts_with('D') || channel_id.starts_with('G');
        let context = SlackContext::from_event(channel_id.clone(), thread_ts.clone(), is_dm);

        Some(Self {
            channel_id,
            user_id,
            text,
            message_ts,
            thread_ts,
            is_mention,
            context,
        })
    }

    /// Get the thread_ts to use for replies.
    /// If already in a thread, use that. Otherwise use the message_ts to start a new thread.
    pub fn reply_thread_ts(&self, force_thread: bool) -> Option<String> {
        if let Some(ref ts) = self.thread_ts {
            // Already in a thread, reply there
            Some(ts.clone())
        } else if force_thread {
            // Start a new thread from this message
            Some(self.message_ts.clone())
        } else {
            // Reply in channel
            None
        }
    }

    /// Strip the bot mention from the text.
    pub fn text_without_mention(&self, bot_user_id: &SlackUserId) -> String {
        let mention_pattern = format!("<@{}>", bot_user_id);
        self.text.replace(&mention_pattern, "").trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_dm_channel() {
        assert!(CovenSlackClient::is_dm_channel("D12345"));
        assert!(CovenSlackClient::is_dm_channel("G12345"));
        assert!(!CovenSlackClient::is_dm_channel("C12345"));
    }
}
