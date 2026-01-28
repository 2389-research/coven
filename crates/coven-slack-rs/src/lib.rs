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

use slack_morphism::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};

/// Error handler for Socket Mode events.
fn socket_mode_error_handler(
    err: Box<dyn std::error::Error + Send + Sync>,
    _client: Arc<SlackHyperClient>,
    _states: SlackClientEventsUserState,
) -> HttpStatusCode {
    error!(error = %err, "Socket Mode error");
    HttpStatusCode::OK
}

/// Run the Slack bridge with the given config path.
pub async fn run(config_path: Option<PathBuf>) -> anyhow::Result<()> {
    info!("coven-slack-bridge starting");

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

    // Set up Socket Mode listener
    let client = Arc::new(slack_morphism::SlackClient::new(
        SlackClientHyperConnector::new()?,
    ));

    // Create listener environment with the bridge as user state
    let listener_environment = Arc::new(
        SlackClientEventsListenerEnvironment::new(client.clone())
            .with_error_handler(socket_mode_error_handler)
            .with_user_state(bridge.clone()),
    );

    // Set up event callbacks
    let socket_mode_callbacks = SlackSocketModeListenerCallbacks::new()
        .with_push_events(handle_push_event)
        .with_command_events(handle_slash_command);

    let socket_mode_listener = SlackClientSocketModeListener::new(
        &SlackClientSocketModeConfig::new(),
        listener_environment.clone(),
        socket_mode_callbacks,
    );

    // Start listening with app token
    let app_token_value: SlackApiTokenValue = config.slack.app_token.clone().into();
    let app_token = SlackApiToken::new(app_token_value);

    info!("Starting Socket Mode listener");
    socket_mode_listener.listen_for(&app_token).await?;

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

    // Run until shutdown signal
    tokio::select! {
        _ = socket_mode_listener.serve() => {
            info!("Socket Mode listener stopped");
        }
        _ = ctrl_c => {
            info!("Received Ctrl+C, shutting down");
        }
        _ = terminate => {
            info!("Received terminate signal, shutting down");
        }
    }

    info!("coven-slack-bridge stopped");
    Ok(())
}

/// Handle push events (messages, app mentions, etc.)
async fn handle_push_event(
    event: SlackPushEventCallback,
    _client: Arc<SlackHyperClient>,
    states: SlackClientEventsUserState,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state_guard = states.read().await;
    let bridge: &Arc<Bridge> = state_guard
        .get_user_state::<Arc<Bridge>>()
        .ok_or("Missing bridge state")?;
    let bridge = Arc::clone(bridge);
    drop(state_guard);

    match event.event {
        SlackEventCallbackBody::Message(msg_event) => {
            if let Some(msg_info) = SlackMessageInfo::from_message_event(
                &msg_event,
                bridge.slack_client().bot_user_id(),
            ) {
                let bridge = Arc::clone(&bridge);
                tokio::spawn(async move {
                    if let Err(e) = bridge.handle_message(msg_info).await {
                        error!(error = %e, "Failed to handle message");
                    }
                });
            }
        }
        SlackEventCallbackBody::AppMention(mention_event) => {
            let channel_id = mention_event.channel.to_string();
            let user_id = mention_event.user.to_string();
            let text = mention_event.content.text.clone().unwrap_or_default();
            let message_ts = mention_event.origin.ts.to_string();
            let thread_ts = mention_event
                .origin
                .thread_ts
                .as_ref()
                .map(|ts| ts.to_string());

            let context = bridge
                .slack_client()
                .build_context(&channel_id, thread_ts.as_deref());

            let msg_info = SlackMessageInfo {
                channel_id,
                user_id,
                text,
                message_ts,
                thread_ts,
                is_mention: true,
                context,
            };

            let bridge = Arc::clone(&bridge);
            tokio::spawn(async move {
                if let Err(e) = bridge.handle_message(msg_info).await {
                    error!(error = %e, "Failed to handle app mention");
                }
            });
        }
        _ => {}
    }

    Ok(())
}

/// Handle /coven slash command events.
async fn handle_slash_command(
    event: SlackCommandEvent,
    _client: Arc<SlackHyperClient>,
    states: SlackClientEventsUserState,
) -> std::result::Result<SlackCommandEventResponse, Box<dyn std::error::Error + Send + Sync>> {
    let state_guard = states.read().await;
    let bridge: &Arc<Bridge> = state_guard
        .get_user_state::<Arc<Bridge>>()
        .ok_or("Missing bridge state")?;
    let bridge = Arc::clone(bridge);
    drop(state_guard);

    if event.command.0 != "/coven" {
        return Ok(SlackCommandEventResponse::new(
            SlackMessageContent::new().with_text("Unknown command".to_string()),
        ));
    }

    let channel_id = event.channel_id.to_string();
    let command_text = event.text.clone().unwrap_or_default();

    info!(
        channel_id = %channel_id,
        user_id = %event.user_id,
        command_text = %command_text,
        "Processing /coven slash command"
    );

    let command = commands::Command::parse(&command_text);
    let ctx = commands::CommandContext {
        gateway: bridge.gateway_client(),
        bindings: bridge.bindings(),
        channel_id: &channel_id,
    };

    let response_text = match commands::execute_command(command, ctx).await {
        Ok(resp) => resp,
        Err(e) => format!(":x: Command error: {}", e),
    };

    Ok(SlackCommandEventResponse::new(
        SlackMessageContent::new().with_text(response_text),
    ))
}
