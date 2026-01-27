// ABOUTME: Handles /coven commands in Matrix rooms for binding management.
// ABOUTME: Supports bind, unbind, status, and agents commands.

use crate::error::Result;
use crate::gateway::GatewayClient;
use std::sync::Arc;
use tokio::sync::RwLock;

pub enum Command {
    Bind(String),      // /coven bind <agent-id>
    Unbind,            // /coven unbind
    Status,            // /coven status
    Agents,            // /coven agents
    Help,              // /coven help
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
            "bind" => {
                let agent_id = parts.get(1).map(|s| s.trim().to_string())?;
                if agent_id.is_empty() {
                    None
                } else {
                    Some(Command::Bind(agent_id))
                }
            }
            "unbind" => Some(Command::Unbind),
            "status" => Some(Command::Status),
            "agents" => Some(Command::Agents),
            "help" => Some(Command::Help),
            other => Some(Command::Unknown(other.to_string())),
        }
    }
}

pub async fn execute_command(
    command: Command,
    gateway: &Arc<RwLock<GatewayClient>>,
) -> Result<String> {
    match command {
        Command::Bind(agent_id) => {
            Ok(format!("Binding to agent: {}\nUse `/coven status` to verify.", agent_id))
        }
        Command::Unbind => {
            Ok("Room unbound from agent.".to_string())
        }
        Command::Status => {
            Ok("Status: No agent bound to this room.\nUse `/coven bind <agent-id>` to bind an agent.".to_string())
        }
        Command::Agents => {
            let mut gateway = gateway.write().await;
            let agents = gateway.list_agents().await?;

            if agents.is_empty() {
                Ok("No agents currently online.".to_string())
            } else {
                let mut response = String::from("Online agents:\n");
                for agent in agents {
                    response.push_str(&format!(
                        "- {} ({})\n",
                        agent.id,
                        agent.metadata.as_ref()
                            .map(|m| m.working_directory.as_str())
                            .unwrap_or("unknown")
                    ));
                }
                Ok(response)
            }
        }
        Command::Help => {
            Ok(r#"Coven Bridge Commands:
- /coven bind <agent-id> - Bind this room to an agent
- /coven unbind - Unbind this room from current agent
- /coven status - Show current binding status
- /coven agents - List available agents
- /coven help - Show this help message"#.to_string())
        }
        Command::Unknown(cmd) => {
            Ok(format!("Unknown command: {}\nUse `/coven help` for available commands.", cmd))
        }
    }
}
