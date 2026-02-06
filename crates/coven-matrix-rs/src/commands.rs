// ABOUTME: Handles !coven commands in Matrix rooms for binding management.
// ABOUTME: Supports bind, unbind, status, and agents commands.

use crate::bridge::RoomBinding;
use crate::error::Result;
use crate::gateway::GatewayClient;
use matrix_sdk::ruma::OwnedRoomId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

pub enum Command {
    Bind(String), // !coven bind <agent-id>
    Unbind,       // !coven unbind
    Status,       // !coven status
    Agents,       // !coven agents
    Rooms,        // !coven rooms (DM only - list user's bound rooms)
    Help,         // !coven help
    Unknown(String),
}

impl Command {
    pub fn parse(input: &str) -> Option<Command> {
        let input = input.trim();

        // Check for !coven prefix
        let rest = input.strip_prefix("!coven")?.trim();

        if rest.is_empty() || rest == "help" {
            return Some(Command::Help);
        }

        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        match parts[0] {
            "bind" => match parts.get(1).map(|s| s.trim().to_string()) {
                Some(agent_id) if !agent_id.is_empty() => Some(Command::Bind(agent_id)),
                _ => Some(Command::Unknown(
                    "bind (requires agent-id, e.g., !coven bind ef2bbe1b-f0f)".to_string(),
                )),
            },
            "unbind" => Some(Command::Unbind),
            "status" => Some(Command::Status),
            "agents" => Some(Command::Agents),
            "rooms" => Some(Command::Rooms),
            "help" => Some(Command::Help),
            other => Some(Command::Unknown(other.to_string())),
        }
    }
}

/// Context required for executing commands that modify state.
pub struct CommandContext<'a> {
    pub gateway: &'a Arc<RwLock<GatewayClient>>,
    pub bindings: &'a Arc<RwLock<HashMap<OwnedRoomId, RoomBinding>>>,
    pub room_id: &'a OwnedRoomId,
    /// The Matrix user who sent the command (used to set binding ownership).
    pub sender: &'a str,
}

pub async fn execute_command(command: Command, ctx: CommandContext<'_>) -> Result<String> {
    match command {
        Command::Bind(agent_id) => {
            let binding = RoomBinding {
                room_id: ctx.room_id.clone(),
                conversation_key: agent_id.clone(),
                owner: Some(ctx.sender.to_string()),
            };
            ctx.bindings
                .write()
                .await
                .insert(ctx.room_id.clone(), binding);
            info!(room_id = %ctx.room_id, agent_id = %agent_id, "Room bound to agent via command");
            Ok(format!(
                "🔗 Bound this room to agent: {}\nUse `!coven status` to verify.",
                agent_id
            ))
        }
        Command::Unbind => {
            let removed = ctx.bindings.write().await.remove(ctx.room_id);
            info!(room_id = %ctx.room_id, "Room unbound via command");
            match removed {
                Some(binding) => Ok(format!(
                    "Unbound this room from agent: {}",
                    binding.conversation_key
                )),
                None => Ok("This room was not bound to any agent.".to_string()),
            }
        }
        Command::Status => {
            let bindings = ctx.bindings.read().await;
            match bindings.get(ctx.room_id) {
                Some(binding) => Ok(format!(
                    "📊 Status: Bound to agent `{}`\nRoom ID: {}",
                    binding.conversation_key, ctx.room_id
                )),
                None => Ok(format!(
                    "📊 Status: No agent bound to this room.\nRoom ID: {}\nUse `!coven bind <agent-id>` to bind an agent.",
                    ctx.room_id
                )),
            }
        }
        Command::Agents => {
            let mut gateway = ctx.gateway.write().await;
            let agents = gateway.list_agents().await?;

            if agents.is_empty() {
                Ok("📭 No agents currently online.".to_string())
            } else {
                let mut response = String::from("🤖 Online agents:\n\n");
                for agent in &agents {
                    let dir = agent
                        .metadata
                        .as_ref()
                        .map(|m| m.working_directory.as_str())
                        .unwrap_or("unknown");

                    response.push_str(&format!("• `{}`\n", agent.id));
                    response.push_str(&format!("  📁 {}\n", dir));
                    response.push_str(&format!("  → `!coven bind {}`\n\n", agent.id));
                }
                Ok(response)
            }
        }
        Command::Rooms => {
            // This is handled specially in bridge.rs for DMs
            Ok("Use `!coven rooms` in a DM with me to see your bound rooms.".to_string())
        }
        Command::Help => Ok(r#"🌙 Coven Bridge Commands:

In a DM with me:
  !coven bind <id>  - Create a room bound to an agent
  !coven agents     - List available agents
  !coven rooms      - List your bound rooms

In a bound room:
  !coven status     - Show current binding
  !coven unbind     - Unbind this room
  !coven help       - Show this help"#
            .to_string()),
        Command::Unknown(cmd) => Ok(format!(
            "Unknown command: {}\nUse `!coven help` for available commands.",
            cmd
        )),
    }
}
