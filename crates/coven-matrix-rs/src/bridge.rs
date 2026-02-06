// ABOUTME: Core bridge logic that ties Matrix and Gateway clients together.
// ABOUTME: Handles message routing, room bindings, command processing, and event streaming.

use crate::commands::{execute_command, Command, CommandContext};
use crate::config::Config;
use crate::error::Result;
use crate::gateway::GatewayClient;
use crate::matrix::{extract_text_content, MatrixClient};

use coven_proto::client_stream_event::Payload;
use futures::StreamExt;
use matrix_sdk::{
    config::SyncSettings, ruma::events::room::message::OriginalSyncRoomMessageEvent,
    ruma::OwnedRoomId, RoomMemberships, RoomState,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Room binding information mapping a Matrix room to a gateway conversation.
#[derive(Clone, Debug)]
pub struct RoomBinding {
    pub room_id: OwnedRoomId,
    pub conversation_key: String,
    /// The Matrix user who created this binding (used to scope visibility).
    pub owner: Option<String>,
}

/// The Bridge ties together Matrix and Gateway clients to route messages.
pub struct Bridge {
    config: Config,
    matrix: MatrixClient,
    gateway: Arc<RwLock<GatewayClient>>,
    bindings: Arc<RwLock<HashMap<OwnedRoomId, RoomBinding>>>,
}

impl Bridge {
    /// Create a new Bridge with the given configuration.
    /// Establishes connections to both Matrix and the Gateway.
    pub async fn new(config: Config) -> Result<Self> {
        info!("Initializing bridge");

        // Connect to Matrix
        let matrix = MatrixClient::login(&config.matrix).await?;

        // Connect to Gateway
        let gateway =
            GatewayClient::connect(&config.gateway.endpoint_uri(), config.gateway.token.clone())
                .await?;

        // Do an initial sync to populate room list
        matrix.sync_once().await?;

        Ok(Self {
            config,
            matrix,
            gateway: Arc::new(RwLock::new(gateway)),
            bindings: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Bind a Matrix room to a gateway conversation.
    pub async fn bind_room(
        &self,
        room_id: OwnedRoomId,
        conversation_key: String,
        owner: Option<String>,
    ) {
        let binding = RoomBinding {
            room_id: room_id.clone(),
            conversation_key,
            owner,
        };

        info!(
            room_id = %room_id,
            conversation_key = %binding.conversation_key,
            "Binding room to conversation"
        );

        self.bindings.write().await.insert(room_id, binding);
    }

    /// Unbind a Matrix room from any gateway conversation.
    pub async fn unbind_room(&self, room_id: &OwnedRoomId) -> Option<RoomBinding> {
        let binding = self.bindings.write().await.remove(room_id);
        if let Some(ref b) = binding {
            info!(
                room_id = %room_id,
                conversation_key = %b.conversation_key,
                "Unbound room from conversation"
            );
        }
        binding
    }

    /// Get the binding for a room, if any.
    pub async fn get_binding(&self, room_id: &OwnedRoomId) -> Option<RoomBinding> {
        self.bindings.read().await.get(room_id).cloned()
    }

    /// Run the bridge, setting up event handlers and starting the sync loop.
    pub async fn run(&self) -> Result<()> {
        info!("Starting bridge sync loop");

        let client = self.matrix.client().clone();
        let user_id = self.matrix.user_id().clone();
        let bindings = Arc::clone(&self.bindings);
        let gateway = Arc::clone(&self.gateway);
        let config = self.config.clone();

        // Set up the event handler for room messages
        client.add_event_handler(
            move |event: OriginalSyncRoomMessageEvent, room: matrix_sdk::Room| {
                let bindings = Arc::clone(&bindings);
                let gateway = Arc::clone(&gateway);
                let config = config.clone();
                let user_id = user_id.clone();

                async move {
                    // Only process messages from joined rooms
                    if room.state() != RoomState::Joined {
                        return;
                    }

                    let room_id = room.room_id().to_owned();

                    // Ignore messages from ourselves
                    if event.sender == user_id {
                        return;
                    }

                    // Extract text content
                    let Some(text) = extract_text_content(&event) else {
                        debug!(room_id = %room_id, "Non-text message, ignoring");
                        return;
                    };

                    // Check if this is a DM (direct room with just bot + user).
                    // Computed early so the allowlist check can bypass for DM commands.
                    let is_dm = room.is_direct().await.unwrap_or(false) && {
                        let members = room.members(RoomMemberships::ACTIVE).await;
                        members.map(|m| m.len() == 2).unwrap_or(false)
                    };

                    // Bypass room allowlist only for !coven commands sent via DM.
                    // Non-DM rooms still require allowlist membership even for commands.
                    let is_dm_command = is_dm && Command::parse(&text).is_some();

                    if !is_dm_command && !config.is_room_allowed(room_id.as_str()) {
                        debug!(room_id = %room_id, "Message from non-allowed room, ignoring");
                        return;
                    }

                    // Check if sender is allowed
                    if !config.is_sender_allowed(event.sender.as_str()) {
                        debug!(sender = %event.sender, "Message from non-allowed sender, ignoring");
                        return;
                    }

                    // Check for !coven commands FIRST, before binding check
                    // Commands work regardless of room binding status
                    if let Some(command) = Command::parse(&text) {
                        info!(room_id = %room_id, sender = %event.sender, "Processing !coven command");

                        // Handle DM-specific bind command: create room + invite + bind
                        if is_dm {
                            if let Command::Bind(agent_id) = &command {
                                info!(agent_id = %agent_id, sender = %event.sender, "DM bind: checking agent exists");

                                // First, verify the agent exists in the gateway
                                let agent_exists = {
                                    let mut gw = gateway.write().await;
                                    match gw.list_agents().await {
                                        Ok(agents) => agents.iter().any(|a| a.id == *agent_id),
                                        Err(e) => {
                                            error!(error = %e, "Failed to list agents");
                                            let error_response = format!("❌ Failed to connect to gateway: {}", e);
                                            let _ = room.send(matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(&error_response)).await;
                                            return;
                                        }
                                    }
                                };

                                if !agent_exists {
                                    warn!(agent_id = %agent_id, "Agent not found in gateway");
                                    let error_response = format!(
                                        "❌ Agent `{}` not found.\n\nUse `!coven agents` to see available agents.",
                                        agent_id
                                    );
                                    let _ = room.send(matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(&error_response)).await;
                                    return;
                                }

                                info!(agent_id = %agent_id, sender = %event.sender, "DM bind: creating room for user");

                                // Get the Matrix client to create the room
                                let room_name = agent_id.clone();

                                // Create room request
                                let mut request = matrix_sdk::ruma::api::client::room::create_room::v3::Request::new();
                                request.name = Some(room_name.clone());
                                request.invite = vec![event.sender.clone()];
                                request.is_direct = false; // Agent rooms are not DMs

                                // Get the underlying client from the room
                                let client = room.client();
                                match client.create_room(request).await {
                                    Ok(response) => {
                                        let new_room_id = response.room_id().to_owned();
                                        info!(new_room_id = %new_room_id, agent_id = %agent_id, "Created agent room");

                                        // Bind the new room
                                        let binding = crate::bridge::RoomBinding {
                                            room_id: new_room_id.clone(),
                                            conversation_key: agent_id.clone(),
                                            owner: Some(event.sender.to_string()),
                                        };
                                        bindings.write().await.insert(new_room_id.clone(), binding);

                                        // Send success message in DM
                                        let dm_response = format!(
                                            "✨ Created room **{}** and bound to agent `{}`\n\nAccept the invite to start chatting!",
                                            room_name, agent_id
                                        );
                                        if let Err(e) = room
                                            .send(matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(&dm_response))
                                            .await
                                        {
                                            error!(error = %e, "Failed to send DM response");
                                        }

                                        // Send welcome in the new room (after a short delay for room setup)
                                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                        if let Some(new_room) = client.get_room(&new_room_id) {
                                            let welcome = format!(
                                                "🔗 Connected to agent `{}`\n\nStart chatting! Use `!coven status` to check the binding or `!coven unbind` to disconnect.",
                                                agent_id
                                            );
                                            if let Err(e) = new_room
                                                .send(matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(&welcome))
                                                .await
                                            {
                                                warn!(error = %e, "Failed to send welcome to new room");
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!(error = %e, "Failed to create room for DM bind");
                                        let error_response = format!("❌ Failed to create room: {}", e);
                                        let _ = room
                                            .send(matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(&error_response))
                                            .await;
                                    }
                                }
                                return;
                            }

                            // Handle !coven rooms in DM - list only this user's bound rooms
                            if let Command::Rooms = &command {
                                let bindings_read = bindings.read().await;
                                let sender_str = event.sender.to_string();
                                let user_bindings: Vec<_> = bindings_read.values()
                                    .filter(|b| b.owner.as_deref() == Some(sender_str.as_str()))
                                    .collect();
                                if user_bindings.is_empty() {
                                    let response = "You have no bound rooms.\n\nUse `!coven bind <agent-id>` to create one.";
                                    let _ = room.send(matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(response)).await;
                                } else {
                                    let mut response = String::from("📋 Your bound rooms:\n\n");
                                    for binding in &user_bindings {
                                        response.push_str(&format!("• `{}` → {}\n", binding.conversation_key, binding.room_id));
                                    }
                                    let _ = room.send(matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(&response)).await;
                                }
                                return;
                            }
                        }

                        // Regular command execution for non-DM rooms or other commands
                        let ctx = CommandContext {
                            gateway: &gateway,
                            bindings: &bindings,
                            room_id: &room_id,
                            sender: event.sender.as_str(),
                        };
                        let response = match execute_command(command, ctx).await {
                            Ok(resp) => resp,
                            Err(e) => format!("Command error: {}", e),
                        };
                        // Send command response to room
                        if let Err(e) = room
                            .send(
                                matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(&response),
                            )
                            .await
                        {
                            error!(error = %e, "Failed to send command response to room");
                        }
                        return;
                    }

                    // Check for command prefix if configured (for regular messages to agent)
                    let text = if let Some(ref prefix) = config.bridge.command_prefix {
                        if let Some(stripped) = text.strip_prefix(prefix) {
                            stripped.trim().to_string()
                        } else {
                            debug!(room_id = %room_id, "Message doesn't have command prefix, ignoring");
                            return;
                        }
                    } else {
                        text
                    };

                    if text.is_empty() {
                        return;
                    }

                    // Check if room is bound (required for forwarding to agent)
                    let binding = {
                        let bindings_read = bindings.read().await;
                        bindings_read.get(&room_id).cloned()
                    };

                    let Some(binding) = binding else {
                        debug!(room_id = %room_id, "Message from unbound room, ignoring");
                        return;
                    };

                    info!(
                        room_id = %room_id,
                        sender = %event.sender,
                        conversation_key = %binding.conversation_key,
                        "Forwarding message to gateway"
                    );
                    debug!(
                        room_id = %room_id,
                        message_preview = %text.chars().take(50).collect::<String>(),
                        "Message content preview"
                    );

                    // Process the message
                    if let Err(e) = process_message(
                        &room,
                        &binding,
                        &text,
                        &gateway,
                        config.bridge.typing_indicator,
                    )
                    .await
                    {
                        error!(error = %e, room_id = %room_id, "Failed to process message");

                        // Create a user-friendly error message
                        let error_str = e.to_string().to_lowercase();
                        let error_msg = if error_str.contains("agent not found") || error_str.contains("not found") {
                            format!(
                                "⚠️ Agent `{}` is offline or disconnected.\n\nThe agent needs to be running to receive messages. Use `!coven agents` to see online agents.",
                                binding.conversation_key
                            )
                        } else if error_str.contains("connection") || error_str.contains("unavailable") {
                            "⚠️ Unable to reach the gateway. Please try again later.".to_string()
                        } else {
                            format!("❌ Error: {}", e)
                        };

                        if let Err(send_err) = room
                            .send(
                                matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(&error_msg),
                            )
                            .await
                        {
                            error!(error = %send_err, "Failed to send error message to room");
                        }
                    }
                }
            },
        );

        // Start the sync loop
        let settings = SyncSettings::default().timeout(std::time::Duration::from_secs(30));
        client.sync(settings).await?;

        Ok(())
    }

    /// Get a reference to the Matrix client.
    pub fn matrix_client(&self) -> &MatrixClient {
        &self.matrix
    }

    /// Get a reference to the Gateway client (locked).
    pub fn gateway_client(&self) -> &Arc<RwLock<GatewayClient>> {
        &self.gateway
    }
}

/// Process a message from Matrix by sending to gateway and streaming response back.
async fn process_message(
    room: &matrix_sdk::Room,
    binding: &RoomBinding,
    text: &str,
    gateway: &Arc<RwLock<GatewayClient>>,
    typing_indicator: bool,
) -> Result<()> {
    let idempotency_key = Uuid::new_v4().to_string();

    // Set typing indicator
    if typing_indicator && room.state() == RoomState::Joined {
        if let Err(e) = room.typing_notice(true).await {
            warn!(error = %e, "Failed to set typing indicator");
        }
    }

    // Send message to gateway
    let send_result = {
        let mut gateway = gateway.write().await;
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
            // Clear typing indicator on error
            if typing_indicator && room.state() == RoomState::Joined {
                let _ = room.typing_notice(false).await;
            }
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
        let mut gateway = gateway.write().await;
        gateway
            .stream_events(binding.conversation_key.clone())
            .await
    };

    let mut stream = match stream_result {
        Ok(s) => s,
        Err(e) => {
            // Clear typing indicator on error
            if typing_indicator && room.state() == RoomState::Joined {
                let _ = room.typing_notice(false).await;
            }
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
                // Use the full response if available, otherwise use accumulated
                let final_text = done
                    .full_response
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| accumulated_text.clone());

                if !final_text.is_empty() && !has_sent_message {
                    send_response_to_room(room, &final_text).await?;
                    has_sent_message = true;
                }
                break;
            }
            Some(Payload::Error(error)) => {
                error!(message = %error.message, "Stream error event");
                if !has_sent_message {
                    let error_msg = format!("Error: {}", error.message);
                    send_response_to_room(room, &error_msg).await?;
                    has_sent_message = true;
                }
                break;
            }
            Some(Payload::Event(_)) => {
                // Full event replay, typically for history - ignore in streaming context
                debug!("Received full event (history replay)");
            }
            Some(Payload::ToolApproval(approval)) => {
                // Tool approval requests not supported in Matrix bridge - auto-deny
                debug!(tool_name = %approval.tool_name, "Tool approval request (auto-denied in Matrix)");
                let mut gw = gateway.write().await;
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

    // Clear typing indicator
    if typing_indicator && room.state() == RoomState::Joined {
        let _ = room.typing_notice(false).await;
    }

    // If we accumulated text but didn't send yet (no Done event), send now
    if !accumulated_text.is_empty() && !has_sent_message {
        send_response_to_room(room, &accumulated_text).await?;
    }

    Ok(())
}

/// Send a response back to the Matrix room.
async fn send_response_to_room(room: &matrix_sdk::Room, text: &str) -> Result<()> {
    if room.state() != RoomState::Joined {
        warn!(room_id = %room.room_id(), "Cannot send to non-joined room");
        return Ok(());
    }

    let content =
        matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(text);
    room.send(content).await?;

    debug!(room_id = %room.room_id(), text_len = text.len(), "Sent response to room");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_room_binding_clone() {
        let binding = RoomBinding {
            room_id: OwnedRoomId::try_from("!test:example.org").unwrap(),
            conversation_key: "test-conversation".to_string(),
            owner: Some("@user:example.org".to_string()),
        };

        let cloned = binding.clone();
        assert_eq!(binding.room_id, cloned.room_id);
        assert_eq!(binding.conversation_key, cloned.conversation_key);
        assert_eq!(binding.owner, cloned.owner);
    }
}
