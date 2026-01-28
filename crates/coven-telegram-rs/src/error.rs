// ABOUTME: Error types for coven-telegram-rs.
// ABOUTME: Defines BridgeError enum covering Telegram, Gateway, Config, and IO failures.

use thiserror::Error;

/// Error types for the Telegram bridge.
#[derive(Error, Debug)]
pub enum BridgeError {
    /// Configuration loading or validation error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Telegram API error from teloxide.
    #[error("Telegram API error: {0}")]
    Telegram(String),

    /// Telegram request error from teloxide.
    #[error("Telegram request error: {0}")]
    TeloxideRequest(#[from] teloxide::RequestError),

    /// gRPC status error from gateway communication.
    #[error("Gateway error: {0}")]
    Gateway(#[from] tonic::Status),

    /// gRPC connection/transport error.
    #[error("Connection error: {0}")]
    Connection(#[from] tonic::transport::Error),

    /// IO error for file operations.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type alias using BridgeError.
pub type Result<T> = std::result::Result<T, BridgeError>;
