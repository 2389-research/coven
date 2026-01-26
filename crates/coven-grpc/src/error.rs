// ABOUTME: Error types for the coven-grpc crate.
// ABOUTME: Provides structured errors for channel, registration, and stream operations.

use thiserror::Error;

/// Errors that can occur in the gRPC client.
#[derive(Error, Debug)]
pub enum GrpcClientError {
    /// Invalid server address format.
    #[error("invalid server address: {0}")]
    InvalidAddress(String),

    /// Failed to connect to the server.
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    /// Maximum registration attempts exceeded.
    #[error("max registration attempts ({attempts}) exceeded for base ID '{base_id}'")]
    MaxRegistrationAttempts { attempts: usize, base_id: String },

    /// Registration was rejected.
    #[error("registration rejected: {reason}")]
    RegistrationRejected { reason: String },

    /// Server sent unexpected message during registration.
    #[error("unexpected message during registration: {0}")]
    UnexpectedRegistrationMessage(String),

    /// Stream was closed unexpectedly.
    #[error("stream closed unexpectedly")]
    StreamClosed,

    /// Error on the gRPC stream.
    #[error("stream error: {0}")]
    StreamError(String),

    /// Server requested shutdown.
    #[error("server shutdown: {0}")]
    ServerShutdown(String),

    /// Message handling failed.
    #[error("message handling failed: {0}")]
    HandlerError(String),

    /// Authentication failed.
    #[error("authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Protocol error.
    #[error("protocol error: {0}")]
    ProtocolError(String),
}

impl From<tonic::Status> for GrpcClientError {
    fn from(status: tonic::Status) -> Self {
        GrpcClientError::StreamError(status.to_string())
    }
}

impl From<tonic::transport::Error> for GrpcClientError {
    fn from(err: tonic::transport::Error) -> Self {
        GrpcClientError::ConnectionFailed(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = GrpcClientError::InvalidAddress("not a url".to_string());
        assert_eq!(err.to_string(), "invalid server address: not a url");

        let err = GrpcClientError::MaxRegistrationAttempts {
            attempts: 100,
            base_id: "my-agent".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "max registration attempts (100) exceeded for base ID 'my-agent'"
        );
    }

    #[test]
    fn test_from_tonic_status() {
        let status = tonic::Status::internal("test error");
        let err: GrpcClientError = status.into();
        assert!(matches!(err, GrpcClientError::StreamError(_)));
    }

    #[test]
    fn test_all_error_variants_display() {
        // Test display output for all error variants
        let invalid_address = GrpcClientError::InvalidAddress("bad url".to_string());
        assert!(invalid_address
            .to_string()
            .contains("invalid server address"));

        let connection_failed = GrpcClientError::ConnectionFailed("timeout".to_string());
        assert!(connection_failed.to_string().contains("connection failed"));

        let max_attempts = GrpcClientError::MaxRegistrationAttempts {
            attempts: 50,
            base_id: "test".to_string(),
        };
        assert!(max_attempts.to_string().contains("50"));
        assert!(max_attempts.to_string().contains("test"));

        let rejected = GrpcClientError::RegistrationRejected {
            reason: "denied".to_string(),
        };
        assert!(rejected.to_string().contains("registration rejected"));

        let unexpected_msg =
            GrpcClientError::UnexpectedRegistrationMessage("wrong type".to_string());
        assert!(unexpected_msg.to_string().contains("unexpected message"));

        let stream_closed = GrpcClientError::StreamClosed;
        assert!(stream_closed.to_string().contains("stream closed"));

        let stream_error = GrpcClientError::StreamError("broken".to_string());
        assert!(stream_error.to_string().contains("stream error"));

        let server_shutdown = GrpcClientError::ServerShutdown("maintenance".to_string());
        assert!(server_shutdown.to_string().contains("server shutdown"));

        let handler_error = GrpcClientError::HandlerError("failed".to_string());
        assert!(handler_error
            .to_string()
            .contains("message handling failed"));

        let auth_failed = GrpcClientError::AuthenticationFailed("bad token".to_string());
        assert!(auth_failed.to_string().contains("authentication failed"));

        let protocol_error = GrpcClientError::ProtocolError("version mismatch".to_string());
        assert!(protocol_error.to_string().contains("protocol error"));
    }

    #[test]
    fn test_error_debug() {
        let err = GrpcClientError::StreamClosed;
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("StreamClosed"));
    }

    #[test]
    fn test_from_tonic_status_various_codes() {
        let not_found = tonic::Status::not_found("resource not found");
        let err: GrpcClientError = not_found.into();
        assert!(matches!(err, GrpcClientError::StreamError(msg) if msg.contains("NotFound")));

        let permission_denied = tonic::Status::permission_denied("access denied");
        let err: GrpcClientError = permission_denied.into();
        assert!(
            matches!(err, GrpcClientError::StreamError(msg) if msg.contains("PermissionDenied"))
        );
    }

    #[tokio::test]
    async fn test_from_tonic_transport_error() {
        // Create a transport error by trying to connect to an invalid endpoint
        use tonic::transport::Endpoint;

        let endpoint = Endpoint::from_static("http://[::1]:1");
        let result = endpoint.connect().await;

        if let Err(transport_err) = result {
            // Use the From trait to convert
            let grpc_err: GrpcClientError = transport_err.into();
            assert!(matches!(grpc_err, GrpcClientError::ConnectionFailed(_)));
        }
    }
}
