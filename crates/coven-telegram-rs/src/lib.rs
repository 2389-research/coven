// ABOUTME: Library root for coven-telegram-rs.
// ABOUTME: Exports bridge, config, context, commands, and error modules.

pub mod bridge;
pub mod commands;
pub mod config;
pub mod context;
pub mod error;
pub mod gateway;
pub mod telegram;

pub use bridge::{Bridge, ChatBinding};
pub use config::{Config, ResponseMode};
pub use context::TelegramContext;
pub use error::{BridgeError, Result};
pub use gateway::GatewayClient;
pub use telegram::{CovenTelegramBot, TelegramMessageInfo};
