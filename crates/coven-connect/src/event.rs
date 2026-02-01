// ABOUTME: Event conversion utilities for gateway communication
// ABOUTME: Converts between coven-core OutgoingEvent and coven-proto types

use coven_core::OutgoingEvent;
use coven_proto::{agent_message, message_response::Event, AgentMessage, MessageResponse};

use crate::MAX_FILE_SIZE_BYTES;

/// Convert a tool state string to the proto enum value.
/// Used by ToolState events in the response stream.
pub fn tool_state_to_proto(state: &str) -> i32 {
    match state {
        "pending" => coven_proto::ToolState::Pending as i32,
        "awaiting_approval" => coven_proto::ToolState::AwaitingApproval as i32,
        "running" => coven_proto::ToolState::Running as i32,
        "completed" => coven_proto::ToolState::Completed as i32,
        "failed" => coven_proto::ToolState::Failed as i32,
        "denied" => coven_proto::ToolState::Denied as i32,
        "timeout" => coven_proto::ToolState::Timeout as i32,
        "cancelled" => coven_proto::ToolState::Cancelled as i32,
        _ => coven_proto::ToolState::Unspecified as i32,
    }
}

/// Convert an OutgoingEvent to an AgentMessage response.
/// Handles file reading asynchronously with size limits.
pub async fn convert_event_to_response(request_id: &str, event: OutgoingEvent) -> AgentMessage {
    let event = match event {
        OutgoingEvent::Thinking => Event::Thinking("thinking...".to_string()),
        OutgoingEvent::Text(s) => Event::Text(s),
        OutgoingEvent::ToolUse { id, name, input } => Event::ToolUse(coven_proto::ToolUse {
            id,
            name,
            input_json: input.to_string(),
        }),
        OutgoingEvent::ToolResult {
            id,
            output,
            is_error,
        } => Event::ToolResult(coven_proto::ToolResult {
            id,
            output,
            is_error,
        }),
        OutgoingEvent::Done { full_response } => Event::Done(coven_proto::Done { full_response }),
        OutgoingEvent::Error(e) => Event::Error(e),
        OutgoingEvent::ToolApprovalRequest { id, name, input } => {
            Event::ToolApprovalRequest(coven_proto::ToolApprovalRequest {
                id,
                name,
                input_json: input.to_string(),
            })
        }
        OutgoingEvent::File {
            path,
            filename,
            mime_type,
        } => {
            // Check file size before reading to avoid memory issues
            match tokio::fs::metadata(&path).await {
                Ok(metadata) => {
                    let size = metadata.len();
                    if size > MAX_FILE_SIZE_BYTES {
                        Event::Error(format!(
                            "File '{}' exceeds size limit: {} bytes (max {} bytes)",
                            path.display(),
                            size,
                            MAX_FILE_SIZE_BYTES
                        ))
                    } else {
                        // Use async read to avoid blocking the runtime
                        match tokio::fs::read(&path).await {
                            Ok(data) => Event::File(coven_proto::FileData {
                                filename,
                                mime_type,
                                data,
                            }),
                            Err(e) => Event::Error(format!(
                                "Failed to read file '{}': {}",
                                path.display(),
                                e
                            )),
                        }
                    }
                }
                Err(e) => Event::Error(format!(
                    "Failed to get file metadata for '{}': {}",
                    path.display(),
                    e
                )),
            }
        }
        OutgoingEvent::SessionInit { session_id } => {
            Event::SessionInit(coven_proto::SessionInit { session_id })
        }
        OutgoingEvent::SessionOrphaned => Event::SessionOrphaned(coven_proto::SessionOrphaned {
            reason: "Session expired".to_string(),
        }),
        OutgoingEvent::Usage {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            thinking_tokens,
        } => Event::Usage(coven_proto::TokenUsage {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            thinking_tokens,
        }),
        OutgoingEvent::ToolState { id, state, detail } => {
            Event::ToolState(coven_proto::ToolStateUpdate {
                id,
                state: tool_state_to_proto(&state),
                detail,
            })
        }
    };

    AgentMessage {
        payload: Some(agent_message::Payload::Response(MessageResponse {
            request_id: request_id.to_string(),
            event: Some(event),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_state_to_proto() {
        assert_eq!(
            tool_state_to_proto("pending"),
            coven_proto::ToolState::Pending as i32
        );
        assert_eq!(
            tool_state_to_proto("running"),
            coven_proto::ToolState::Running as i32
        );
        assert_eq!(
            tool_state_to_proto("completed"),
            coven_proto::ToolState::Completed as i32
        );
        assert_eq!(
            tool_state_to_proto("unknown"),
            coven_proto::ToolState::Unspecified as i32
        );
    }

    #[tokio::test]
    async fn test_convert_thinking_event() {
        let msg = convert_event_to_response("req-1", OutgoingEvent::Thinking).await;

        match msg.payload {
            Some(agent_message::Payload::Response(resp)) => {
                assert_eq!(resp.request_id, "req-1");
                match resp.event {
                    Some(Event::Thinking(s)) => assert_eq!(s, "thinking..."),
                    other => panic!("Expected Thinking event, got {:?}", other),
                }
            }
            other => panic!("Expected Response payload, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_convert_text_event() {
        let msg =
            convert_event_to_response("req-2", OutgoingEvent::Text("hello".to_string())).await;

        match msg.payload {
            Some(agent_message::Payload::Response(resp)) => match resp.event {
                Some(Event::Text(s)) => assert_eq!(s, "hello"),
                other => panic!("Expected Text event, got {:?}", other),
            },
            other => panic!("Expected Response payload, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_convert_error_event() {
        let msg =
            convert_event_to_response("req-3", OutgoingEvent::Error("oops".to_string())).await;

        match msg.payload {
            Some(agent_message::Payload::Response(resp)) => match resp.event {
                Some(Event::Error(s)) => assert_eq!(s, "oops"),
                other => panic!("Expected Error event, got {:?}", other),
            },
            other => panic!("Expected Response payload, got {:?}", other),
        }
    }
}
