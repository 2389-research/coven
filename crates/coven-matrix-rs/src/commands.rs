// ABOUTME: Handles /coven commands in Matrix rooms for binding management.
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
    Bind(String), // /coven bind <agent-id>
    Unbind,       // /coven unbind
    Status,       // /coven status
    Agents,       // /coven agents
    Help,         // /coven help
    Unknown(String),
}

impl Command {
    pub fn parse(input: &str) -> Option<Command> {
        let input = input.trim();

        // Check for /coven prefix
        let rest = input.strip_prefix("/coven")?.trim();

        if rest.is_empty() || rest == "help" {
            return Some(Command::Help);
        }

        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        match parts[0] {
            "bind" => match parts.get(1).map(|s| s.trim().to_string()) {
                Some(agent_id) if !agent_id.is_empty() => Some(Command::Bind(agent_id)),
                _ => Some(Command::Unknown(
                    "bind (requires agent-id, e.g., /coven bind agent-123)".to_string(),
                )),
            },
            "unbind" => Some(Command::Unbind),
            "status" => Some(Command::Status),
            "agents" => Some(Command::Agents),
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
}

pub async fn execute_command(command: Command, ctx: CommandContext<'_>) -> Result<String> {
    match command {
        Command::Bind(agent_id) => {
            let binding = RoomBinding {
                room_id: ctx.room_id.clone(),
                conversation_key: agent_id.clone(),
            };
            ctx.bindings
                .write()
                .await
                .insert(ctx.room_id.clone(), binding);
            info!(room_id = %ctx.room_id, agent_id = %agent_id, "Room bound to agent via command");
            Ok(format!(
                "Bound this room to agent: {}\nUse `/coven status` to verify.",
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
                    "Status: Bound to agent `{}`\nRoom ID: {}",
                    binding.conversation_key, ctx.room_id
                )),
                None => Ok(format!(
                    "Status: No agent bound to this room.\nRoom ID: {}\nUse `/coven bind <agent-id>` to bind an agent.",
                    ctx.room_id
                )),
            }
        }
        Command::Agents => {
            let mut gateway = ctx.gateway.write().await;
            let agents = gateway.list_agents().await?;

            if agents.is_empty() {
                Ok("No agents currently online.".to_string())
            } else {
                let mut response = String::from("Online agents:\n");
                for agent in agents {
                    response.push_str(&format!(
                        "- {} ({})\n",
                        agent.id,
                        agent
                            .metadata
                            .as_ref()
                            .map(|m| m.working_directory.as_str())
                            .unwrap_or("unknown")
                    ));
                }
                Ok(response)
            }
        }
        Command::Help => Ok(r#"Coven Bridge Commands:
- /coven bind <agent-id> - Bind this room to an agent
- /coven unbind - Unbind this room from current agent
- /coven status - Show current binding status
- /coven agents - List available agents
- /coven help - Show this help message"#
            .to_string()),
        Command::Unknown(cmd) => Ok(format!(
            "Unknown command: {}\nUse `/coven help` for available commands.",
            cmd
        )),
    }
}
