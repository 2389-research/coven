// ABOUTME: Codex CLI backend - spawns codex exec --json as subprocess
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

/// Configuration for the Codex CLI backend
#[derive(Debug, Clone)]
pub struct CodexCliConfig {
    /// Path to the codex binary (defaults to "codex")
    pub binary: String,
    /// Working directory for the agent
    pub working_dir: PathBuf,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Gateway MCP endpoint URL for pack tools
    pub mcp_endpoint: Option<String>,
}

impl Default for CodexCliConfig {
    fn default() -> Self {
        Self {
            binary: "codex".to_string(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            timeout_secs: 300,
            mcp_endpoint: None,
        }
    }
}

pub struct CodexCliBackend {
    config: CodexCliConfig,
    /// MCP endpoint can be set after construction (when token is received from gateway)
    mcp_endpoint_override: std::sync::RwLock<Option<String>>,
}

impl CodexCliBackend {
    pub fn new(config: CodexCliConfig) -> Self {
        Self {
            config,
            mcp_endpoint_override: std::sync::RwLock::new(None),
        }
    }

    /// Set the MCP endpoint URL (typically called after receiving mcp_token from gateway)
    pub fn set_mcp_endpoint(&self, endpoint: String) {
        match self.mcp_endpoint_override.write() {
            Ok(mut guard) => {
                *guard = Some(endpoint);
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to set MCP endpoint: RwLock poisoned");
            }
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
impl Backend for CodexCliBackend {
    fn name(&self) -> &'static str {
        "codex-cli"
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
            let child_result = spawn_codex_process(
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
                    tracing::error!(error = %e, "Failed to spawn Codex CLI process");
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
            let result = tokio::time::timeout(
                timeout_duration,
                process_codex_output(&mut child, tx.clone()),
            )
            .await;

            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::error!(error = %e, "Codex CLI prompt failed");
                    let _ = tx.send(BackendEvent::Error(e.to_string())).await;
                    let _ = tx
                        .send(BackendEvent::Done {
                            full_response: String::new(),
                        })
                        .await;
                }
                Err(_) => {
                    tracing::error!(timeout_secs = config.timeout_secs, "Codex CLI timed out");
                    // Kill the child process on timeout
                    if let Err(e) = child.kill().await {
                        tracing::warn!(error = %e, "Failed to kill timed-out Codex CLI process");
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

/// Spawn the Codex CLI process with appropriate arguments.
///
/// Codex uses `codex exec --json` for non-interactive streaming JSON output.
/// Session resume uses `codex exec resume <session_id> "message"`.
/// MCP servers are configured via `--config` flags.
async fn spawn_codex_process(
    config: &CodexCliConfig,
    session_id: &str,
    text: &str,
    is_new_session: bool,
    mcp_endpoint: Option<&str>,
) -> Result<Child> {
    let mut args = vec![
        "exec".to_string(),
        "--json".to_string(),
        "--sandbox".to_string(),
        "danger-full-access".to_string(),
    ];

    // Add gateway MCP server if endpoint provided (enables pack tools)
    if let Some(endpoint) = mcp_endpoint {
        tracing::info!(
            endpoint = %endpoint,
            "Adding gateway MCP server for pack tools (Codex)"
        );
        args.push("--config".to_string());
        args.push(format!("mcp_servers.coven-gateway.url={}", endpoint));
        args.push("--config".to_string());
        args.push("mcp_servers.coven-gateway.type=streamable-http".to_string());
    }

    // Session resume for existing sessions
    if !is_new_session {
        args.push("resume".to_string());
        args.push(session_id.to_string());
    }

    // Message is always the last positional argument
    args.push(text.to_string());

    tracing::debug!(args = ?args, cwd = %config.working_dir.display(), "Spawning Codex CLI");

    let child = ProcessCommand::new(&config.binary)
        .args(&args)
        .current_dir(&config.working_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn Codex CLI")?;

    Ok(child)
}

/// Process output from a spawned Codex CLI process
async fn process_codex_output(
    child: &mut Child,
    event_tx: mpsc::Sender<BackendEvent>,
) -> Result<()> {
    let stdout = child.stdout.take().context("Failed to capture stdout")?;
    let stderr = child.stderr.take().context("Failed to capture stderr")?;

    // Track whether we detected a session orphan
    let orphan_detected = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let orphan_detected_stderr = orphan_detected.clone();

    // Spawn task to read stderr and detect errors
    let stderr_handle = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if !line.is_empty() {
                if line.contains("session not found")
                    || line.contains("Session not found")
                    || line.contains("No conversation found")
                {
                    tracing::warn!(
                        "Detected orphaned session (Codex) - will clear stored session ID"
                    );
                    orphan_detected_stderr.store(true, std::sync::atomic::Ordering::SeqCst);
                } else {
                    tracing::debug!(stderr = %line, "Codex CLI stderr");
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
                if let Some(events) = parse_codex_event(&json, &mut accumulated_text) {
                    for event in events {
                        if event_tx.send(event).await.is_err() {
                            tracing::debug!("Event receiver closed, stopping stream");
                            return Ok(());
                        }
                    }
                }
            }
            Err(e) => {
                let display_line = if line.chars().count() > 200 {
                    let truncated: String = line.chars().take(200).collect();
                    format!("{}...[truncated]", truncated)
                } else {
                    line.clone()
                };
                tracing::warn!(
                    error = %e,
                    line = %display_line,
                    "Failed to parse Codex CLI output line as JSON"
                );
            }
        }
    }

    // Wait for stderr reader to complete
    if let Err(e) = stderr_handle.await {
        tracing::warn!(error = %e, "stderr reader task failed to complete");
    }

    // Check if session was orphaned
    if orphan_detected.load(std::sync::atomic::Ordering::SeqCst) {
        let _ = event_tx.send(BackendEvent::SessionOrphaned).await;
        let _ = event_tx
            .send(BackendEvent::Done {
                full_response: String::new(),
            })
            .await;
        return Ok(());
    }

    let status = child.wait().await?;
    if !status.success() {
        let _ = event_tx
            .send(BackendEvent::Error(format!(
                "Codex CLI exited with status: {:?}",
                status.code()
            )))
            .await;
        let _ = event_tx
            .send(BackendEvent::Done {
                full_response: String::new(),
            })
            .await;
    }

    Ok(())
}

/// Parse a Codex JSONL event into BackendEvent(s).
///
/// Codex CLI (with --json) emits events with a "type" field:
/// - "thread.started" -> SessionInit with session_id
/// - "item.completed" -> Text, ToolUse, or ToolResult depending on item type
/// - "turn.completed" -> Usage + Done
/// - "error" -> Error
fn parse_codex_event(json: &Value, accumulated_text: &mut String) -> Option<Vec<BackendEvent>> {
    let event_type = json.get("type")?.as_str()?;

    match event_type {
        "thread.started" => {
            // Extract session/thread ID from the event
            let session_id = json
                .get("thread_id")
                .or_else(|| json.get("session_id"))
                .or_else(|| json.get("id"))
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string();

            tracing::debug!(session_id = %session_id, "Codex session initialized");
            Some(vec![BackendEvent::SessionInit { session_id }])
        }
        "item.completed" => {
            let item = json.get("item").unwrap_or(json);
            let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match item_type {
                "message" | "agent_message" => {
                    // Extract text content from message
                    let mut events = Vec::new();
                    if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                        for block in content {
                            let block_type =
                                block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            if block_type == "text" || block_type == "output_text" {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    if !text.is_empty() {
                                        if !accumulated_text.is_empty() {
                                            let ends_with_ws = accumulated_text
                                                .ends_with(|c: char| c.is_whitespace());
                                            let starts_with_ws_or_punct =
                                                text.starts_with(|c: char| {
                                                    c.is_whitespace() || c.is_ascii_punctuation()
                                                });
                                            if !ends_with_ws && !starts_with_ws_or_punct {
                                                accumulated_text.push(' ');
                                            }
                                        }
                                        accumulated_text.push_str(text);
                                        events.push(BackendEvent::Text(text.to_string()));
                                    }
                                }
                            }
                        }
                    }
                    // Also check for top-level text field
                    if events.is_empty() {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                accumulated_text.push_str(text);
                                events.push(BackendEvent::Text(text.to_string()));
                            }
                        }
                    }
                    if events.is_empty() {
                        None
                    } else {
                        Some(events)
                    }
                }
                "function_call" | "tool_call" => {
                    let name = item
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let id = item
                        .get("id")
                        .or_else(|| item.get("call_id"))
                        .and_then(|i| i.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let input = item
                        .get("arguments")
                        .or_else(|| item.get("input"))
                        .cloned()
                        .unwrap_or(Value::Null);

                    // Parse arguments string to JSON if it's a string
                    let input = if let Value::String(ref s) = input {
                        serde_json::from_str(s).unwrap_or(input)
                    } else {
                        input
                    };

                    tracing::debug!(tool = %name, id = %id, "Codex tool use detected");
                    Some(vec![BackendEvent::ToolUse { id, name, input }])
                }
                "function_call_output" | "tool_result" => {
                    let id = item
                        .get("call_id")
                        .or_else(|| item.get("tool_use_id"))
                        .or_else(|| item.get("id"))
                        .and_then(|i| i.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let output = item
                        .get("output")
                        .or_else(|| item.get("content"))
                        .and_then(|o| {
                            if o.is_string() {
                                o.as_str().map(|s| s.to_string())
                            } else {
                                Some(o.to_string())
                            }
                        })
                        .unwrap_or_default();
                    let is_error = item
                        .get("is_error")
                        .and_then(|e| e.as_bool())
                        .unwrap_or_else(|| {
                            // Codex uses status field: "error" means is_error=true
                            item.get("status")
                                .and_then(|s| s.as_str())
                                .map(|s| s == "error")
                                .unwrap_or(false)
                        });

                    Some(vec![BackendEvent::ToolResult {
                        id,
                        output,
                        is_error,
                    }])
                }
                _ => {
                    tracing::debug!(item_type = %item_type, "Unhandled Codex item type");
                    None
                }
            }
        }
        "turn.completed" => {
            let mut events = Vec::new();

            // Extract usage data if present
            if let Some(usage) = json.get("usage") {
                let input_tokens = usage
                    .get("input_tokens")
                    .or_else(|| usage.get("prompt_tokens"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                let output_tokens = usage
                    .get("output_tokens")
                    .or_else(|| usage.get("completion_tokens"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;

                if input_tokens > 0 || output_tokens > 0 {
                    events.push(BackendEvent::Usage {
                        input_tokens,
                        output_tokens,
                        cache_read_tokens: 0,
                        cache_write_tokens: 0,
                        thinking_tokens: 0,
                    });
                }
            }

            // Use accumulated text from message events
            let result_text = if !accumulated_text.is_empty() {
                std::mem::take(accumulated_text)
            } else {
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
        "error" => {
            let message = json
                .get("message")
                .or_else(|| json.get("error"))
                .and_then(|e| {
                    if e.is_string() {
                        e.as_str().map(|s| s.to_string())
                    } else {
                        Some(e.to_string())
                    }
                })
                .unwrap_or_else(|| "Unknown Codex error".to_string());

            Some(vec![BackendEvent::Error(message)])
        }
        _ => {
            tracing::debug!(event_type = %event_type, "Unhandled Codex event type");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codex_cli_config_default() {
        let config = CodexCliConfig::default();
        assert_eq!(config.binary, "codex");
        assert_eq!(config.timeout_secs, 300);
        assert!(config.mcp_endpoint.is_none());
    }

    #[test]
    fn test_parse_thread_started() {
        let json: Value = serde_json::json!({
            "type": "thread.started",
            "thread_id": "thread-abc-123"
        });
        let mut text = String::new();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::SessionInit { session_id } => {
                assert_eq!(session_id, "thread-abc-123");
            }
            other => panic!("Expected SessionInit, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_thread_started_with_id_field() {
        let json: Value = serde_json::json!({
            "type": "thread.started",
            "id": "sess-xyz-789"
        });
        let mut text = String::new();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::SessionInit { session_id } => {
                assert_eq!(session_id, "sess-xyz-789");
            }
            other => panic!("Expected SessionInit, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_item_completed_message() {
        let json: Value = serde_json::json!({
            "type": "item.completed",
            "item": {
                "type": "message",
                "content": [
                    {
                        "type": "text",
                        "text": "Hello, world!"
                    }
                ]
            }
        });
        let mut text = String::new();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::Text(t) => assert_eq!(t, "Hello, world!"),
            other => panic!("Expected Text, got {:?}", other),
        }
        assert_eq!(text, "Hello, world!");
    }

    #[test]
    fn test_parse_item_completed_message_output_text() {
        let json: Value = serde_json::json!({
            "type": "item.completed",
            "item": {
                "type": "message",
                "content": [
                    {
                        "type": "output_text",
                        "text": "Response text here"
                    }
                ]
            }
        });
        let mut text = String::new();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::Text(t) => assert_eq!(t, "Response text here"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_item_completed_tool_call() {
        let json: Value = serde_json::json!({
            "type": "item.completed",
            "item": {
                "type": "function_call",
                "name": "read_file",
                "id": "call-001",
                "arguments": "{\"path\": \"/tmp/test.txt\"}"
            }
        });
        let mut text = String::new();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::ToolUse { id, name, input } => {
                assert_eq!(id, "call-001");
                assert_eq!(name, "read_file");
                assert_eq!(input["path"], "/tmp/test.txt");
            }
            other => panic!("Expected ToolUse, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_item_completed_tool_call_with_call_id() {
        let json: Value = serde_json::json!({
            "type": "item.completed",
            "item": {
                "type": "tool_call",
                "name": "write_file",
                "call_id": "call-002",
                "input": {"path": "/tmp/out.txt", "content": "hello"}
            }
        });
        let mut text = String::new();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::ToolUse { id, name, input } => {
                assert_eq!(id, "call-002");
                assert_eq!(name, "write_file");
                assert_eq!(input["path"], "/tmp/out.txt");
            }
            other => panic!("Expected ToolUse, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_item_completed_tool_result() {
        let json: Value = serde_json::json!({
            "type": "item.completed",
            "item": {
                "type": "function_call_output",
                "call_id": "call-001",
                "output": "file contents here"
            }
        });
        let mut text = String::new();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::ToolResult {
                id,
                output,
                is_error,
            } => {
                assert_eq!(id, "call-001");
                assert_eq!(output, "file contents here");
                assert!(!is_error);
            }
            other => panic!("Expected ToolResult, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_item_completed_tool_result_error() {
        let json: Value = serde_json::json!({
            "type": "item.completed",
            "item": {
                "type": "function_call_output",
                "call_id": "call-003",
                "output": "file not found",
                "is_error": true
            }
        });
        let mut text = String::new();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::ToolResult {
                id,
                output,
                is_error,
            } => {
                assert_eq!(id, "call-003");
                assert_eq!(output, "file not found");
                assert!(is_error);
            }
            other => panic!("Expected ToolResult, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_turn_completed_with_usage() {
        let json: Value = serde_json::json!({
            "type": "turn.completed",
            "usage": {
                "input_tokens": 150,
                "output_tokens": 75
            }
        });
        let mut text = "accumulated response".to_string();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 2);
        match &events[0] {
            BackendEvent::Usage {
                input_tokens,
                output_tokens,
                ..
            } => {
                assert_eq!(*input_tokens, 150);
                assert_eq!(*output_tokens, 75);
            }
            other => panic!("Expected Usage, got {:?}", other),
        }
        match &events[1] {
            BackendEvent::Done { full_response } => {
                assert_eq!(full_response, "accumulated response");
            }
            other => panic!("Expected Done, got {:?}", other),
        }
        // accumulated_text should be cleared
        assert!(text.is_empty());
    }

    #[test]
    fn test_parse_turn_completed_without_usage() {
        let json: Value = serde_json::json!({
            "type": "turn.completed"
        });
        let mut text = "the response".to_string();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::Done { full_response } => {
                assert_eq!(full_response, "the response");
            }
            other => panic!("Expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_turn_completed_with_prompt_tokens() {
        let json: Value = serde_json::json!({
            "type": "turn.completed",
            "usage": {
                "prompt_tokens": 200,
                "completion_tokens": 100
            }
        });
        let mut text = String::new();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 2);
        match &events[0] {
            BackendEvent::Usage {
                input_tokens,
                output_tokens,
                ..
            } => {
                assert_eq!(*input_tokens, 200);
                assert_eq!(*output_tokens, 100);
            }
            other => panic!("Expected Usage, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_error() {
        let json: Value = serde_json::json!({
            "type": "error",
            "message": "Rate limit exceeded"
        });
        let mut text = String::new();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::Error(msg) => {
                assert_eq!(msg, "Rate limit exceeded");
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_error_with_error_field() {
        let json: Value = serde_json::json!({
            "type": "error",
            "error": "Connection failed"
        });
        let mut text = String::new();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::Error(msg) => {
                assert_eq!(msg, "Connection failed");
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_unknown_event_type() {
        let json: Value = serde_json::json!({
            "type": "some.unknown.event",
            "data": "irrelevant"
        });
        let mut text = String::new();
        let result = parse_codex_event(&json, &mut text);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_missing_type() {
        let json: Value = serde_json::json!({
            "data": "no type field"
        });
        let mut text = String::new();
        let result = parse_codex_event(&json, &mut text);
        assert!(result.is_none());
    }

    #[test]
    fn test_text_accumulation_spacing() {
        let json1: Value = serde_json::json!({
            "type": "item.completed",
            "item": {
                "type": "message",
                "content": [{"type": "text", "text": "Hello"}]
            }
        });
        let json2: Value = serde_json::json!({
            "type": "item.completed",
            "item": {
                "type": "message",
                "content": [{"type": "text", "text": "world"}]
            }
        });

        let mut text = String::new();
        parse_codex_event(&json1, &mut text);
        parse_codex_event(&json2, &mut text);
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn test_text_accumulation_no_extra_space() {
        let json1: Value = serde_json::json!({
            "type": "item.completed",
            "item": {
                "type": "message",
                "content": [{"type": "text", "text": "Hello "}]
            }
        });
        let json2: Value = serde_json::json!({
            "type": "item.completed",
            "item": {
                "type": "message",
                "content": [{"type": "text", "text": "world"}]
            }
        });

        let mut text = String::new();
        parse_codex_event(&json1, &mut text);
        parse_codex_event(&json2, &mut text);
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn test_parse_item_completed_unknown_item_type() {
        let json: Value = serde_json::json!({
            "type": "item.completed",
            "item": {
                "type": "unknown_item_type"
            }
        });
        let mut text = String::new();
        let result = parse_codex_event(&json, &mut text);
        assert!(result.is_none());
    }

    #[test]
    fn test_backend_name() {
        let backend = CodexCliBackend::new(CodexCliConfig::default());
        assert_eq!(backend.name(), "codex-cli");
    }

    #[test]
    fn test_mcp_endpoint_override() {
        let backend = CodexCliBackend::new(CodexCliConfig {
            mcp_endpoint: Some("http://original.com/mcp".to_string()),
            ..Default::default()
        });

        // Before override, should return config value
        assert_eq!(
            backend.effective_mcp_endpoint(),
            Some("http://original.com/mcp".to_string())
        );

        // After override, should return override value
        backend.set_mcp_endpoint("http://override.com/mcp".to_string());
        assert_eq!(
            backend.effective_mcp_endpoint(),
            Some("http://override.com/mcp".to_string())
        );
    }

    #[test]
    fn test_mcp_endpoint_none_by_default() {
        let backend = CodexCliBackend::new(CodexCliConfig::default());
        assert!(backend.effective_mcp_endpoint().is_none());
    }

    #[test]
    fn test_parse_turn_completed_result_field_fallback() {
        // When accumulated_text is empty, turn.completed should fall back to the "result" field
        let json: Value = serde_json::json!({
            "type": "turn.completed",
            "result": "fallback response text"
        });
        let mut text = String::new();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::Done { full_response } => {
                assert_eq!(full_response, "fallback response text");
            }
            other => panic!("Expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_turn_completed_accumulated_takes_precedence() {
        // When accumulated_text has content, it should be used instead of "result" field
        let json: Value = serde_json::json!({
            "type": "turn.completed",
            "result": "should be ignored"
        });
        let mut text = "accumulated text wins".to_string();
        let events = parse_codex_event(&json, &mut text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::Done { full_response } => {
                assert_eq!(full_response, "accumulated text wins");
            }
            other => panic!("Expected Done, got {:?}", other),
        }
        assert!(text.is_empty(), "accumulated_text should be cleared");
    }
}
