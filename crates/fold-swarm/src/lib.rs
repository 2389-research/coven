// ABOUTME: fold-swarm library with supervisor, agent, and init modules.
// ABOUTME: Re-exports for programmatic use of swarm functionality.

pub mod agent;
pub mod init;
pub mod supervisor;

pub use agent::{GatewayClient, Session};
pub use supervisor::{discover_workspaces, socket, AgentProcess, Tui, TuiEvent};
