// ABOUTME: Supervisor daemon that spawns and manages workspace agents.
// ABOUTME: Provides Unix socket API for dispatch agent to manage swarm.

pub mod discover;
pub mod socket;
pub mod spawn;
pub mod tui;

pub use discover::discover_workspaces;
pub use spawn::AgentProcess;
pub use tui::{Tui, TuiEvent};
