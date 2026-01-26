// ABOUTME: State persistence and config management.
// ABOUTME: Handles saving/loading conversations and settings.

pub mod config;

use crate::error::{AppError, Result};
use config::Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Current state file version for future migrations
const STATE_VERSION: u32 = 1;

/// Application state persisted to disk between sessions.
/// Stores conversation history, last accessed agent, and state version.
#[derive(Serialize, Deserialize, Default)]
pub struct AppState {
    /// Version number for state file migrations
    pub version: u32,
    /// Last accessed agent ID to restore on startup
    pub last_agent_id: Option<String>,
    /// Conversation history keyed by agent ID
    pub conversations: HashMap<String, ConversationState>,
}

/// Persisted conversation state for a single agent.
#[derive(Serialize, Deserialize, Clone)]
pub struct ConversationState {
    /// The agent ID this conversation belongs to
    pub agent_id: String,
    /// Messages in the conversation
    pub messages: Vec<PersistedMessage>,
    /// Unix timestamp (millis) of last access
    pub last_accessed: i64,
}

/// A single persisted message in a conversation.
#[derive(Serialize, Deserialize, Clone)]
pub struct PersistedMessage {
    /// Message content
    pub content: String,
    /// Whether this message was sent by the user
    pub is_user: bool,
    /// Unix timestamp (millis) when message was created
    pub timestamp: i64,
}

impl PersistedMessage {
    /// Create a new persisted message
    #[allow(dead_code)]
    pub fn new(content: String, is_user: bool, timestamp: i64) -> Self {
        Self {
            content,
            is_user,
            timestamp,
        }
    }

    /// Convert from fold_client::Message
    pub fn from_client_message(msg: &fold_client::Message) -> Self {
        Self {
            content: msg.content.clone(),
            is_user: msg.is_user,
            timestamp: msg.timestamp,
        }
    }

    /// Convert to fold_client::Message
    pub fn to_client_message(&self) -> fold_client::Message {
        fold_client::Message {
            id: format!(
                "{}-{}",
                if self.is_user { "user" } else { "agent" },
                self.timestamp
            ),
            sender: if self.is_user {
                "You".to_string()
            } else {
                "Agent".to_string()
            },
            content: self.content.clone(),
            timestamp: self.timestamp,
            is_user: self.is_user,
        }
    }
}

impl ConversationState {
    /// Create a new conversation state for an agent
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            messages: Vec::new(),
            last_accessed: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Update the last accessed timestamp to now
    pub fn touch(&mut self) {
        self.last_accessed = chrono::Utc::now().timestamp_millis();
    }
}

impl AppState {
    /// Create a new empty app state
    pub fn new() -> Self {
        Self {
            version: STATE_VERSION,
            last_agent_id: None,
            conversations: HashMap::new(),
        }
    }

    /// Load state from disk, returning default if file doesn't exist or is invalid
    pub fn load() -> Result<Self> {
        let path = Config::state_path()?;

        if !path.exists() {
            tracing::debug!("State file does not exist, using defaults");
            return Ok(Self::new());
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read state file: {}, using defaults", e);
                return Ok(Self::new());
            }
        };

        match serde_json::from_str::<Self>(&content) {
            Ok(mut state) => {
                // Handle version migration if needed
                if state.version < STATE_VERSION {
                    tracing::info!(
                        "Migrating state from v{} to v{}",
                        state.version,
                        STATE_VERSION
                    );
                    state = Self::migrate(state);
                }
                Ok(state)
            }
            Err(e) => {
                tracing::warn!("Failed to parse state file: {}, using defaults", e);
                Ok(Self::new())
            }
        }
    }

    /// Save state to disk
    pub fn save(&self) -> Result<()> {
        let path = Config::state_path()?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Config(format!("Failed to create state dir: {}", e)))?;
        }

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Config(format!("Failed to serialize state: {}", e)))?;

        std::fs::write(&path, content)
            .map_err(|e| AppError::Config(format!("Failed to write state: {}", e)))?;

        tracing::debug!("Saved state to {}", path.display());
        Ok(())
    }

    /// Migrate state from an older version to the current version
    fn migrate(old_state: Self) -> Self {
        // Currently we only have v1, so no migration needed yet
        // Future versions would handle migration here
        Self {
            version: STATE_VERSION,
            last_agent_id: old_state.last_agent_id,
            conversations: old_state.conversations,
        }
    }

    /// Get conversation state for an agent
    pub fn get_conversation(&self, agent_id: &str) -> Option<&ConversationState> {
        self.conversations.get(agent_id)
    }

    /// Get mutable conversation state for an agent
    #[allow(dead_code)]
    pub fn get_conversation_mut(&mut self, agent_id: &str) -> Option<&mut ConversationState> {
        self.conversations.get_mut(agent_id)
    }

    /// Update or create conversation state for an agent
    pub fn update_conversation(&mut self, agent_id: &str, messages: Vec<PersistedMessage>) {
        let conversation = self
            .conversations
            .entry(agent_id.to_string())
            .or_insert_with(|| ConversationState::new(agent_id.to_string()));

        conversation.messages = messages;
        conversation.touch();
        self.last_agent_id = Some(agent_id.to_string());
    }

    /// Set the last accessed agent ID
    pub fn set_last_agent(&mut self, agent_id: &str) {
        self.last_agent_id = Some(agent_id.to_string());
        if let Some(conversation) = self.conversations.get_mut(agent_id) {
            conversation.touch();
        }
    }

    /// Get messages for an agent as client messages
    pub fn get_messages(&self, agent_id: &str) -> Vec<fold_client::Message> {
        self.get_conversation(agent_id)
            .map(|conv| {
                conv.messages
                    .iter()
                    .map(|m| m.to_client_message())
                    .collect()
            })
            .unwrap_or_default()
    }
}
