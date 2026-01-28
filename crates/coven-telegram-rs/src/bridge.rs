// ABOUTME: Core bridge logic connecting Telegram events to coven-gateway.
// ABOUTME: Handles message routing, bindings, command processing, and response streaming.

use crate::commands::{execute_command, Command, CommandContext};
use crate::config::Config;
use crate::error::Result;
use crate::gateway::GatewayClient;
use crate::telegram::{CovenTelegramBot, TelegramMessageInfo};

use coven_proto::client_stream_event::Payload;
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use teloxide::types::{ChatId, MessageId};
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use uuid::Uuid;

/// Chat binding information mapping a Telegram chat to a gateway conversation.
#[derive(Clone, Debug)]
pub struct ChatBinding {
    pub chat_id: i64,
    pub conversation_key: String,
}

/// The Bridge ties together Telegram and Gateway clients to route messages.
pub struct Bridge {
    config: Config,
    telegram: CovenTelegramBot,
    gateway: Arc<RwLock<GatewayClient>>,
    bindings: Arc<RwLock<HashMap<i64, ChatBinding>>>,
}

impl Bridge {
    /// Create a new Bridge with the given configuration.
    /// Establishes connections to both Telegram and the Gateway.
    pub async fn new(config: Config) -> Result<Self> {
        info!("Initializing Telegram bridge");

        // Connect to Telegram
        let telegram = CovenTelegramBot::new(&config.telegram).await?;

        // Connect to Gateway
        let gateway =
            GatewayClient::connect(&config.gateway.url, config.gateway.token.clone()).await?;

        Ok(Self {
            config,
            telegram,
            gateway: Arc::new(RwLock::new(gateway)),
            bindings: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Bind a Telegram chat to a gateway conversation.
    pub async fn bind_chat(&self, chat_id: i64, conversation_key: String) {
        let binding = ChatBinding {
            chat_id,
            conversation_key: conversation_key.clone(),
        };

        info!(
            chat_id = %chat_id,
            conversation_key = %conversation_key,
            "Binding chat to conversation"
        );

        self.bindings.write().await.insert(chat_id, binding);
    }

    /// Unbind a Telegram chat from any gateway conversation.
    pub async fn unbind_chat(&self, chat_id: i64) -> Option<ChatBinding> {
        let binding = self.bindings.write().await.remove(&chat_id);
        if let Some(ref b) = binding {
            info!(
                chat_id = %chat_id,
                conversation_key = %b.conversation_key,
                "Unbound chat from conversation"
            );
        }
        binding
    }

    /// Get the binding for a chat, if any.
    pub async fn get_binding(&self, chat_id: i64) -> Option<ChatBinding> {
        self.bindings.read().await.get(&chat_id).cloned()
    }

    /// Get a reference to the Telegram bot.
    pub fn telegram_bot(&self) -> &CovenTelegramBot {
        &self.telegram
    }

    /// Get a reference to the Gateway client (locked).
    pub fn gateway_client(&self) -> &Arc<RwLock<GatewayClient>> {
        &self.gateway
    }

    /// Get a reference to the bindings map.
    pub fn bindings(&self) -> &Arc<RwLock<HashMap<i64, ChatBinding>>> {
        &self.bindings
    }

    /// Get a reference to the config.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Handle an incoming Telegram message event.
    pub async fn handle_message(&self, msg_info: TelegramMessageInfo) -> Result<()> {
        let chat_id = msg_info.chat_id;

        // Check if chat is allowed
        if !self.config.is_chat_allowed(chat_id) {
            debug!(chat_id = %chat_id, "Message from non-allowed chat, ignoring");
            return Ok(());
        }

        // Ignore messages from the bot itself
        if msg_info.user_id == self.telegram.bot_id().0 as i64 {
            return Ok(());
        }

        // Check for /coven commands first (commands work regardless of binding)
        if let Some(command) = Command::from_message(&msg_info.text) {
            info!(
                chat_id = %chat_id,
                user_id = %msg_info.user_id,
                "Processing /coven command"
            );

            let ctx = CommandContext {
                gateway: &self.gateway,
                bindings: &self.bindings,
                chat_id,
            };

            let response = match execute_command(command, ctx).await {
                Ok(resp) => resp,
                Err(e) => format!("❌ Command error: {}", e),
            };

            // Reply to the command message
            let reply_to = msg_info.reply_message_id(self.config.bridge.thread_replies);
            self.telegram
                .send_message(ChatId(chat_id), &response, reply_to)
                .await?;

            return Ok(());
        }

        // Check response conditions
        let is_interaction = msg_info.is_bot_interaction();
        if !msg_info
            .context
            .should_respond(self.config.bridge.response_mode, is_interaction)
        {
            debug!(
                chat_id = %chat_id,
                "Message doesn't require response (response_mode={:?}, is_interaction={})",
                self.config.bridge.response_mode,
                is_interaction
            );
            return Ok(());
        }

        // Check for binding (required for forwarding to agent)
        let binding = match self.get_binding(chat_id).await {
            Some(b) => b,
            None => {
                debug!(chat_id = %chat_id, "Message from unbound chat, ignoring");
                return Ok(());
            }
        };

        // Strip bot mention from text for cleaner processing
        let text = msg_info.text_without_mention(&self.telegram);
        if text.is_empty() {
            return Ok(());
        }

        info!(
            chat_id = %chat_id,
            user_id = %msg_info.user_id,
            conversation_key = %binding.conversation_key,
            "Processing message"
        );

        // Process the message
        let reply_to = msg_info.reply_message_id(self.config.bridge.thread_replies);
        if let Err(e) = self
            .process_message(chat_id, reply_to, &binding, &text)
            .await
        {
            error!(error = %e, chat_id = %chat_id, "Failed to process message");
            // Send error to Telegram
            let error_msg = format!("❌ Error: {}", e);
            let _ = self
                .telegram
                .send_message(ChatId(chat_id), &error_msg, reply_to)
                .await;
        }

        Ok(())
    }

    /// Process a message by sending to gateway and streaming response back.
    async fn process_message(
        &self,
        chat_id: i64,
        reply_to: Option<MessageId>,
        binding: &ChatBinding,
        text: &str,
    ) -> Result<()> {
        let idempotency_key = Uuid::new_v4().to_string();

        // Send message to gateway
        let send_result = {
            let mut gateway = self.gateway.write().await;
            gateway
                .send_message(
                    binding.conversation_key.clone(),
                    text.to_string(),
                    idempotency_key,
                )
                .await
        };

        let response = match send_result {
            Ok(r) => r,
            Err(e) => {
                return Err(e);
            }
        };

        debug!(
            status = %response.status,
            message_id = %response.message_id,
            "Message sent to gateway"
        );

        // Stream events from gateway
        let stream_result = {
            let mut gateway = self.gateway.write().await;
            gateway
                .stream_events(binding.conversation_key.clone())
                .await
        };

        let mut stream = match stream_result {
            Ok(s) => s,
            Err(e) => {
                return Err(e);
            }
        };

        // Accumulate text chunks for final message
        let mut accumulated_text = String::new();
        let mut has_sent_message = false;

        while let Some(event_result) = stream.next().await {
            let event = match event_result {
                Ok(e) => e,
                Err(status) => {
                    error!(error = %status, "Stream error");
                    break;
                }
            };

            // Process the event payload
            match event.payload {
                Some(Payload::Text(chunk)) => {
                    accumulated_text.push_str(&chunk.content);
                    debug!(
                        chunk_len = chunk.content.len(),
                        total_len = accumulated_text.len(),
                        "Received text chunk"
                    );
                }
                Some(Payload::Thinking(chunk)) => {
                    debug!(
                        thinking_len = chunk.content.len(),
                        "Received thinking chunk (not relayed)"
                    );
                }
                Some(Payload::ToolUse(tool)) => {
                    debug!(tool_name = %tool.name, tool_id = %tool.id, "Tool use started");
                }
                Some(Payload::ToolResult(result)) => {
                    debug!(
                        tool_id = %result.id,
                        is_error = result.is_error,
                        "Tool result received"
                    );
                }
                Some(Payload::ToolState(state)) => {
                    debug!(tool_id = %state.id, state = ?state.state, "Tool state update");
                }
                Some(Payload::Usage(usage)) => {
                    debug!(
                        input = usage.input_tokens,
                        output = usage.output_tokens,
                        "Token usage update"
                    );
                }
                Some(Payload::Done(done)) => {
                    info!("Stream completed");
                    // Use full response if available, otherwise accumulated text
                    let final_text = done
                        .full_response
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| accumulated_text.clone());

                    if !final_text.is_empty() && !has_sent_message {
                        self.send_response(chat_id, reply_to, &final_text).await?;
                        has_sent_message = true;
                    }
                    break;
                }
                Some(Payload::Error(error)) => {
                    error!(message = %error.message, "Stream error event");
                    if !has_sent_message {
                        let error_msg = format!("❌ Error: {}", error.message);
                        self.send_response(chat_id, reply_to, &error_msg).await?;
                        has_sent_message = true;
                    }
                    break;
                }
                Some(Payload::Event(_)) => {
                    debug!("Received full event (history replay)");
                }
                None => {
                    debug!("Received empty payload");
                }
            }
        }

        // If we accumulated text but didn't send yet, send now
        if !accumulated_text.is_empty() && !has_sent_message {
            self.send_response(chat_id, reply_to, &accumulated_text)
                .await?;
        }

        Ok(())
    }

    /// Send a response to Telegram.
    async fn send_response(
        &self,
        chat_id: i64,
        reply_to: Option<MessageId>,
        text: &str,
    ) -> Result<()> {
        self.telegram
            .send_message(ChatId(chat_id), text, reply_to)
            .await?;
        debug!(
            chat_id = %chat_id,
            reply_to = ?reply_to,
            text_len = text.len(),
            "Sent response to Telegram"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_binding_clone() {
        let binding = ChatBinding {
            chat_id: 12345,
            conversation_key: "test-conversation".to_string(),
        };

        let cloned = binding.clone();
        assert_eq!(binding.chat_id, cloned.chat_id);
        assert_eq!(binding.conversation_key, cloned.conversation_key);
    }
}
