// ABOUTME: Direct CLI backend - spawns claude with --output-format stream-json
// ABOUTME: Parses streaming JSONL from stdout, emits BackendEvents

use super::{Backend, BackendEvent};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::stream::BoxStream;
use serde_json::Value;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command as ProcessCommand};
use tokio::sync::mpsc;

/// Configuration for the Direct CLI backend
#[derive(Debug, Clone)]
pub struct DirectCliConfig {
    /// Path to the claude binary (defaults to "claude")
    pub binary: String,
    /// Working directory for the agent
    pub working_dir: PathBuf,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Gateway MCP endpoint URL for pack tools (e.g., "http://localhost:8080/mcp?token=xxx")
    /// Registered via `claude mcp add` so the CLI subprocess discovers it via Streamable HTTP
    pub mcp_endpoint: Option<String>,
}

impl Default for DirectCliConfig {
    fn default() -> Self {
        Self {
            binary: "claude".to_string(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            timeout_secs: 300,
            mcp_endpoint: None,
        }
    }
}

pub struct DirectCliBackend {
    config: DirectCliConfig,
    /// MCP endpoint can be set after construction (when token is received from gateway)
    mcp_endpoint_override: std::sync::RwLock<Option<String>>,
}

impl DirectCliBackend {
    pub fn new(config: DirectCliConfig) -> Self {
        Self {
            config,
            mcp_endpoint_override: std::sync::RwLock::new(None),
        }
    }

    /// Set the MCP endpoint URL (typically called after receiving mcp_token from gateway)
    pub fn set_mcp_endpoint(&self, endpoint: String) {
        if let Ok(mut guard) = self.mcp_endpoint_override.write() {
            *guard = Some(endpoint);
        }
    }

    /// Get the effective MCP endpoint (override takes precedence over config)
    fn effective_mcp_endpoint(&self) -> Option<String> {
        if let Ok(guard) = self.mcp_endpoint_override.read() {
            if let Some(ref endpoint) = *guard {
                return Some(endpoint.clone());
            }
        }
        self.config.mcp_endpoint.clone()
    }
}

#[async_trait]
impl Backend for DirectCliBackend {
    fn name(&self) -> &'static str {
        "direct-cli"
    }

    async fn send(
        &self,
        session_id: &str,
        message: &str,
        is_new_session: bool,
    ) -> Result<BoxStream<'static, BackendEvent>> {
        let config = self.config.clone();
        let mcp_endpoint = self.effective_mcp_endpoint();
        let session_id = session_id.to_string();
        let message = message.to_string();

        let (tx, rx) = mpsc::channel::<BackendEvent>(100);
        let timeout_duration = std::time::Duration::from_secs(config.timeout_secs);

        tokio::spawn(async move {
            // Spawn the child process first so we have a handle to kill on timeout
            let child_result = spawn_cli_process(
                &config,
                &session_id,
                &message,
                is_new_session,
                mcp_endpoint.as_deref(),
            )
            .await;

            let mut child = match child_result {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to spawn CLI process");
                    let _ = tx.send(BackendEvent::Error(e.to_string())).await;
                    let _ = tx
                        .send(BackendEvent::Done {
                            full_response: String::new(),
                        })
                        .await;
                    return;
                }
            };

            // Send thinking indicator
            let _ = tx.send(BackendEvent::Thinking).await;

            // Run the prompt processing with timeout
            let result =
                tokio::time::timeout(timeout_duration, process_cli_output(&mut child, tx.clone()))
                    .await;

            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::error!(error = %e, "Direct CLI prompt failed");
                    let _ = tx.send(BackendEvent::Error(e.to_string())).await;
                    let _ = tx
                        .send(BackendEvent::Done {
                            full_response: String::new(),
                        })
                        .await;
                }
                Err(_) => {
                    tracing::error!(timeout_secs = config.timeout_secs, "Direct CLI timed out");
                    // Kill the child process on timeout
                    if let Err(e) = child.kill().await {
                        tracing::warn!(error = %e, "Failed to kill timed-out CLI process");
                    }
                    let _ = tx
                        .send(BackendEvent::Error(format!(
                            "Request timed out after {} seconds",
                            config.timeout_secs
                        )))
                        .await;
                    let _ = tx
                        .send(BackendEvent::Done {
                            full_response: String::new(),
                        })
                        .await;
                }
            }
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }
}

/// Spawn the Claude CLI process with appropriate arguments.
///
/// When an MCP endpoint is provided (gateway pack tools), it's passed via --mcp-config
/// which adds the server while preserving default MCP discovery from ~/.claude.
/// Note: --strict-mcp-config disables all other discovery and causes hangs in some
/// environments, so we use --mcp-config instead.
async fn spawn_cli_process(
    config: &DirectCliConfig,
    session_id: &str,
    text: &str,
    is_new_session: bool,
    mcp_endpoint: Option<&str>,
) -> Result<Child> {
    let mut args = vec![
        "--print".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
        "--dangerously-skip-permissions".to_string(),
    ];

    // Add gateway MCP server if endpoint provided (enables pack tools like log_entry, todo_*, bbs_*)
    // Uses --strict-mcp-config to ONLY use the gateway's MCP server and avoid hangs from other MCP servers
    // Note: Must include "type": "http" for Claude CLI to recognize HTTP transport
    if let Some(endpoint) = mcp_endpoint {
        let mcp_config = serde_json::json!({
            "mcpServers": {
                "coven-gateway": {
                    "type": "http",
                    "url": endpoint
                }
            }
        });
        let mcp_config_str = mcp_config.to_string();
        tracing::info!(
            endpoint = %endpoint,
            "Adding gateway MCP server for pack tools"
        );
        args.push("--strict-mcp-config".to_string());
        args.push("--mcp-config".to_string());
        args.push(mcp_config_str);
    }

    // Only use --resume for existing sessions, not new ones
    if !is_new_session {
        args.push("--resume".to_string());
        args.push(session_id.to_string());
    }

    // When using MCP, pass message via stdin instead of argument to avoid hang
    // Claude CLI with MCP servers hangs when message is passed as argument with stdin=null
    let use_stdin = mcp_endpoint.is_some();

    if !use_stdin {
        args.push(text.to_string());
    }

    tracing::debug!(args = ?args, cwd = %config.working_dir.display(), use_stdin = use_stdin, "Spawning Claude CLI");

    let mut child = ProcessCommand::new(&config.binary)
        .args(&args)
        .current_dir(&config.working_dir)
        // Use piped stdin when MCP is enabled, otherwise null to prevent hangs
        .stdin(if use_stdin {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        })
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn Claude CLI")?;

    // Write message to stdin if using MCP
    if use_stdin {
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin
                .write_all(text.as_bytes())
                .await
                .context("Failed to write to stdin")?;
            // stdin is dropped here, closing it and signaling EOF to Claude CLI
        }
    }

    Ok(child)
}

/// Process output from a spawned Claude CLI process
async fn process_cli_output(child: &mut Child, event_tx: mpsc::Sender<BackendEvent>) -> Result<()> {
    let stdout = child.stdout.take().context("Failed to capture stdout")?;
    let stderr = child.stderr.take().context("Failed to capture stderr")?;

    // Track whether we detected a session orphan (to suppress duplicate errors)
    let orphan_detected = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let orphan_detected_stderr = orphan_detected.clone();

    // Spawn task to read stderr and detect errors
    // Uses an atomic flag instead of channel to signal orphan detection
    let stderr_handle = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if !line.is_empty() {
                // Only log non-error stderr lines at debug level to reduce noise
                if line.contains("No conversation found with session ID") {
                    tracing::warn!("Detected orphaned session - will clear stored session ID");
                    orphan_detected_stderr.store(true, std::sync::atomic::Ordering::SeqCst);
                } else {
                    tracing::debug!(stderr = %line, "Claude CLI stderr");
                }
            }
        }
    });

    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();
    let mut accumulated_text = String::new();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(&line) {
            Ok(json) => {
                if let Some(events) = parse_cli_event(&json, &mut accumulated_text) {
                    for event in events {
                        if event_tx.send(event).await.is_err() {
                            tracing::debug!("Event receiver closed, stopping stream");
                            return Ok(());
                        }
                    }
                }
            }
            Err(e) => {
                // Log parse failures to help debug unexpected CLI output
                let display_line = if line.chars().count() > 200 {
                    let truncated: String = line.chars().take(200).collect();
                    format!("{}...[truncated]", truncated)
                } else {
                    line.clone()
                };
                tracing::warn!(
                    error = %e,
                    line = %display_line,
                    "Failed to parse CLI output line as JSON"
                );
            }
        }
    }

    // LIMITATION: Orphan detection happens here, after stdout is fully consumed. This means
    // orphaned sessions won't be detected until the CLI process exits and closes stdout.
    // To detect orphans earlier, we would need to interleave stdout/stderr processing with
    // tokio::select!, but that adds complexity and the CLI typically fails fast on orphans.

    // Wait for stderr reader to complete
    if let Err(e) = stderr_handle.await {
        tracing::warn!(error = %e, "stderr reader task failed to complete");
    }

    // Check if session was orphaned - send this event INSTEAD of the exit status error
    if orphan_detected.load(std::sync::atomic::Ordering::SeqCst) {
        let _ = event_tx.send(BackendEvent::SessionOrphaned).await;
        // Send Done event so the stream completes properly
        let _ = event_tx
            .send(BackendEvent::Done {
                full_response: String::new(),
            })
            .await;
        // Don't check exit status - we know why it failed
        return Ok(());
    }

    let status = child.wait().await?;
    if !status.success() {
        let _ = event_tx
            .send(BackendEvent::Error(format!(
                "CLI exited with status: {:?}",
                status.code()
            )))
            .await;
        // Send Done event so the stream completes properly
        let _ = event_tx
            .send(BackendEvent::Done {
                full_response: String::new(),
            })
            .await;
    }

    Ok(())
}

fn parse_cli_event(json: &Value, accumulated_text: &mut String) -> Option<Vec<BackendEvent>> {
    let event_type = json.get("type")?.as_str()?;

    match event_type {
        "system" => {
            // Capture session_id from init event and emit SessionInit
            let subtype = json.get("subtype").and_then(|s| s.as_str());
            if subtype == Some("init") {
                if let Some(session_id) = json.get("session_id").and_then(|s| s.as_str()) {
                    tracing::debug!(session_id = %session_id, "Session initialized");
                    return Some(vec![BackendEvent::SessionInit {
                        session_id: session_id.to_string(),
                    }]);
                }
            } else if subtype == Some("compact_boundary") {
                // Claude CLI emits compact_boundary when conversation history is
                // compressed due to context window limits. Log this for operational
                // visibility in long-running agent sessions.
                let metadata = json.get("compact_metadata");
                let trigger = metadata
                    .and_then(|m| m.get("trigger"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown");
                let pre_tokens = metadata
                    .and_then(|m| m.get("pre_tokens"))
                    .and_then(|t| t.as_u64());
                if let Some(tokens) = pre_tokens {
                    tracing::info!(
                        trigger = %trigger,
                        pre_tokens = tokens,
                        "Conversation history compacted"
                    );
                } else {
                    tracing::info!(
                        trigger = %trigger,
                        "Conversation history compacted"
                    );
                }
            }
            None
        }
        "assistant" => {
            let mut events = Vec::new();

            if let Some(content) = json
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for item in content {
                    let item_type = item.get("type").and_then(|t| t.as_str());

                    if item_type == Some("tool_use") {
                        let name = item
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let id = item
                            .get("id")
                            .and_then(|i| i.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let input = item.get("input").cloned().unwrap_or(Value::Null);

                        tracing::debug!(tool = %name, id = %id, "Tool use detected");
                        events.push(BackendEvent::ToolUse { id, name, input });
                    } else if item_type == Some("thinking") {
                        // Extended thinking block - emit Thinking event but don't
                        // stream the content (it's internal model reasoning)
                        events.push(BackendEvent::Thinking);
                    } else if item_type == Some("text") {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                // Handle text accumulation with proper spacing
                                if !accumulated_text.is_empty() {
                                    let ends_with_ws =
                                        accumulated_text.ends_with(|c: char| c.is_whitespace());
                                    let starts_with_ws_or_punct = text.starts_with(|c: char| {
                                        c.is_whitespace() || c.is_ascii_punctuation()
                                    });
                                    if !ends_with_ws && !starts_with_ws_or_punct {
                                        accumulated_text.push(' ');
                                    }
                                }
                                accumulated_text.push_str(text);

                                // Emit text event for streaming display
                                events.push(BackendEvent::Text(text.to_string()));
                            }
                        }
                    }
                }
            }

            if events.is_empty() {
                None
            } else {
                Some(events)
            }
        }
        "result" => {
            let is_error = json
                .get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if is_error {
                // Extract error message: prefer "errors" (array) from Claude CLI,
                // fall back to "error" (string) for backwards compatibility
                let error_detail =
                    if let Some(errors_array) = json.get("errors").and_then(|v| v.as_array()) {
                        let messages: Vec<String> = errors_array
                            .iter()
                            .filter_map(|e| e.as_str().map(|s| s.to_string()))
                            .collect();
                        if messages.is_empty() {
                            "Unknown error".to_string()
                        } else {
                            messages.join("; ")
                        }
                    } else if let Some(error_str) = json.get("error").and_then(|v| v.as_str()) {
                        error_str.to_string()
                    } else {
                        "Unknown error".to_string()
                    };

                // Include the error subtype for context (e.g., "error_max_turns")
                let message = if let Some(subtype) = json.get("subtype").and_then(|v| v.as_str()) {
                    format!("{}: {}", subtype, error_detail)
                } else {
                    error_detail
                };

                Some(vec![BackendEvent::Error(message)])
            } else {
                let mut events = Vec::new();

                // Extract usage data if present
                if let Some(usage) = json.get("usage") {
                    let input_tokens = usage
                        .get("input_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;
                    let output_tokens = usage
                        .get("output_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;
                    let cache_read_tokens = usage
                        .get("cache_read_input_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;
                    let cache_write_tokens = usage
                        .get("cache_creation_input_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;

                    let thinking_tokens = usage
                        .get("thinking_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;

                    if input_tokens > 0 || output_tokens > 0 {
                        events.push(BackendEvent::Usage {
                            input_tokens,
                            output_tokens,
                            cache_read_tokens,
                            cache_write_tokens,
                            thinking_tokens,
                        });
                    }
                }

                // Use accumulated text from assistant messages
                let result_text = if !accumulated_text.is_empty() {
                    std::mem::take(accumulated_text)
                } else {
                    // Fallback to result field if present
                    json.get("result")
                        .and_then(|r| r.as_str())
                        .unwrap_or("")
                        .to_string()
                };

                events.push(BackendEvent::Done {
                    full_response: result_text,
                });

                Some(events)
            }
        }
        "user" => {
            // Tool results come back as user messages
            if let Some(content) = json
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                let mut events = Vec::new();
                for item in content {
                    let item_type = item.get("type").and_then(|t| t.as_str());
                    if item_type == Some("tool_result") {
                        let id = item
                            .get("tool_use_id")
                            .and_then(|i| i.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let output = item
                            .get("content")
                            .and_then(|c| c.as_str())
                            .unwrap_or("")
                            .to_string();
                        let is_error = item
                            .get("is_error")
                            .and_then(|e| e.as_bool())
                            .unwrap_or(false);

                        events.push(BackendEvent::ToolResult {
                            id,
                            output,
                            is_error,
                        });
                    }
                }
                if !events.is_empty() {
                    return Some(events);
                }
            }
            None
        }
        unknown => {
            let json_preview: String = {
                let full = json.to_string();
                if full.chars().count() > 200 {
                    let truncated: String = full.chars().take(200).collect();
                    format!("{}...[truncated]", truncated)
                } else {
                    full
                }
            };
            tracing::debug!(
                event_type = %unknown,
                json_preview = %json_preview,
                "Unhandled CLI event type"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── 1. Error result with "errors" array ──────────────────────────────

    #[test]
    fn error_result_with_single_errors_array_entry() {
        let json = json!({
            "type": "result",
            "subtype": "error_max_turns",
            "is_error": true,
            "errors": ["Maximum turns reached"]
        });
        let mut acc = String::new();
        let events = parse_cli_event(&json, &mut acc).expect("should return events");
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::Error(msg) => {
                assert_eq!(msg, "error_max_turns: Maximum turns reached");
            }
            other => panic!("expected BackendEvent::Error, got {:?}", other),
        }
    }

    #[test]
    fn error_result_with_multiple_errors_array_entries() {
        let json = json!({
            "type": "result",
            "subtype": "error_max_budget_usd",
            "is_error": true,
            "errors": ["Budget exceeded", "Spending limit hit"]
        });
        let mut acc = String::new();
        let events = parse_cli_event(&json, &mut acc).expect("should return events");
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::Error(msg) => {
                assert_eq!(
                    msg,
                    "error_max_budget_usd: Budget exceeded; Spending limit hit"
                );
            }
            other => panic!("expected BackendEvent::Error, got {:?}", other),
        }
    }

    #[test]
    fn error_result_fallback_to_error_string_field() {
        let json = json!({
            "type": "result",
            "is_error": true,
            "error": "Something broke"
        });
        let mut acc = String::new();
        let events = parse_cli_event(&json, &mut acc).expect("should return events");
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::Error(msg) => {
                assert_eq!(msg, "Something broke");
            }
            other => panic!("expected BackendEvent::Error, got {:?}", other),
        }
    }

    #[test]
    fn error_result_fallback_to_unknown_error() {
        let json = json!({
            "type": "result",
            "is_error": true
        });
        let mut acc = String::new();
        let events = parse_cli_event(&json, &mut acc).expect("should return events");
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::Error(msg) => {
                assert_eq!(msg, "Unknown error");
            }
            other => panic!("expected BackendEvent::Error, got {:?}", other),
        }
    }

    // ── 2. Thinking content blocks in assistant messages ─────────────────

    #[test]
    fn assistant_message_with_thinking_and_text() {
        let json = json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "thinking", "thinking": "Let me analyze..."},
                    {"type": "text", "text": "Here is my answer"}
                ]
            }
        });
        let mut acc = String::new();
        let events = parse_cli_event(&json, &mut acc).expect("should return events");
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], BackendEvent::Thinking),
            "first event should be Thinking, got {:?}",
            events[0]
        );
        match &events[1] {
            BackendEvent::Text(text) => {
                assert_eq!(text, "Here is my answer");
            }
            other => panic!("expected BackendEvent::Text, got {:?}", other),
        }
    }

    #[test]
    fn assistant_message_with_only_thinking_block() {
        let json = json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "thinking", "thinking": "Internal reasoning only"}
                ]
            }
        });
        let mut acc = String::new();
        let events = parse_cli_event(&json, &mut acc).expect("should return events");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], BackendEvent::Thinking));
    }

    // ── 3. Thinking tokens in usage ──────────────────────────────────────

    #[test]
    fn result_with_usage_including_thinking_tokens() {
        let json = json!({
            "type": "result",
            "is_error": false,
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_read_input_tokens": 10,
                "cache_creation_input_tokens": 5,
                "thinking_tokens": 200
            }
        });
        let mut acc = String::new();
        let events = parse_cli_event(&json, &mut acc).expect("should return events");

        // Should contain Usage event and Done event
        let usage_event = events
            .iter()
            .find(|e| matches!(e, BackendEvent::Usage { .. }))
            .expect("should contain a Usage event");
        match usage_event {
            BackendEvent::Usage {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                thinking_tokens,
            } => {
                assert_eq!(*input_tokens, 100);
                assert_eq!(*output_tokens, 50);
                assert_eq!(*cache_read_tokens, 10);
                assert_eq!(*cache_write_tokens, 5);
                assert_eq!(*thinking_tokens, 200);
            }
            _ => unreachable!(),
        }

        // Should also contain Done event
        let done_event = events
            .iter()
            .find(|e| matches!(e, BackendEvent::Done { .. }))
            .expect("should contain a Done event");
        assert!(
            matches!(done_event, BackendEvent::Done { full_response } if full_response.is_empty())
        );
    }

    #[test]
    fn result_with_usage_zero_thinking_tokens() {
        let json = json!({
            "type": "result",
            "is_error": false,
            "usage": {
                "input_tokens": 50,
                "output_tokens": 25,
                "cache_read_input_tokens": 0,
                "cache_creation_input_tokens": 0
            }
        });
        let mut acc = String::new();
        let events = parse_cli_event(&json, &mut acc).expect("should return events");

        let usage_event = events
            .iter()
            .find(|e| matches!(e, BackendEvent::Usage { .. }))
            .expect("should contain a Usage event");
        match usage_event {
            BackendEvent::Usage {
                thinking_tokens, ..
            } => {
                assert_eq!(
                    *thinking_tokens, 0,
                    "missing thinking_tokens should default to 0"
                );
            }
            _ => unreachable!(),
        }
    }

    // ── 4. compact_boundary system event returns None ─────────────────────

    #[test]
    fn system_compact_boundary_returns_none() {
        let json = json!({
            "type": "system",
            "subtype": "compact_boundary",
            "compact_metadata": {
                "trigger": "auto",
                "pre_tokens": 12345
            }
        });
        let mut acc = String::new();
        let result = parse_cli_event(&json, &mut acc);
        assert!(
            result.is_none(),
            "compact_boundary should return None, got {:?}",
            result
        );
    }

    #[test]
    fn system_compact_boundary_without_pre_tokens_returns_none() {
        let json = json!({
            "type": "system",
            "subtype": "compact_boundary",
            "compact_metadata": {
                "trigger": "manual"
            }
        });
        let mut acc = String::new();
        let result = parse_cli_event(&json, &mut acc);
        assert!(result.is_none());
    }

    // ── 5. Unknown event type returns None ───────────────────────────────

    #[test]
    fn unknown_event_type_returns_none() {
        let json = json!({
            "type": "stream_event",
            "event": {"type": "message_start"}
        });
        let mut acc = String::new();
        let result = parse_cli_event(&json, &mut acc);
        assert!(
            result.is_none(),
            "unknown event type should return None, got {:?}",
            result
        );
    }

    #[test]
    fn completely_unknown_event_type_returns_none() {
        let json = json!({
            "type": "banana_event",
            "data": 42
        });
        let mut acc = String::new();
        let result = parse_cli_event(&json, &mut acc);
        assert!(result.is_none());
    }

    #[test]
    fn event_with_no_type_field_returns_none() {
        let json = json!({
            "data": "something",
            "value": 123
        });
        let mut acc = String::new();
        let result = parse_cli_event(&json, &mut acc);
        assert!(result.is_none());
    }

    // ── Bonus: accumulated text flows into result Done ───────────────────

    #[test]
    fn accumulated_text_appears_in_done_event() {
        // Simulate an assistant message followed by a result
        let assistant_json = json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "text", "text": "Hello world"}
                ]
            }
        });
        let result_json = json!({
            "type": "result",
            "is_error": false
        });
        let mut acc = String::new();

        // Process assistant message - accumulates text
        let _events = parse_cli_event(&assistant_json, &mut acc);
        assert_eq!(acc, "Hello world");

        // Process result - drains accumulated text into Done
        let events = parse_cli_event(&result_json, &mut acc).expect("should return events");
        let done_event = events
            .iter()
            .find(|e| matches!(e, BackendEvent::Done { .. }))
            .expect("should contain Done event");
        match done_event {
            BackendEvent::Done { full_response } => {
                assert_eq!(full_response, "Hello world");
            }
            _ => unreachable!(),
        }
        // Accumulated text should be drained
        assert!(
            acc.is_empty(),
            "accumulated text should be empty after result"
        );
    }
}
