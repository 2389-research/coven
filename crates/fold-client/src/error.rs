// ABOUTME: Error types for fold-client
// ABOUTME: Unified error handling across FFI boundary

use thiserror::Error;

/// Errors that can occur in fold-client operations
#[derive(Debug, Error)]
pub enum FoldError {
    #[error("Connection failed: {0}")]
    Connection(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("Stream error: {0}")]
    Stream(String),

    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    #[error("Already streaming to this agent")]
    AlreadyStreaming,

    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

impl From<tonic::Status> for FoldError {
    fn from(status: tonic::Status) -> Self {
        match status.code() {
            tonic::Code::Unavailable | tonic::Code::Unknown => {
                FoldError::Connection(status.message().to_string())
            }
            tonic::Code::NotFound => FoldError::AgentNotFound(status.message().to_string()),
            tonic::Code::Cancelled | tonic::Code::Aborted => {
                FoldError::Stream(status.message().to_string())
            }
            _ => FoldError::Api(status.message().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fold_error_display_connection() {
        let err = FoldError::Connection("timeout".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Connection failed"));
        assert!(display.contains("timeout"));
    }

    #[test]
    fn test_fold_error_display_api() {
        let err = FoldError::Api("rate limited".to_string());
        let display = format!("{}", err);
        assert!(display.contains("API error"));
        assert!(display.contains("rate limited"));
    }

    #[test]
    fn test_fold_error_display_stream() {
        let err = FoldError::Stream("disconnected".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Stream error"));
        assert!(display.contains("disconnected"));
    }

    #[test]
    fn test_fold_error_display_agent_not_found() {
        let err = FoldError::AgentNotFound("agent-123".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Agent not found"));
        assert!(display.contains("agent-123"));
    }

    #[test]
    fn test_fold_error_display_already_streaming() {
        let err = FoldError::AlreadyStreaming;
        let display = format!("{}", err);
        assert!(display.contains("Already streaming"));
    }

    #[test]
    fn test_fold_error_display_invalid_response() {
        let err = FoldError::InvalidResponse("malformed JSON".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Invalid response"));
        assert!(display.contains("malformed JSON"));
    }

    #[test]
    fn test_from_tonic_status_unavailable() {
        let status = tonic::Status::unavailable("server down");
        let err: FoldError = status.into();
        assert!(matches!(err, FoldError::Connection(_)));
        if let FoldError::Connection(msg) = err {
            assert_eq!(msg, "server down");
        }
    }

    #[test]
    fn test_from_tonic_status_unknown() {
        let status = tonic::Status::unknown("unknown error");
        let err: FoldError = status.into();
        assert!(matches!(err, FoldError::Connection(_)));
        if let FoldError::Connection(msg) = err {
            assert_eq!(msg, "unknown error");
        }
    }

    #[test]
    fn test_from_tonic_status_not_found() {
        let status = tonic::Status::not_found("agent not found");
        let err: FoldError = status.into();
        assert!(matches!(err, FoldError::AgentNotFound(_)));
        if let FoldError::AgentNotFound(msg) = err {
            assert_eq!(msg, "agent not found");
        }
    }

    #[test]
    fn test_from_tonic_status_cancelled() {
        let status = tonic::Status::cancelled("request cancelled");
        let err: FoldError = status.into();
        assert!(matches!(err, FoldError::Stream(_)));
        if let FoldError::Stream(msg) = err {
            assert_eq!(msg, "request cancelled");
        }
    }

    #[test]
    fn test_from_tonic_status_aborted() {
        let status = tonic::Status::aborted("operation aborted");
        let err: FoldError = status.into();
        assert!(matches!(err, FoldError::Stream(_)));
        if let FoldError::Stream(msg) = err {
            assert_eq!(msg, "operation aborted");
        }
    }

    #[test]
    fn test_from_tonic_status_other_codes() {
        // Test various other status codes that map to Api error
        let codes_and_messages = vec![
            (tonic::Status::invalid_argument("bad arg"), "bad arg"),
            (tonic::Status::deadline_exceeded("timeout"), "timeout"),
            (tonic::Status::permission_denied("forbidden"), "forbidden"),
            (tonic::Status::resource_exhausted("quota"), "quota"),
            (
                tonic::Status::failed_precondition("precondition"),
                "precondition",
            ),
            (tonic::Status::out_of_range("range"), "range"),
            (
                tonic::Status::unimplemented("not implemented"),
                "not implemented",
            ),
            (tonic::Status::internal("internal"), "internal"),
            (tonic::Status::data_loss("lost data"), "lost data"),
            (
                tonic::Status::unauthenticated("unauthenticated"),
                "unauthenticated",
            ),
        ];

        for (status, expected_msg) in codes_and_messages {
            let err: FoldError = status.into();
            assert!(matches!(err, FoldError::Api(_)), "Expected Api error");
            if let FoldError::Api(msg) = err {
                assert_eq!(msg, expected_msg);
            }
        }
    }

    #[test]
    fn test_fold_error_debug() {
        let err = FoldError::Connection("test".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("Connection"));
        assert!(debug_str.contains("test"));
    }
}
