// ABOUTME: Error types for coven-matrix-rs.
// ABOUTME: Defines BridgeError enum for all bridge failure modes.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum BridgeError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Matrix error: {0}")]
    Matrix(#[from] matrix_sdk::Error),

    #[error("Matrix client build error: {0}")]
    MatrixBuild(#[from] matrix_sdk::ClientBuildError),

    #[error("Gateway error: {0}")]
    Gateway(#[from] tonic::Status),

    #[error("Connection error: {0}")]
    Connection(#[from] tonic::transport::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, BridgeError>;
