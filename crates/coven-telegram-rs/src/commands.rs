// ABOUTME: Handles /coven commands for chat binding management.
// ABOUTME: Supports bind, unbind, status, agents, and help commands.

use crate::bridge::ChatBinding;
use crate::error::Result;
use crate::gateway::GatewayClient;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Parsed command from /coven command text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Bind chat to an agent: /coven bind <agent-id>
    Bind(String),
    /// Unbind chat from current agent: /coven unbind
    Unbind,
    /// Show current binding status: /coven status
    Status,
    /// List available agents: /coven agents
    Agents,
    /// Show help message: /coven help or /coven
    Help,
    /// Unknown command with the unrecognized text.
    Unknown(String),
}

impl Command {
    /// Parse command text into a Command.
    /// The text comes after "/coven" has been stripped.
    pub fn parse(text: &str) -> Command {
        let text = text.trim();

        if text.is_empty() || text == "help" {
            return Command::Help;
        }

        let parts: Vec<&str> = text.splitn(2, ' ').collect();
        match parts[0] {
            "bind" => match parts.get(1).map(|s| s.trim().to_string()) {
                Some(agent_id) if !agent_id.is_empty() => Command::Bind(agent_id),
                _ => Command::Unknown(
                    "bind (requires agent-id, e.g., /coven bind agent-123)".to_string(),
                ),
            },
            "unbind" => Command::Unbind,
            "status" => Command::Status,
            "agents" => Command::Agents,
            "help" => Command::Help,
            other => Command::Unknown(other.to_string()),
        }
    }

    /// Check if text is a /coven command.
    pub fn is_command(text: &str) -> bool {
        let trimmed = text.trim();
        trimmed.starts_with("/coven")
    }

    /// Parse from raw message text that includes the /coven prefix.
    pub fn from_message(text: &str) -> Option<Command> {
        let trimmed = text.trim();
        let rest = trimmed.strip_prefix("/coven")?;
        Some(Self::parse(rest))
    }
}

/// Context required for executing commands that modify state.
pub struct CommandContext<'a> {
    pub gateway: &'a Arc<RwLock<GatewayClient>>,
    pub bindings: &'a Arc<RwLock<HashMap<i64, ChatBinding>>>,
    pub chat_id: i64,
}

/// Execute a command and return the response text.
pub async fn execute_command(command: Command, ctx: CommandContext<'_>) -> Result<String> {
    match command {
        Command::Bind(agent_id) => {
            let binding = ChatBinding {
                chat_id: ctx.chat_id,
                conversation_key: agent_id.clone(),
            };
            ctx.bindings.write().await.insert(ctx.chat_id, binding);
            info!(
                chat_id = %ctx.chat_id,
                agent_id = %agent_id,
                "Chat bound to agent via command"
            );
            Ok(format!(
                "✅ Bound this chat to agent: `{}`\nUse /coven status to verify.",
                agent_id
            ))
        }

        Command::Unbind => {
            let removed = ctx.bindings.write().await.remove(&ctx.chat_id);
            info!(chat_id = %ctx.chat_id, "Chat unbound via command");
            match removed {
                Some(binding) => Ok(format!(
                    "❌ Unbound this chat from agent: `{}`",
                    binding.conversation_key
                )),
                None => Ok("ℹ️ This chat was not bound to any agent.".to_string()),
            }
        }

        Command::Status => {
            let bindings = ctx.bindings.read().await;
            match bindings.get(&ctx.chat_id) {
                Some(binding) => Ok(format!(
                    "🔗 *Status*: Bound to agent `{}`\nChat ID: `{}`",
                    binding.conversation_key, ctx.chat_id
                )),
                None => Ok(format!(
                    "⚠️ *Status*: No agent bound to this chat.\nChat ID: `{}`\n\nUse /coven bind <agent-id> to bind an agent.",
                    ctx.chat_id
                )),
            }
        }

        Command::Agents => {
            let mut gateway = ctx.gateway.write().await;
            let agents = gateway.list_agents().await?;

            if agents.is_empty() {
                Ok("🤖 No agents currently online.".to_string())
            } else {
                let mut response = String::from("🤖 *Online agents:*\n");
                for agent in agents {
                    let working_dir = agent
                        .metadata
                        .as_ref()
                        .map(|m| m.working_directory.as_str())
                        .unwrap_or("unknown");
                    response.push_str(&format!("• `{}` _({})\n", agent.id, working_dir));
                }
                Ok(response)
            }
        }

        Command::Help => Ok(r#"📖 *Coven Bridge Commands:*

• `/coven bind <agent-id>` - Bind this chat to an agent
• `/coven unbind` - Unbind this chat from current agent
• `/coven status` - Show current binding status
• `/coven agents` - List available agents
• `/coven help` - Show this help message

_Messages in bound chats will be forwarded to the agent._"#
            .to_string()),

        Command::Unknown(cmd) => Ok(format!(
            "❓ Unknown command: `{}`\nUse /coven help for available commands.",
            cmd
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_parse_help() {
        assert_eq!(Command::parse(""), Command::Help);
        assert_eq!(Command::parse("help"), Command::Help);
        assert_eq!(Command::parse("  help  "), Command::Help);
    }

    #[test]
    fn test_command_parse_bind() {
        assert_eq!(
            Command::parse("bind agent-123"),
            Command::Bind("agent-123".to_string())
        );
        assert_eq!(
            Command::parse("bind my-agent-with-dashes"),
            Command::Bind("my-agent-with-dashes".to_string())
        );
    }

    #[test]
    fn test_command_parse_bind_without_id() {
        let cmd = Command::parse("bind");
        match cmd {
            Command::Unknown(msg) => assert!(msg.contains("requires agent-id")),
            _ => panic!("Expected Unknown command"),
        }

        let cmd2 = Command::parse("bind   ");
        match cmd2 {
            Command::Unknown(msg) => assert!(msg.contains("requires agent-id")),
            _ => panic!("Expected Unknown command"),
        }
    }

    #[test]
    fn test_command_parse_other() {
        assert_eq!(Command::parse("unbind"), Command::Unbind);
        assert_eq!(Command::parse("status"), Command::Status);
        assert_eq!(Command::parse("agents"), Command::Agents);
    }

    #[test]
    fn test_command_parse_unknown() {
        assert_eq!(Command::parse("foo"), Command::Unknown("foo".to_string()));
    }

    #[test]
    fn test_is_command() {
        assert!(Command::is_command("/coven help"));
        assert!(Command::is_command("/coven bind agent-1"));
        assert!(Command::is_command("  /coven status"));
        assert!(!Command::is_command("hello world"));
        assert!(!Command::is_command("/other command"));
    }

    #[test]
    fn test_from_message() {
        assert_eq!(Command::from_message("/coven help"), Some(Command::Help));
        assert_eq!(
            Command::from_message("/coven bind agent-1"),
            Some(Command::Bind("agent-1".to_string()))
        );
        assert!(Command::from_message("hello world").is_none());
    }
}
