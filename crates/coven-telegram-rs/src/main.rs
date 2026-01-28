// ABOUTME: Entry point for coven-telegram-bridge binary.
// ABOUTME: Loads config, connects to Telegram and gateway, runs Long Polling event loop.

use anyhow::Result;
use clap::Parser;
use coven_telegram_rs::{Bridge, Config, TelegramMessageInfo};
use std::sync::Arc;
use teloxide::prelude::*;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "coven-telegram-bridge")]
#[command(about = "Telegram bridge for coven-gateway using Long Polling")]
struct Cli {
    /// Config file path
    #[arg(short, long, env = "COVEN_TELEGRAM_CONFIG")]
    config: Option<std::path::PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("coven_telegram_rs=info".parse()?),
        )
        .init();

    let cli = Cli::parse();
    info!("coven-telegram-bridge starting");

    // Load configuration
    let config = Config::load(cli.config)?;
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
                // Build message info from the Telegram message
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
