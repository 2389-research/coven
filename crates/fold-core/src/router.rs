// ABOUTME: The Fold router - maps incoming messages to threads and streams responses
// ABOUTME: Core orchestration layer between frontends and backends

use crate::backend::{Backend, BackendEvent, ToolStateKind};
use crate::config::Config as FoldConfig;
use crate::store::ThreadStore;
use crate::types::{IncomingMessage, OutgoingEvent};
use anyhow::Result;
use futures::StreamExt;
use futures::stream::BoxStream;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Convert ToolStateKind to string representation
fn tool_state_to_string(state: ToolStateKind) -> String {
    match state {
        ToolStateKind::Pending => "pending",
        ToolStateKind::AwaitingApproval => "awaiting_approval",
        ToolStateKind::Running => "running",
        ToolStateKind::Completed => "completed",
        ToolStateKind::Failed => "failed",
        ToolStateKind::Denied => "denied",
        ToolStateKind::Timeout => "timeout",
        ToolStateKind::Cancelled => "cancelled",
    }
    .to_string()
}

/// Rewrite message content to include file attachment paths for Claude
fn rewrite_with_attachments(msg: &IncomingMessage) -> String {
    if msg.attachments.is_empty() {
        return msg.content.clone();
    }

    let file_refs: Vec<String> = msg
        .attachments
        .iter()
        .map(|f| format!("  - {} ({}): {}", f.filename, f.mime_type, f.path.display()))
        .collect();

    if msg.content.is_empty() {
        format!("Attached files:\n{}", file_refs.join("\n"))
    } else {
        format!(
            "{}\n\nAttached files:\n{}",
            msg.content,
            file_refs.join("\n")
        )
    }
}

/// The core router that handles messages and manages sessions
pub struct Fold {
    threads: Arc<ThreadStore>,
    backend: Arc<dyn Backend>,
    /// Cache of active session IDs
    sessions: Arc<RwLock<std::collections::HashMap<String, String>>>,
}

impl Fold {
    /// Create a new Fold router from config
    pub async fn new(config: &FoldConfig, backend: Arc<dyn Backend>) -> Result<Self> {
        let db_path = config.db_path();

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let threads = Arc::new(ThreadStore::open(&db_path).await?);

        Ok(Self {
            threads,
            backend,
            sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        })
    }

    /// Handle an incoming message and return a stream of response events
    pub async fn handle(&self, msg: IncomingMessage) -> Result<BoxStream<'static, OutgoingEvent>> {
        // Get or create the thread (keeping the thread for session ID lookup)
        let (thread, _is_new_thread) = self.threads.get_or_create(&msg.thread_id).await?;

        // Get or create session ID (use write lock to avoid TOCTOU race)
        // Also track whether this is a new session for the backend
        let (session_id, is_new_session) = {
            let mut sessions = self.sessions.write().await;
            if let Some(sid) = sessions.get(&msg.thread_id) {
                (sid.clone(), false)
            } else {
                // Use the thread we already have instead of re-fetching
                let (session_id, is_new) = if thread.claude_session_id.is_empty() {
                    let new_id = Uuid::new_v4().to_string();
                    self.threads.set_session_id(&msg.thread_id, &new_id).await?;
                    (new_id, true)
                } else {
                    (thread.claude_session_id.clone(), false)
                };

                sessions.insert(msg.thread_id.clone(), session_id.clone());
                (session_id, is_new)
            }
        };

        // Update last active
        self.threads.touch(&msg.thread_id).await?;

        // Rewrite message content to include file paths for Claude
        let message_for_claude = rewrite_with_attachments(&msg);

        // Store user message (with attachments info)
        if let Err(e) = self
            .threads
            .add_message(&msg.thread_id, "user", &message_for_claude)
            .await
        {
            tracing::warn!(error = %e, "Failed to store user message");
        }

        // Send to backend
        let backend_stream = self
            .backend
            .send(&session_id, &message_for_claude, is_new_session)
            .await?;

        // Clone for the async stream
        let threads = self.threads.clone();
        let sessions = self.sessions.clone();
        let thread_id = msg.thread_id.clone();

        // Map BackendEvent to OutgoingEvent and log events
        let mapped = backend_stream.then(move |event| {
            let threads = threads.clone();
            let sessions = sessions.clone();
            let thread_id = thread_id.clone();
            async move {
                // Log the event
                let (event_type, event_data) = match &event {
                    BackendEvent::Thinking => ("thinking", serde_json::json!({})),
                    BackendEvent::SessionInit { session_id } => {
                        // Update the stored session ID to match backend's actual session
                        if let Err(e) = threads.set_session_id(&thread_id, session_id).await {
                            tracing::warn!(error = %e, "Failed to update session ID");
                        }
                        // Update the cache too
                        sessions.write().await.insert(thread_id.clone(), session_id.clone());
                        ("session_init", serde_json::json!({"session_id": session_id}))
                    }
                    BackendEvent::SessionOrphaned => {
                        // Clear stored session ID so next message starts fresh
                        if let Err(e) = threads.set_session_id(&thread_id, "").await {
                            tracing::warn!(error = %e, "Failed to clear orphaned session ID");
                        }
                        // Clear from cache too
                        sessions.write().await.remove(&thread_id);
                        tracing::warn!(thread_id = %thread_id, "Cleared orphaned session - retry the message");
                        ("session_orphaned", serde_json::json!({}))
                    }
                    BackendEvent::Text(t) => ("text", serde_json::json!({"content": t})),
                    BackendEvent::ToolUse { id, name, input } => {
                        ("tool_use", serde_json::json!({"id": id, "name": name, "input": input}))
                    }
                    BackendEvent::ToolResult { id, output, is_error } => {
                        ("tool_result", serde_json::json!({"id": id, "output": output, "is_error": is_error}))
                    }
                    BackendEvent::ToolApprovalRequest { id, name, input } => {
                        ("tool_approval_request", serde_json::json!({"id": id, "name": name, "input": input}))
                    }
                    BackendEvent::Usage {
                        input_tokens,
                        output_tokens,
                        cache_read_tokens,
                        cache_write_tokens,
                        thinking_tokens,
                    } => (
                        "usage",
                        serde_json::json!({
                            "input_tokens": input_tokens,
                            "output_tokens": output_tokens,
                            "cache_read_tokens": cache_read_tokens,
                            "cache_write_tokens": cache_write_tokens,
                            "thinking_tokens": thinking_tokens,
                        }),
                    ),
                    BackendEvent::ToolState { id, state, detail } => (
                        "tool_state",
                        serde_json::json!({
                            "id": id,
                            "state": tool_state_to_string(*state),
                            "detail": detail,
                        }),
                    ),
                    BackendEvent::Done { full_response } => {
                        // Store assistant response (skip empty to avoid polluting history)
                        if !full_response.is_empty() {
                            if let Err(e) = threads
                                .add_message(&thread_id, "assistant", full_response)
                                .await
                            {
                                tracing::warn!(error = %e, "Failed to store assistant message");
                            }
                        }
                        ("done", serde_json::json!({"length": full_response.len()}))
                    }
                    BackendEvent::Error(e) => ("error", serde_json::json!({"message": e})),
                };

                // Log to database
                if let Err(e) = threads.add_event(&thread_id, event_type, &event_data).await {
                    tracing::warn!(error = %e, event_type = %event_type, "Failed to log event");
                }

                // Convert to OutgoingEvent
                match event {
                    BackendEvent::Thinking => OutgoingEvent::Thinking,
                    BackendEvent::SessionInit { session_id } => OutgoingEvent::SessionInit { session_id },
                    BackendEvent::SessionOrphaned => OutgoingEvent::SessionOrphaned,
                    BackendEvent::Text(t) => OutgoingEvent::Text(t),
                    BackendEvent::ToolUse { id, name, input } => {
                        OutgoingEvent::ToolUse { id, name, input }
                    }
                    BackendEvent::ToolResult { id, output, is_error } => {
                        OutgoingEvent::ToolResult { id, output, is_error }
                    }
                    BackendEvent::ToolApprovalRequest { id, name, input } => {
                        OutgoingEvent::ToolApprovalRequest { id, name, input }
                    }
                    BackendEvent::Usage {
                        input_tokens,
                        output_tokens,
                        cache_read_tokens,
                        cache_write_tokens,
                        thinking_tokens,
                    } => OutgoingEvent::Usage {
                        input_tokens,
                        output_tokens,
                        cache_read_tokens,
                        cache_write_tokens,
                        thinking_tokens,
                    },
                    BackendEvent::ToolState { id, state, detail } => OutgoingEvent::ToolState {
                        id,
                        state: tool_state_to_string(state),
                        detail,
                    },
                    BackendEvent::Done { full_response } => OutgoingEvent::Done { full_response },
                    BackendEvent::Error(e) => OutgoingEvent::Error(e),
                }
            }
        });

        Ok(Box::pin(mapped))
    }

    /// List all threads
    pub async fn list_threads(&self) -> Result<Vec<crate::types::Thread>> {
        self.threads.list().await
    }

    /// Delete a thread
    pub async fn delete_thread(&self, thread_id: &str) -> Result<()> {
        // Remove from cache
        let mut sessions = self.sessions.write().await;
        sessions.remove(thread_id);
        drop(sessions);

        // Remove from store
        self.threads.delete(thread_id).await
    }
}
