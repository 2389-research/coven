// ABOUTME: Matrix client wrapper using matrix-sdk.
// ABOUTME: Handles login, sync, message sending, and event handling.

use crate::config::MatrixConfig;
use crate::error::{BridgeError, Result};
use matrix_sdk::{
    config::SyncSettings,
    room::Room,
    ruma::{
        api::client::room::create_room::v3::Request as CreateRoomRequest,
        events::room::message::{
            MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
        },
        OwnedRoomId, OwnedUserId, UserId,
    },
    Client, RoomMemberships, RoomState,
};
use std::path::PathBuf;
use tracing::{debug, info, warn};

pub struct MatrixClient {
    client: Client,
    user_id: OwnedUserId,
}

impl MatrixClient {
    pub async fn login(config: &MatrixConfig) -> Result<Self> {
        info!(homeserver = %config.homeserver, username = %config.username, "Logging into Matrix");

        let state_dir = config.state_dir.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local")
                .join("share")
                .join("coven-matrix-bridge")
        });

        std::fs::create_dir_all(&state_dir)?;

        // Try to build client and login, handling stale crypto store
        match Self::try_login(config, &state_dir).await {
            Ok(client) => Ok(client),
            Err(e) => {
                let error_str = e.to_string();
                // Check for crypto store mismatch errors
                if error_str.contains("account in the store doesn't match")
                    || error_str.contains("crypto store")
                {
                    warn!(
                        "Crypto store mismatch detected, clearing state and retrying: {}",
                        error_str
                    );
                    // Clear the state directory and retry
                    if let Err(rm_err) = std::fs::remove_dir_all(&state_dir) {
                        warn!("Failed to remove state dir: {}", rm_err);
                    }
                    std::fs::create_dir_all(&state_dir)?;
                    Self::try_login(config, &state_dir).await
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn try_login(config: &MatrixConfig, state_dir: &PathBuf) -> Result<Self> {
        let client = Client::builder()
            .homeserver_url(&config.homeserver)
            .sqlite_store(state_dir, None)
            .build()
            .await?;

        client
            .matrix_auth()
            .login_username(&config.username, &config.password)
            .initial_device_display_name("coven-matrix-bridge")
            .await?;

        let user_id = client
            .user_id()
            .ok_or_else(|| {
                BridgeError::Matrix(Box::new(matrix_sdk::Error::UnknownError(
                    "No user ID after login".into(),
                )))
            })?
            .to_owned();

        info!(user_id = %user_id, "Matrix login successful");

        Ok(Self { client, user_id })
    }

    pub fn user_id(&self) -> &OwnedUserId {
        &self.user_id
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub async fn send_text(&self, room_id: &OwnedRoomId, text: &str) -> Result<()> {
        let room = self
            .client
            .get_room(room_id)
            .ok_or_else(|| BridgeError::Config(format!("Room not found: {}", room_id)))?;

        if room.state() == RoomState::Joined {
            let content = RoomMessageEventContent::text_plain(text);
            room.send(content).await?;
            debug!(room_id = %room_id, "Sent message to Matrix room");
        } else {
            warn!(room_id = %room_id, "Cannot send to non-joined room");
        }

        Ok(())
    }

    pub async fn send_html(&self, room_id: &OwnedRoomId, plain: &str, html: &str) -> Result<()> {
        let room = self
            .client
            .get_room(room_id)
            .ok_or_else(|| BridgeError::Config(format!("Room not found: {}", room_id)))?;

        if room.state() == RoomState::Joined {
            let content = RoomMessageEventContent::text_html(plain, html);
            room.send(content).await?;
            debug!(room_id = %room_id, "Sent HTML message to Matrix room");
        } else {
            warn!(room_id = %room_id, "Cannot send to non-joined room");
        }

        Ok(())
    }

    pub async fn set_typing(&self, room_id: &OwnedRoomId, typing: bool) -> Result<()> {
        let room = self
            .client
            .get_room(room_id)
            .ok_or_else(|| BridgeError::Config(format!("Room not found: {}", room_id)))?;

        if room.state() == RoomState::Joined {
            room.typing_notice(typing).await?;
        }

        Ok(())
    }

    pub async fn sync_once(&self) -> Result<()> {
        self.client.sync_once(SyncSettings::default()).await?;
        Ok(())
    }

    /// Create a new room and invite a user.
    /// Returns the new room ID.
    pub async fn create_room_with_user(
        &self,
        user_id: &UserId,
        room_name: &str,
    ) -> Result<OwnedRoomId> {
        info!(user_id = %user_id, room_name = %room_name, "Creating room for user");

        let mut request = CreateRoomRequest::new();
        request.name = Some(room_name.to_string());
        request.invite = vec![user_id.to_owned()];
        request.is_direct = true;

        let response = self.client.create_room(request).await?;
        let room_id = response.room_id().to_owned();

        info!(room_id = %room_id, user_id = %user_id, "Created room and invited user");
        Ok(room_id)
    }

    /// Check if a room is a DM (direct message) with just the bot and one other user.
    pub async fn is_dm_room(&self, room_id: &OwnedRoomId) -> bool {
        let Some(room) = self.client.get_room(room_id) else {
            return false;
        };

        // Check if it's marked as direct
        if !room.is_direct().await.unwrap_or(false) {
            return false;
        }

        // Check member count (should be 2: bot + user)
        let members = match room.members(RoomMemberships::ACTIVE).await {
            Ok(m) => m,
            Err(_) => return false,
        };

        members.len() == 2
    }

    /// Get the other user in a DM room.
    pub async fn get_dm_partner(&self, room_id: &OwnedRoomId) -> Option<OwnedUserId> {
        let room = self.client.get_room(room_id)?;
        let members = room.members(RoomMemberships::ACTIVE).await.ok()?;

        for member in members {
            if member.user_id() != self.user_id {
                return Some(member.user_id().to_owned());
            }
        }
        None
    }

    /// Get a room by ID.
    pub fn get_room(&self, room_id: &OwnedRoomId) -> Option<Room> {
        self.client.get_room(room_id)
    }
}

/// Extract text content from a Matrix message event
pub fn extract_text_content(event: &OriginalSyncRoomMessageEvent) -> Option<String> {
    match &event.content.msgtype {
        MessageType::Text(text) => Some(text.body.clone()),
        _ => None,
    }
}
