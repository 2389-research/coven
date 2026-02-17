// ABOUTME: Library interface for coven-human.
// ABOUTME: Exposes the human agent TUI for responding to agent messages.

mod app;
mod messages;
mod ui;

pub use app::{Action, App};
pub use messages::{AppEvent, ConnectionEvent, IncomingMessageEvent, Message, MessageDirection};

/// Configuration for the human agent TUI
#[derive(Debug, Clone)]
pub struct HumanConfig {
    /// Gateway server URL (defaults to config or http://127.0.0.1:50051)
    pub gateway: Option<String>,
    /// Agent name (defaults to hostname)
    pub name: Option<String>,
    /// Agent ID (auto-generated UUID if not provided)
    pub id: Option<String>,
}

/// Run the human agent TUI
pub async fn run_human(config: HumanConfig) -> anyhow::Result<()> {
    app::run(config).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_config_defaults() {
        let config = HumanConfig {
            gateway: None,
            name: None,
            id: None,
        };
        assert!(config.gateway.is_none());
        assert!(config.name.is_none());
        assert!(config.id.is_none());
    }

    #[test]
    fn test_human_config_with_values() {
        let config = HumanConfig {
            gateway: Some("http://localhost:50051".to_string()),
            name: Some("human-1".to_string()),
            id: Some("agent-abc".to_string()),
        };
        assert_eq!(config.gateway.as_deref(), Some("http://localhost:50051"));
        assert_eq!(config.name.as_deref(), Some("human-1"));
        assert_eq!(config.id.as_deref(), Some("agent-abc"));
    }
}
