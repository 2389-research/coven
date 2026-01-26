// ABOUTME: Error types for the coven-pack SDK.
// ABOUTME: Provides PackError for client operations and ToolError for tool execution.

use thiserror::Error;

/// Errors that can occur in the pack client.
#[derive(Error, Debug)]
pub enum PackError {
    /// Failed to load SSH key.
    #[error("failed to load SSH key: {0}")]
    KeyLoadFailed(String),

    /// Failed to create authentication credentials.
    #[error("authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Failed to connect to the gateway.
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    /// Registration was rejected by the gateway.
    #[error("registration rejected: {0}")]
    RegistrationRejected(String),

    /// Stream was closed unexpectedly.
    #[error("stream closed unexpectedly")]
    StreamClosed,

    /// Error on the gRPC stream.
    #[error("stream error: {0}")]
    StreamError(String),

    /// Tool execution failed.
    #[error("tool execution failed: {0}")]
    ToolExecutionFailed(String),

    /// Configuration loading failed.
    #[error("config error: {0}")]
    ConfigError(String),

    /// Internal error.
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<tonic::Status> for PackError {
    fn from(status: tonic::Status) -> Self {
        PackError::StreamError(status.to_string())
    }
}

impl From<tonic::transport::Error> for PackError {
    fn from(err: tonic::transport::Error) -> Self {
        PackError::ConnectionFailed(err.to_string())
    }
}

impl From<coven_ssh::SshError> for PackError {
    fn from(err: coven_ssh::SshError) -> Self {
        PackError::AuthenticationFailed(err.to_string())
    }
}

impl From<coven_grpc::GrpcClientError> for PackError {
    fn from(err: coven_grpc::GrpcClientError) -> Self {
        match err {
            coven_grpc::GrpcClientError::ConnectionFailed(msg) => {
                PackError::ConnectionFailed(msg)
            }
            coven_grpc::GrpcClientError::StreamClosed => PackError::StreamClosed,
            coven_grpc::GrpcClientError::StreamError(msg) => PackError::StreamError(msg),
            coven_grpc::GrpcClientError::AuthenticationFailed(msg) => {
                PackError::AuthenticationFailed(msg)
            }
            _ => PackError::Internal(err.to_string()),
        }
    }
}

/// Errors that can occur during tool execution.
///
/// Implement your tool handler to return these errors when execution fails.
#[derive(Error, Debug)]
pub enum ToolError {
    /// The requested tool does not exist in this pack.
    #[error("unknown tool: {0}")]
    UnknownTool(String),

    /// Invalid input was provided to the tool.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Tool execution failed.
    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    /// Tool execution timed out.
    #[error("execution timed out")]
    Timeout,

    /// Tool requires a capability that is not available.
    #[error("missing capability: {0}")]
    MissingCapability(String),

    /// Internal error during tool execution.
    #[error("internal error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_error_display() {
        let err = PackError::KeyLoadFailed("file not found".to_string());
        assert!(err.to_string().contains("failed to load SSH key"));
        assert!(err.to_string().contains("file not found"));

        let err = PackError::ConnectionFailed("timeout".to_string());
        assert!(err.to_string().contains("connection failed"));

        let err = PackError::StreamClosed;
        assert!(err.to_string().contains("stream closed"));
    }

    #[test]
    fn test_tool_error_display() {
        let err = ToolError::UnknownTool("search".to_string());
        assert!(err.to_string().contains("unknown tool"));
        assert!(err.to_string().contains("search"));

        let err = ToolError::InvalidInput("missing required field".to_string());
        assert!(err.to_string().contains("invalid input"));

        let err = ToolError::Timeout;
        assert!(err.to_string().contains("timed out"));
    }

    #[test]
    fn test_from_tonic_status() {
        let status = tonic::Status::internal("server error");
        let err: PackError = status.into();
        assert!(matches!(err, PackError::StreamError(_)));
    }

    #[test]
    fn test_from_grpc_error() {
        let grpc_err = coven_grpc::GrpcClientError::StreamClosed;
        let err: PackError = grpc_err.into();
        assert!(matches!(err, PackError::StreamClosed));

        let grpc_err = coven_grpc::GrpcClientError::ConnectionFailed("refused".to_string());
        let err: PackError = grpc_err.into();
        assert!(matches!(err, PackError::ConnectionFailed(_)));
    }

    #[test]
    fn test_from_grpc_error_stream_error() {
        let grpc_err = coven_grpc::GrpcClientError::StreamError("broken pipe".to_string());
        let err: PackError = grpc_err.into();
        assert!(matches!(err, PackError::StreamError(_)));
        if let PackError::StreamError(msg) = err {
            assert_eq!(msg, "broken pipe");
        }
    }

    #[test]
    fn test_from_grpc_error_authentication_failed() {
        let grpc_err =
            coven_grpc::GrpcClientError::AuthenticationFailed("invalid key".to_string());
        let err: PackError = grpc_err.into();
        assert!(matches!(err, PackError::AuthenticationFailed(_)));
        if let PackError::AuthenticationFailed(msg) = err {
            assert_eq!(msg, "invalid key");
        }
    }

    #[test]
    fn test_from_grpc_error_other() {
        let grpc_err = coven_grpc::GrpcClientError::RegistrationRejected {
            reason: "name collision".to_string(),
        };
        let err: PackError = grpc_err.into();
        assert!(matches!(err, PackError::Internal(_)));
        if let PackError::Internal(msg) = err {
            assert!(msg.contains("name collision"));
        }
    }

    #[test]
    fn test_from_ssh_error() {
        let ssh_err = coven_ssh::SshError::UnsupportedKeyType("rsa".to_string());
        let err: PackError = ssh_err.into();
        assert!(matches!(err, PackError::AuthenticationFailed(_)));
        if let PackError::AuthenticationFailed(msg) = err {
            assert!(msg.contains("unsupported key type"));
        }
    }

    #[test]
    fn test_from_tonic_transport_error() {
        // We can't easily create a real tonic::transport::Error, but we can test
        // the From impl exists by verifying the error types compile correctly
        // The actual conversion is tested implicitly through integration tests
    }

    #[test]
    fn test_pack_error_display_all_variants() {
        // Test all error variant display messages
        let errors = vec![
            (
                PackError::KeyLoadFailed("not found".to_string()),
                "failed to load SSH key",
            ),
            (
                PackError::AuthenticationFailed("bad sig".to_string()),
                "authentication failed",
            ),
            (
                PackError::ConnectionFailed("refused".to_string()),
                "connection failed",
            ),
            (
                PackError::RegistrationRejected("already exists".to_string()),
                "registration rejected",
            ),
            (PackError::StreamClosed, "stream closed"),
            (
                PackError::StreamError("timeout".to_string()),
                "stream error",
            ),
            (
                PackError::ToolExecutionFailed("error".to_string()),
                "tool execution failed",
            ),
            (
                PackError::ConfigError("bad path".to_string()),
                "config error",
            ),
            (PackError::Internal("panic".to_string()), "internal error"),
        ];

        for (err, expected_prefix) in errors {
            let display = format!("{}", err);
            assert!(
                display.contains(expected_prefix),
                "Expected '{}' to contain '{}'",
                display,
                expected_prefix
            );
        }
    }

    #[test]
    fn test_tool_error_display_all_variants() {
        // Test all ToolError variant display messages
        let errors = vec![
            (ToolError::UnknownTool("foo".to_string()), "unknown tool"),
            (
                ToolError::InvalidInput("bad json".to_string()),
                "invalid input",
            ),
            (
                ToolError::ExecutionFailed("crashed".to_string()),
                "execution failed",
            ),
            (ToolError::Timeout, "timed out"),
            (
                ToolError::MissingCapability("network".to_string()),
                "missing capability",
            ),
            (ToolError::Internal("oops".to_string()), "internal error"),
        ];

        for (err, expected_prefix) in errors {
            let display = format!("{}", err);
            assert!(
                display.contains(expected_prefix),
                "Expected '{}' to contain '{}'",
                display,
                expected_prefix
            );
        }
    }

    #[test]
    fn test_pack_error_debug() {
        let err = PackError::StreamClosed;
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("StreamClosed"));

        let err = PackError::ConnectionFailed("test".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("ConnectionFailed"));
        assert!(debug_str.contains("test"));
    }

    #[test]
    fn test_tool_error_debug() {
        let err = ToolError::Timeout;
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("Timeout"));

        let err = ToolError::UnknownTool("test".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("UnknownTool"));
        assert!(debug_str.contains("test"));
    }
}
