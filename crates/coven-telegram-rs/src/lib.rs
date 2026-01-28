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

use std::path::PathBuf;
use std::sync::Arc;
use teloxide::prelude::*;
use tracing::{error, info};

/// Run the Telegram bridge with the given config path.
pub async fn run(config_path: Option<PathBuf>) -> anyhow::Result<()> {
    info!("coven-telegram-bridge starting");

    // Load configuration
    let config = Config::load(config_path)?;
    info!(
        gateway = %config.gateway.url,
        response_mode = ?config.bridge.response_mode,
        "Configuration loaded"
    );

    // Create the bridge
    let bridge = Arc::new(Bridge::new(config.clone()).await?);
    info!("Bridge initialized");

    // Get a reference to the telegram bot for the dispatcher
    let bot = bridge.telegram_bot().inner().clone();

    // Create the message handler
    let bridge_for_handler = Arc::clone(&bridge);
    let handler = Update::filter_message().endpoint(
        move |msg: Message, _bot: Bot| {
            let bridge = Arc::clone(&bridge_for_handler);
            async move {
                if let Some(msg_info) = TelegramMessageInfo::from_message(&msg, bridge.telegram_bot()) {
                    if let Err(e) = bridge.handle_message(msg_info).await {
                        error!(error = %e, "Failed to handle message");
                    }
                }
                Ok::<(), std::convert::Infallible>(())
            }
        },
    );

    // Create and run the dispatcher
    info!("Starting Long Polling dispatcher");
    let mut dispatcher = Dispatcher::builder(bot, handler)
        .enable_ctrlc_handler()
        .build();

    // Handle shutdown signals
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install ctrl+c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    // Run dispatcher until shutdown signal
    tokio::select! {
        _ = dispatcher.dispatch() => {
            info!("Dispatcher stopped");
        }
        _ = ctrl_c => {
            info!("Received Ctrl+C, shutting down");
        }
        _ = terminate => {
            info!("Received terminate signal, shutting down");
        }
    }

    info!("coven-telegram-bridge stopped");
    Ok(())
}
