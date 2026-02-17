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
    Gateway(Box<tonic::Status>),

    #[error("Connection error: {0}")]
    Connection(Box<tonic::transport::Error>),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<tonic::Status> for BridgeError {
    fn from(e: tonic::Status) -> Self {
        BridgeError::Gateway(Box::new(e))
    }
}

impl From<tonic::transport::Error> for BridgeError {
    fn from(e: tonic::transport::Error) -> Self {
        BridgeError::Connection(Box::new(e))
    }
}

pub type Result<T> = std::result::Result<T, BridgeError>;
