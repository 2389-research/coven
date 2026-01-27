// ABOUTME: Library root for coven-slack-rs.
// ABOUTME: Exports bridge, config, context, commands, and error modules.

pub mod bridge;
pub mod commands;
pub mod config;
pub mod context;
pub mod error;
pub mod gateway;
pub mod slack;

pub use bridge::{Bridge, ChannelBinding};
pub use config::{Config, ResponseMode};
pub use context::SlackContext;
pub use error::{BridgeError, Result};
pub use gateway::GatewayClient;
pub use slack::{CovenSlackClient, SlackMessageInfo};
