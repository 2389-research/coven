// ABOUTME: Error types for coven-matrix-rs.
// ABOUTME: Defines BridgeError enum for all bridge failure modes.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum BridgeError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Matrix error: {0}")]
    Matrix(Box<matrix_sdk::Error>),

    #[error("Matrix client build error: {0}")]
    MatrixBuild(Box<matrix_sdk::ClientBuildError>),

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

impl From<matrix_sdk::Error> for BridgeError {
    fn from(e: matrix_sdk::Error) -> Self {
        BridgeError::Matrix(Box::new(e))
    }
}

impl From<matrix_sdk::ClientBuildError> for BridgeError {
    fn from(e: matrix_sdk::ClientBuildError) -> Self {
        BridgeError::MatrixBuild(Box::new(e))
    }
}

pub type Result<T> = std::result::Result<T, BridgeError>;
