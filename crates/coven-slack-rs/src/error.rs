// ABOUTME: Error types for coven-slack-rs.
// ABOUTME: Defines BridgeError enum covering Slack, Gateway, Config, and IO failures.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum BridgeError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Slack API error: {0}")]
    Slack(String),

    #[error("Slack client error: {0}")]
    SlackClient(#[from] slack_morphism::errors::SlackClientError),

    #[error("Gateway error: {0}")]
    Gateway(#[from] tonic::Status),

    #[error("Connection error: {0}")]
    Connection(#[from] tonic::transport::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, BridgeError>;
