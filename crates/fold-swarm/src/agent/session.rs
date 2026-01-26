// ABOUTME: Session manages a backend and handles prompts.
// ABOUTME: Bridges between gRPC messages and backend events with debounced streaming.

use anyhow::Result;
use fold_swarm_backend::{BackendEvent, BackendHandle};
use futures::StreamExt;
use std::time::{Duration, Instant};

use super::grpc::{fold, ResponseSender};

/// Minimum interval between sending text chunks to avoid flooding the gateway.
const TEXT_DEBOUNCE_MS: u64 = 50;

/// Maximum text buffer size before forcing a send regardless of debounce timer.
const TEXT_BUFFER_MAX: usize = 4096;

pub struct Session {
    backend: BackendHandle,
    session_id: String,
    is_new_session: bool,
}

impl Session {
    pub fn new(backend: BackendHandle) -> Self {
        Self {
            backend,
            session_id: uuid::Uuid::new_v4().to_string(),
            is_new_session: true,
        }
    }

    /// Handle an incoming message and stream responses back with debouncing.
    ///
    /// Text events are accumulated and sent periodically to avoid overwhelming
    /// the gateway's response channel. Non-text events (tools, errors) are sent
    /// immediately. SessionInit/SessionOrphaned events update internal state.
    pub async fn handle_message(&mut self, msg: fold::SendMessage, tx: ResponseSender) -> Result<()> {
        let request_id = msg.request_id.clone();
        let mut accumulated_text = String::new();
        let mut sent_done = false;

        // Text debouncing state
        let mut text_buffer = String::new();
        let mut last_text_send = Instant::now();
        let debounce_interval = Duration::from_millis(TEXT_DEBOUNCE_MS);

        match self.backend.send(&self.session_id, &msg.content, self.is_new_session).await {
            Ok(mut stream) => {
                while let Some(event) = stream.next().await {
                    match event {
                        BackendEvent::SessionInit { session_id } => {
                            // Backend assigned a real session ID - update our state
                            tracing::info!(
                                old_id = %self.session_id,
                                new_id = %session_id,
                                "Session initialized by backend"
                            );
                            self.session_id = session_id;
                            self.is_new_session = false;
                        }
                        BackendEvent::SessionOrphaned => {
                            // Session was lost - reset and notify gateway
                            tracing::warn!(
                                session_id = %self.session_id,
                                "Session orphaned, resetting"
                            );
                            self.session_id = uuid::Uuid::new_v4().to_string();
                            self.is_new_session = true;

                            let resp = fold::MessageResponse {
                                request_id: request_id.clone(),
                                event: Some(fold::message_response::Event::Error(
                                    "Session lost, will retry with new session".to_string(),
                                )),
                            };
                            if tx.send(resp).await.is_err() {
                                tracing::warn!("Failed to send response - channel closed");
                                return Ok(());
                            }
                        }
                        BackendEvent::ToolApprovalRequest { id, name, input } => {
                            // Swarm agents are autonomous - auto-approve all tools
                            tracing::debug!(
                                tool_id = %id,
                                tool_name = %name,
                                "Auto-approving tool (swarm agent is autonomous)"
                            );
                            // The approval is handled internally by the backend;
                            // we just emit a ToolUse event for visibility
                            let resp = fold::MessageResponse {
                                request_id: request_id.clone(),
                                event: Some(fold::message_response::Event::ToolUse(
                                    fold::ToolUse {
                                        id,
                                        name,
                                        input_json: input.to_string(),
                                    },
                                )),
                            };
                            if tx.send(resp).await.is_err() {
                                tracing::warn!("Failed to send response - channel closed");
                                return Ok(());
                            }
                        }
                        BackendEvent::Text(text) => {
                            // Accumulate text for debouncing
                            text_buffer.push_str(&text);
                            accumulated_text.push_str(&text);

                            // Send if buffer is large enough or debounce interval elapsed
                            let should_send = text_buffer.len() >= TEXT_BUFFER_MAX
                                || last_text_send.elapsed() >= debounce_interval;

                            if should_send && !text_buffer.is_empty() {
                                let resp = fold::MessageResponse {
                                    request_id: request_id.clone(),
                                    event: Some(fold::message_response::Event::Text(
                                        std::mem::take(&mut text_buffer),
                                    )),
                                };
                                if tx.send(resp).await.is_err() {
                                    tracing::warn!("Failed to send response - channel closed");
                                    return Ok(());
                                }
                                last_text_send = Instant::now();
                            }
                        }
                        BackendEvent::ToolUse { id, name, input } => {
                            // Flush any pending text before tool events
                            if !text_buffer.is_empty() {
                                let resp = fold::MessageResponse {
                                    request_id: request_id.clone(),
                                    event: Some(fold::message_response::Event::Text(
                                        std::mem::take(&mut text_buffer),
                                    )),
                                };
                                let _ = tx.send(resp).await;
                                last_text_send = Instant::now();
                            }

                            let resp = fold::MessageResponse {
                                request_id: request_id.clone(),
                                event: Some(fold::message_response::Event::ToolUse(
                                    fold::ToolUse {
                                        id,
                                        name,
                                        input_json: input.to_string(),
                                    },
                                )),
                            };
                            if tx.send(resp).await.is_err() {
                                tracing::warn!("Failed to send response - channel closed");
                                return Ok(());
                            }
                        }
                        BackendEvent::ToolResult { id, output, is_error } => {
                            let resp = fold::MessageResponse {
                                request_id: request_id.clone(),
                                event: Some(fold::message_response::Event::ToolResult(
                                    fold::ToolResult {
                                        id,
                                        output,
                                        is_error,
                                    },
                                )),
                            };
                            if tx.send(resp).await.is_err() {
                                tracing::warn!("Failed to send response - channel closed");
                                return Ok(());
                            }
                        }
                        BackendEvent::Done { full_response } => {
                            // Flush any pending text before Done
                            if !text_buffer.is_empty() {
                                let resp = fold::MessageResponse {
                                    request_id: request_id.clone(),
                                    event: Some(fold::message_response::Event::Text(
                                        std::mem::take(&mut text_buffer),
                                    )),
                                };
                                let _ = tx.send(resp).await;
                            }

                            // Mark session as established after successful completion
                            self.is_new_session = false;

                            sent_done = true;
                            // Use accumulated_text if backend didn't provide full_response
                            let response_text = if full_response.is_empty() {
                                accumulated_text.clone()
                            } else {
                                full_response
                            };
                            let resp = fold::MessageResponse {
                                request_id: request_id.clone(),
                                event: Some(fold::message_response::Event::Done(fold::Done {
                                    full_response: response_text,
                                })),
                            };
                            if tx.send(resp).await.is_err() {
                                tracing::warn!("Failed to send response - channel closed");
                                return Ok(());
                            }
                        }
                        BackendEvent::Error(message) => {
                            // Flush any pending text before error
                            if !text_buffer.is_empty() {
                                let resp = fold::MessageResponse {
                                    request_id: request_id.clone(),
                                    event: Some(fold::message_response::Event::Text(
                                        std::mem::take(&mut text_buffer),
                                    )),
                                };
                                let _ = tx.send(resp).await;
                            }

                            let resp = fold::MessageResponse {
                                request_id: request_id.clone(),
                                event: Some(fold::message_response::Event::Error(message)),
                            };
                            if tx.send(resp).await.is_err() {
                                tracing::warn!("Failed to send response - channel closed");
                                return Ok(());
                            }
                        }
                        BackendEvent::Thinking => {
                            tracing::debug!("Backend is thinking");
                        }
                        BackendEvent::ToolState { id, state, .. } => {
                            tracing::debug!(tool_id = %id, ?state, "Tool state change");
                        }
                        BackendEvent::Usage { input_tokens, output_tokens, .. } => {
                            tracing::debug!(input_tokens, output_tokens, "Token usage");
                        }
                    }
                }

                // Flush any remaining text in buffer
                if !text_buffer.is_empty() {
                    let resp = fold::MessageResponse {
                        request_id: request_id.clone(),
                        event: Some(fold::message_response::Event::Text(text_buffer)),
                    };
                    let _ = tx.send(resp).await;
                }
            }
            Err(e) => {
                let _ = tx.send(fold::MessageResponse {
                    request_id: request_id.clone(),
                    event: Some(fold::message_response::Event::Error(e.to_string())),
                }).await;
                return Ok(());
            }
        }

        // Ensure we send Done if not already sent
        if !sent_done {
            let _ = tx.send(fold::MessageResponse {
                request_id,
                event: Some(fold::message_response::Event::Done(fold::Done {
                    full_response: accumulated_text,
                })),
            }).await;
        }

        Ok(())
    }
}
