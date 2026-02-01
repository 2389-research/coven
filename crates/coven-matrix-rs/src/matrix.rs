// ABOUTME: Matrix client wrapper using matrix-sdk.
// ABOUTME: Handles login, sync, message sending, and event handling.

use crate::config::MatrixConfig;
use crate::error::{BridgeError, Result};
use matrix_sdk::{
    config::SyncSettings,
    ruma::{
        events::room::message::{
            MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
        },
        OwnedRoomId, OwnedUserId,
    },
    Client, RoomState,
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
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("coven-matrix-bridge")
        });

        std::fs::create_dir_all(&state_dir)?;

        let client = Client::builder()
            .homeserver_url(&config.homeserver)
            .sqlite_store(&state_dir, None)
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
                BridgeError::Matrix(matrix_sdk::Error::UnknownError(
                    "No user ID after login".into(),
                ))
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
}

/// Extract text content from a Matrix message event
pub fn extract_text_content(event: &OriginalSyncRoomMessageEvent) -> Option<String> {
    match &event.content.msgtype {
        MessageType::Text(text) => Some(text.body.clone()),
        _ => None,
    }
}
