// ABOUTME: Error types for coven-client
// ABOUTME: Unified error handling across FFI boundary

use thiserror::Error;

/// Errors that can occur in coven-client operations
#[derive(Debug, Error)]
pub enum CovenError {
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

impl From<tonic::Status> for CovenError {
    fn from(status: tonic::Status) -> Self {
        match status.code() {
            tonic::Code::Unavailable | tonic::Code::Unknown => {
                CovenError::Connection(status.message().to_string())
            }
            tonic::Code::NotFound => CovenError::AgentNotFound(status.message().to_string()),
            tonic::Code::Cancelled | tonic::Code::Aborted => {
                CovenError::Stream(status.message().to_string())
            }
            _ => CovenError::Api(status.message().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coven_error_display_connection() {
        let err = CovenError::Connection("timeout".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Connection failed"));
        assert!(display.contains("timeout"));
    }

    #[test]
    fn test_coven_error_display_api() {
        let err = CovenError::Api("rate limited".to_string());
        let display = format!("{}", err);
        assert!(display.contains("API error"));
        assert!(display.contains("rate limited"));
    }

    #[test]
    fn test_coven_error_display_stream() {
        let err = CovenError::Stream("disconnected".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Stream error"));
        assert!(display.contains("disconnected"));
    }

    #[test]
    fn test_coven_error_display_agent_not_found() {
        let err = CovenError::AgentNotFound("agent-123".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Agent not found"));
        assert!(display.contains("agent-123"));
    }

    #[test]
    fn test_coven_error_display_already_streaming() {
        let err = CovenError::AlreadyStreaming;
        let display = format!("{}", err);
        assert!(display.contains("Already streaming"));
    }

    #[test]
    fn test_coven_error_display_invalid_response() {
        let err = CovenError::InvalidResponse("malformed JSON".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Invalid response"));
        assert!(display.contains("malformed JSON"));
    }

    #[test]
    fn test_from_tonic_status_unavailable() {
        let status = tonic::Status::unavailable("server down");
        let err: CovenError = status.into();
        assert!(matches!(err, CovenError::Connection(_)));
        if let CovenError::Connection(msg) = err {
            assert_eq!(msg, "server down");
        }
    }

    #[test]
    fn test_from_tonic_status_unknown() {
        let status = tonic::Status::unknown("unknown error");
        let err: CovenError = status.into();
        assert!(matches!(err, CovenError::Connection(_)));
        if let CovenError::Connection(msg) = err {
            assert_eq!(msg, "unknown error");
        }
    }

    #[test]
    fn test_from_tonic_status_not_found() {
        let status = tonic::Status::not_found("agent not found");
        let err: CovenError = status.into();
        assert!(matches!(err, CovenError::AgentNotFound(_)));
        if let CovenError::AgentNotFound(msg) = err {
            assert_eq!(msg, "agent not found");
        }
    }

    #[test]
    fn test_from_tonic_status_cancelled() {
        let status = tonic::Status::cancelled("request cancelled");
        let err: CovenError = status.into();
        assert!(matches!(err, CovenError::Stream(_)));
        if let CovenError::Stream(msg) = err {
            assert_eq!(msg, "request cancelled");
        }
    }

    #[test]
    fn test_from_tonic_status_aborted() {
        let status = tonic::Status::aborted("operation aborted");
        let err: CovenError = status.into();
        assert!(matches!(err, CovenError::Stream(_)));
        if let CovenError::Stream(msg) = err {
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
            let err: CovenError = status.into();
            assert!(matches!(err, CovenError::Api(_)), "Expected Api error");
            if let CovenError::Api(msg) = err {
                assert_eq!(msg, expected_msg);
            }
        }
    }

    #[test]
    fn test_coven_error_debug() {
        let err = CovenError::Connection("test".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("Connection"));
        assert!(debug_str.contains("test"));
    }
}
