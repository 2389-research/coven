// ABOUTME: Core bridge logic connecting Slack events to coven-gateway.
// ABOUTME: Handles message routing, bindings, command processing, and response streaming.

use crate::commands::{execute_command, Command, CommandContext};
use crate::config::Config;
use crate::error::Result;
use crate::gateway::GatewayClient;
use crate::slack::{CovenSlackClient, SlackMessageInfo};

use coven_proto::client_stream_event::Payload;
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use uuid::Uuid;

/// Channel binding information mapping a Slack channel to a gateway conversation.
#[derive(Clone, Debug)]
pub struct ChannelBinding {
    pub channel_id: String,
    pub conversation_key: String,
}

/// The Bridge ties together Slack and Gateway clients to route messages.
pub struct Bridge {
    config: Config,
    slack: CovenSlackClient,
    gateway: Arc<RwLock<GatewayClient>>,
    bindings: Arc<RwLock<HashMap<String, ChannelBinding>>>,
}

impl Bridge {
    /// Create a new Bridge with the given configuration.
    /// Establishes connections to both Slack and the Gateway.
    pub async fn new(config: Config) -> Result<Self> {
        info!("Initializing Slack bridge");

        // Connect to Slack
        let slack = CovenSlackClient::new(&config.slack).await?;

        // Connect to Gateway
        let gateway =
            GatewayClient::connect(&config.gateway.url, config.gateway.token.clone()).await?;

        Ok(Self {
            config,
            slack,
            gateway: Arc::new(RwLock::new(gateway)),
            bindings: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Bind a Slack channel to a gateway conversation.
    pub async fn bind_channel(&self, channel_id: String, conversation_key: String) {
        let binding = ChannelBinding {
            channel_id: channel_id.clone(),
            conversation_key,
        };

        info!(
            channel_id = %channel_id,
            conversation_key = %binding.conversation_key,
            "Binding channel to conversation"
        );

        self.bindings.write().await.insert(channel_id, binding);
    }

    /// Unbind a Slack channel from any gateway conversation.
    pub async fn unbind_channel(&self, channel_id: &str) -> Option<ChannelBinding> {
        let binding = self.bindings.write().await.remove(channel_id);
        if let Some(ref b) = binding {
            info!(
                channel_id = %channel_id,
                conversation_key = %b.conversation_key,
                "Unbound channel from conversation"
            );
        }
        binding
    }

    /// Get the binding for a channel, if any.
    pub async fn get_binding(&self, channel_id: &str) -> Option<ChannelBinding> {
        self.bindings.read().await.get(channel_id).cloned()
    }

    /// Get a reference to the Slack client.
    pub fn slack_client(&self) -> &CovenSlackClient {
        &self.slack
    }

    /// Get a reference to the Gateway client (locked).
    pub fn gateway_client(&self) -> &Arc<RwLock<GatewayClient>> {
        &self.gateway
    }

    /// Get a reference to the bindings map.
    pub fn bindings(&self) -> &Arc<RwLock<HashMap<String, ChannelBinding>>> {
        &self.bindings
    }

    /// Get a reference to the config.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Handle an incoming Slack message event.
    pub async fn handle_message(&self, msg_info: SlackMessageInfo) -> Result<()> {
        let channel_id = &msg_info.channel_id;

        // Check if channel is allowed
        if !self.config.is_channel_allowed(channel_id) {
            debug!(channel_id = %channel_id, "Message from non-allowed channel, ignoring");
            return Ok(());
        }

        // Ignore messages from the bot itself
        if msg_info.user_id == self.slack.bot_user_id().to_string() {
            return Ok(());
        }

        // Check for /coven commands first (commands work regardless of binding)
        if let Some(command) = Command::from_message(&msg_info.text) {
            info!(
                channel_id = %channel_id,
                user_id = %msg_info.user_id,
                "Processing /coven command"
            );

            let ctx = CommandContext {
                gateway: &self.gateway,
                bindings: &self.bindings,
                channel_id,
            };

            let response = match execute_command(command, ctx).await {
                Ok(resp) => resp,
                Err(e) => format!(":x: Command error: {}", e),
            };

            // Reply in thread if original was in thread, otherwise start new thread
            let thread_ts = msg_info.reply_thread_ts(self.config.bridge.thread_replies);
            self.slack
                .post_message(channel_id, &response, thread_ts.as_deref())
                .await?;

            return Ok(());
        }

        // Check response conditions
        if !msg_info.context.should_respond(
            self.config.bridge.response_mode,
            msg_info.is_mention,
        ) {
            debug!(
                channel_id = %channel_id,
                "Message doesn't require response (response_mode={:?}, is_mention={})",
                self.config.bridge.response_mode,
                msg_info.is_mention
            );
            return Ok(());
        }

        // Check for binding (required for forwarding to agent)
        let binding = match self.get_binding(channel_id).await {
            Some(b) => b,
            None => {
                debug!(channel_id = %channel_id, "Message from unbound channel, ignoring");
                return Ok(());
            }
        };

        // Strip bot mention from text for cleaner processing
        let text = msg_info.text_without_mention(self.slack.bot_user_id());
        if text.is_empty() {
            return Ok(());
        }

        info!(
            channel_id = %channel_id,
            user_id = %msg_info.user_id,
            conversation_key = %binding.conversation_key,
            "Processing message"
        );

        // Process the message
        let thread_ts = msg_info.reply_thread_ts(self.config.bridge.thread_replies);
        if let Err(e) = self
            .process_message(
                channel_id,
                thread_ts.as_deref(),
                &binding,
                &text,
            )
            .await
        {
            error!(error = %e, channel_id = %channel_id, "Failed to process message");
            // Send error to Slack
            let error_msg = format!(":x: Error: {}", e);
            let _ = self
                .slack
                .post_message(channel_id, &error_msg, thread_ts.as_deref())
                .await;
        }

        Ok(())
    }

    /// Process a message by sending to gateway and streaming response back.
    async fn process_message(
        &self,
        channel_id: &str,
        thread_ts: Option<&str>,
        binding: &ChannelBinding,
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
                        self.send_response(channel_id, thread_ts, &final_text)
                            .await?;
                        has_sent_message = true;
                    }
                    break;
                }
                Some(Payload::Error(error)) => {
                    error!(message = %error.message, "Stream error event");
                    if !has_sent_message {
                        let error_msg = format!(":x: Error: {}", error.message);
                        self.send_response(channel_id, thread_ts, &error_msg)
                            .await?;
                        has_sent_message = true;
                    }
                    break;
                }
                Some(Payload::Event(_)) => {
                    debug!("Received full event (history replay)");
                }
                Some(Payload::ToolApproval(approval)) => {
                    // Tool approval requests not supported in Slack bridge - auto-deny
                    debug!(tool_name = %approval.tool_name, "Tool approval request (auto-denied in Slack)");
                    let mut gw = self.gateway.write().await;
                    if let Err(e) = gw
                        .approve_tool(approval.agent_id, approval.tool_id, false, false)
                        .await
                    {
                        error!(error = %e, "Failed to send tool denial");
                    }
                }
                None => {
                    debug!("Received empty payload");
                }
            }
        }

        // If we accumulated text but didn't send yet, send now
        if !accumulated_text.is_empty() && !has_sent_message {
            self.send_response(channel_id, thread_ts, &accumulated_text)
                .await?;
        }

        Ok(())
    }

    /// Send a response to Slack.
    async fn send_response(
        &self,
        channel_id: &str,
        thread_ts: Option<&str>,
        text: &str,
    ) -> Result<()> {
        self.slack.post_message(channel_id, text, thread_ts).await?;
        debug!(
            channel_id = %channel_id,
            thread_ts = ?thread_ts,
            text_len = text.len(),
            "Sent response to Slack"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_binding_clone() {
        let binding = ChannelBinding {
            channel_id: "C123".to_string(),
            conversation_key: "test-conversation".to_string(),
        };

        let cloned = binding.clone();
        assert_eq!(binding.channel_id, cloned.channel_id);
        assert_eq!(binding.conversation_key, cloned.conversation_key);
    }
}
