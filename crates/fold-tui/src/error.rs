// ABOUTME: Application-wide error types.
// ABOUTME: Uses thiserror for ergonomic error handling.

#![allow(dead_code)]

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Client error: {0}")]
    Client(#[from] fold_client::FoldError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Terminal error: {0}")]
    Terminal(String),

    #[error("Gateway connection error: {message}\n\nIs the gateway running at {url}?\nTry 'folder doctor' to diagnose connectivity issues.")]
    GatewayConnection { message: String, url: String },

    #[error("Agent not found: {name}\n\n{hint}")]
    AgentNotFound { name: String, hint: String },

    #[error("Agent not connected: {name}")]
    AgentNotConnected { name: String },

    #[error("Message send error: {0}")]
    MessageSend(String),

    #[error("Response error: {0}")]
    ResponseError(String),

    #[error("Unknown theme: {name}\n\nAvailable themes:\n{available}")]
    UnknownTheme { name: String, available: String },
}

pub type Result<T> = std::result::Result<T, AppError>;
