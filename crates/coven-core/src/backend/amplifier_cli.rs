// ABOUTME: Amplifier CLI backend - spawns amplifier run as subprocess
// ABOUTME: Parses single JSON output blob from stdout, emits BackendEvents

use super::{Backend, BackendEvent};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::stream::BoxStream;
use serde_json::Value;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command as ProcessCommand};
use tokio::sync::mpsc;

/// Configuration for the Amplifier CLI backend
#[derive(Debug, Clone)]
pub struct AmplifierCliConfig {
    /// Path to the amplifier binary (defaults to "amplifier")
    pub binary: String,
    /// Working directory for the agent
    pub working_dir: PathBuf,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Gateway MCP endpoint URL for pack tools
    pub mcp_endpoint: Option<String>,
}

impl Default for AmplifierCliConfig {
    fn default() -> Self {
        Self {
            binary: "amplifier".to_string(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            timeout_secs: 300,
            mcp_endpoint: None,
        }
    }
}

pub struct AmplifierCliBackend {
    config: AmplifierCliConfig,
    /// MCP endpoint can be set after construction (when token is received from gateway)
    mcp_endpoint_override: std::sync::RwLock<Option<String>>,
}

impl AmplifierCliBackend {
    pub fn new(config: AmplifierCliConfig) -> Self {
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
impl Backend for AmplifierCliBackend {
    fn name(&self) -> &'static str {
        "amplifier-cli"
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
            let child_result = spawn_amplifier_process(
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
                    tracing::error!(error = %e, "Failed to spawn Amplifier CLI process");
                    let _ = tx.send(BackendEvent::Error(e.to_string())).await;
                    let _ = tx
                        .send(BackendEvent::Done {
                            full_response: String::new(),
                        })
                        .await;
                    return;
                }
            };

            // Send thinking indicator (amplifier produces output at the end)
            let _ = tx.send(BackendEvent::Thinking).await;

            // Run the prompt processing with timeout
            let result = tokio::time::timeout(
                timeout_duration,
                process_amplifier_output(&mut child, tx.clone()),
            )
            .await;

            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::error!(error = %e, "Amplifier CLI prompt failed");
                    let _ = tx.send(BackendEvent::Error(e.to_string())).await;
                    let _ = tx
                        .send(BackendEvent::Done {
                            full_response: String::new(),
                        })
                        .await;
                }
                Err(_) => {
                    tracing::error!(
                        timeout_secs = config.timeout_secs,
                        "Amplifier CLI timed out"
                    );
                    // Kill the child process on timeout
                    if let Err(e) = child.kill().await {
                        tracing::warn!(error = %e, "Failed to kill timed-out Amplifier CLI process");
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

/// Spawn the Amplifier CLI process with appropriate arguments.
///
/// Amplifier uses `amplifier run --output-format json-trace --mode single "<message>"`.
/// Session resume uses `--resume <session_id>`.
async fn spawn_amplifier_process(
    config: &AmplifierCliConfig,
    session_id: &str,
    text: &str,
    is_new_session: bool,
    mcp_endpoint: Option<&str>,
) -> Result<Child> {
    if mcp_endpoint.is_some() {
        tracing::warn!("Amplifier CLI does not support MCP server configuration; gateway pack tools will not be available");
    }

    let mut args = vec![
        "run".to_string(),
        "--output-format".to_string(),
        "json-trace".to_string(),
        "--mode".to_string(),
        "single".to_string(),
    ];

    // Session resume for existing sessions
    if !is_new_session {
        args.push("--resume".to_string());
        args.push(session_id.to_string());
    }

    // Separator prevents message content starting with "--" from being parsed as flags
    args.push("--".to_string());
    args.push(text.to_string());

    tracing::debug!(args = ?args, cwd = %config.working_dir.display(), "Spawning Amplifier CLI");

    let child = ProcessCommand::new(&config.binary)
        .args(&args)
        .current_dir(&config.working_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn Amplifier CLI")?;

    Ok(child)
}

/// Process output from a spawned Amplifier CLI process.
///
/// Amplifier emits ANSI-colored human-readable text on stdout followed by a single
/// JSON object at the end. We read all lines, find the JSON blob, and parse it.
async fn process_amplifier_output(
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
                        "Detected orphaned session (Amplifier) - will clear stored session ID"
                    );
                    orphan_detected_stderr.store(true, std::sync::atomic::Ordering::SeqCst);
                } else {
                    tracing::debug!(stderr = %line, "Amplifier CLI stderr");
                }
            }
        }
    });

    // Read all stdout lines and accumulate them
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();
    let mut all_lines = Vec::new();

    while let Ok(Some(line)) = lines.next_line().await {
        all_lines.push(line);
    }

    // Wait for stderr reader to complete
    if let Err(e) = stderr_handle.await {
        tracing::warn!(error = %e, "stderr reader task failed to complete");
    }

    // Wait for child process to avoid zombies
    let status = child.wait().await?;

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

    // Find the JSON blob by scanning backwards for a line starting with '{'
    let json_text = extract_json_from_lines(&all_lines);

    match json_text {
        Some(json_str) => match serde_json::from_str::<Value>(&json_str) {
            Ok(json) => {
                let events = parse_amplifier_response(&json);
                for event in events {
                    if event_tx.send(event).await.is_err() {
                        tracing::debug!("Event receiver closed, stopping stream");
                        return Ok(());
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to parse Amplifier JSON output");
                let _ = event_tx
                    .send(BackendEvent::Error(format!(
                        "Failed to parse Amplifier output: {}",
                        e
                    )))
                    .await;
                let _ = event_tx
                    .send(BackendEvent::Done {
                        full_response: String::new(),
                    })
                    .await;
            }
        },
        None => {
            if !status.success() {
                let _ = event_tx
                    .send(BackendEvent::Error(format!(
                        "Amplifier CLI exited with status: {:?}",
                        status.code()
                    )))
                    .await;
            } else {
                let _ = event_tx
                    .send(BackendEvent::Error(
                        "Amplifier CLI produced no JSON output".to_string(),
                    ))
                    .await;
            }
            let _ = event_tx
                .send(BackendEvent::Done {
                    full_response: String::new(),
                })
                .await;
        }
    }

    Ok(())
}

/// Extract JSON from stdout lines by finding the outermost JSON object.
///
/// Amplifier outputs human-readable text followed by a single JSON object.
/// We scan backwards from the end to find the last line starting with '{',
/// then accumulate lines from that point and attempt to parse valid JSON.
/// Trailing non-JSON content is handled by finding the matching closing '}'.
fn extract_json_from_lines(lines: &[String]) -> Option<String> {
    // Find the last line that starts with '{' (after stripping ANSI escape codes)
    let json_start_idx = lines.iter().rposition(|line| {
        let stripped = strip_ansi_codes(line);
        stripped.trim_start().starts_with('{')
    })?;

    // Accumulate from that line to the end
    let json_str: String = lines[json_start_idx..].join("\n");

    // Strip any ANSI codes that may be embedded in the JSON
    let cleaned = strip_ansi_codes(&json_str);
    let trimmed = cleaned.trim();

    if trimmed.is_empty() {
        return None;
    }

    // Try parsing as-is first (common case: JSON is the last thing in output)
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return Some(trimmed.to_string());
    }

    // If that fails, find the matching closing '}' by tracking brace depth,
    // accounting for braces inside JSON strings
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape_next = false;
    let mut end_pos = None;

    for (i, c) in trimmed.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        match c {
            '\\' if in_string => {
                escape_next = true;
            }
            '"' => {
                in_string = !in_string;
            }
            '{' if !in_string => {
                depth += 1;
            }
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    end_pos = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }

    if let Some(end) = end_pos {
        let json_candidate = &trimmed[..end];
        if serde_json::from_str::<serde_json::Value>(json_candidate).is_ok() {
            return Some(json_candidate.to_string());
        }
    }

    // Last resort: return the trimmed content and let the caller handle parse errors
    Some(trimmed.to_string())
}

/// Strip ANSI escape codes from a string.
///
/// Handles CSI sequences (ESC[...letter), OSC sequences (ESC]...BEL/ST),
/// and other common escape sequences (ESC followed by single character).
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some(&'[') => {
                    // CSI sequence: ESC[ ... <letter>
                    chars.next(); // consume '['
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(&']') => {
                    // OSC sequence: ESC] ... (BEL or ESC\)
                    chars.next(); // consume ']'
                    while let Some(&next) = chars.peek() {
                        if next == '\x07' {
                            // BEL terminates OSC
                            chars.next();
                            break;
                        } else if next == '\x1b' {
                            // ESC\ (ST) terminates OSC
                            chars.next();
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        } else {
                            chars.next();
                        }
                    }
                }
                Some(&c2) if c2.is_ascii_alphanumeric() || c2 == '(' || c2 == ')' => {
                    // Two-character escape: ESC + letter/digit (e.g., ESC7, ESC8, ESCC)
                    // or character set designation: ESC( or ESC)
                    chars.next();
                    // ESC( and ESC) consume one more character (the charset designator)
                    if c2 == '(' || c2 == ')' {
                        chars.next();
                    }
                }
                _ => {
                    // Unknown escape - skip just the ESC character
                }
            }
        } else if c == '\u{9b}' {
            // 8-bit CSI (0x9B) - same as ESC[
            while let Some(&next) = chars.peek() {
                chars.next();
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Parse the Amplifier JSON response into BackendEvents.
///
/// Expected JSON format:
/// ```json
/// {
///   "status": "success",
///   "response": "the text response",
///   "session_id": "sess-abc-123",
///   "execution_trace": [
///     { "tool_name": "read_file", "tool_id": "call-1", "input": {...}, "output": "...", "is_error": false }
///   ],
///   "metadata": { "total_tool_calls": 2, "total_agents_invoked": 1, "duration_ms": 1500 }
/// }
/// ```
fn parse_amplifier_response(json: &Value) -> Vec<BackendEvent> {
    let mut events = Vec::new();

    // Check status
    let status = json
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("unknown");

    if status == "error" {
        let error_msg = json
            .get("response")
            .or_else(|| json.get("error"))
            .and_then(|e| e.as_str())
            .unwrap_or("Unknown Amplifier error");
        events.push(BackendEvent::Error(error_msg.to_string()));
        events.push(BackendEvent::Done {
            full_response: String::new(),
        });
        return events;
    }

    // Emit SessionInit if session_id is present
    if let Some(session_id) = json.get("session_id").and_then(|s| s.as_str()) {
        if !session_id.is_empty() {
            events.push(BackendEvent::SessionInit {
                session_id: session_id.to_string(),
            });
        }
    }

    // Emit ToolUse + ToolResult for each execution trace entry
    if let Some(trace) = json.get("execution_trace").and_then(|t| t.as_array()) {
        for entry in trace {
            let tool_name = entry
                .get("tool_name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();
            let tool_id = entry
                .get("tool_id")
                .or_else(|| entry.get("id"))
                .and_then(|i| i.as_str())
                .unwrap_or("unknown")
                .to_string();
            let input = entry.get("input").cloned().unwrap_or(Value::Null);
            let output = entry
                .get("output")
                .and_then(|o| {
                    if o.is_string() {
                        o.as_str().map(|s| s.to_string())
                    } else {
                        Some(o.to_string())
                    }
                })
                .unwrap_or_default();
            let is_error = entry
                .get("is_error")
                .and_then(|e| e.as_bool())
                .unwrap_or(false);

            events.push(BackendEvent::ToolUse {
                id: tool_id.clone(),
                name: tool_name,
                input,
            });
            events.push(BackendEvent::ToolResult {
                id: tool_id,
                output,
                is_error,
            });
        }
    }

    // Emit the response text
    let response = json
        .get("response")
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .to_string();

    if !response.is_empty() {
        events.push(BackendEvent::Text(response.clone()));
    }

    // Emit Done with the full response
    events.push(BackendEvent::Done {
        full_response: response,
    });

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amplifier_cli_config_default() {
        let config = AmplifierCliConfig::default();
        assert_eq!(config.binary, "amplifier");
        assert_eq!(config.timeout_secs, 300);
        assert!(config.mcp_endpoint.is_none());
    }

    #[test]
    fn test_backend_name() {
        let backend = AmplifierCliBackend::new(AmplifierCliConfig::default());
        assert_eq!(backend.name(), "amplifier-cli");
    }

    #[test]
    fn test_mcp_endpoint_override() {
        let backend = AmplifierCliBackend::new(AmplifierCliConfig {
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
        let backend = AmplifierCliBackend::new(AmplifierCliConfig::default());
        assert!(backend.effective_mcp_endpoint().is_none());
    }

    #[test]
    fn test_parse_amplifier_success() {
        let json: Value = serde_json::json!({
            "status": "success",
            "response": "Hello from Amplifier!",
            "session_id": "sess-amp-123",
            "execution_trace": [],
            "metadata": {
                "total_tool_calls": 0,
                "total_agents_invoked": 1,
                "duration_ms": 500
            }
        });

        let events = parse_amplifier_response(&json);

        // Should have: SessionInit, Text, Done
        assert_eq!(events.len(), 3);

        match &events[0] {
            BackendEvent::SessionInit { session_id } => {
                assert_eq!(session_id, "sess-amp-123");
            }
            other => panic!("Expected SessionInit, got {:?}", other),
        }

        match &events[1] {
            BackendEvent::Text(t) => assert_eq!(t, "Hello from Amplifier!"),
            other => panic!("Expected Text, got {:?}", other),
        }

        match &events[2] {
            BackendEvent::Done { full_response } => {
                assert_eq!(full_response, "Hello from Amplifier!");
            }
            other => panic!("Expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_amplifier_with_tools() {
        let json: Value = serde_json::json!({
            "status": "success",
            "response": "I read the file for you.",
            "session_id": "sess-amp-456",
            "execution_trace": [
                {
                    "tool_name": "read_file",
                    "tool_id": "call-001",
                    "input": {"path": "/tmp/test.txt"},
                    "output": "file contents here",
                    "is_error": false
                },
                {
                    "tool_name": "write_file",
                    "tool_id": "call-002",
                    "input": {"path": "/tmp/out.txt", "content": "hello"},
                    "output": "written successfully",
                    "is_error": false
                }
            ],
            "metadata": {
                "total_tool_calls": 2,
                "total_agents_invoked": 1,
                "duration_ms": 1500
            }
        });

        let events = parse_amplifier_response(&json);

        // SessionInit + 2*(ToolUse + ToolResult) + Text + Done = 7
        assert_eq!(events.len(), 7);

        match &events[0] {
            BackendEvent::SessionInit { session_id } => {
                assert_eq!(session_id, "sess-amp-456");
            }
            other => panic!("Expected SessionInit, got {:?}", other),
        }

        // First tool use
        match &events[1] {
            BackendEvent::ToolUse { id, name, input } => {
                assert_eq!(id, "call-001");
                assert_eq!(name, "read_file");
                assert_eq!(input["path"], "/tmp/test.txt");
            }
            other => panic!("Expected ToolUse, got {:?}", other),
        }

        // First tool result
        match &events[2] {
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

        // Second tool use
        match &events[3] {
            BackendEvent::ToolUse { id, name, input } => {
                assert_eq!(id, "call-002");
                assert_eq!(name, "write_file");
                assert_eq!(input["path"], "/tmp/out.txt");
            }
            other => panic!("Expected ToolUse, got {:?}", other),
        }

        // Second tool result
        match &events[4] {
            BackendEvent::ToolResult {
                id,
                output,
                is_error,
            } => {
                assert_eq!(id, "call-002");
                assert_eq!(output, "written successfully");
                assert!(!is_error);
            }
            other => panic!("Expected ToolResult, got {:?}", other),
        }

        // Text
        match &events[5] {
            BackendEvent::Text(t) => assert_eq!(t, "I read the file for you."),
            other => panic!("Expected Text, got {:?}", other),
        }

        // Done
        match &events[6] {
            BackendEvent::Done { full_response } => {
                assert_eq!(full_response, "I read the file for you.");
            }
            other => panic!("Expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_amplifier_error() {
        let json: Value = serde_json::json!({
            "status": "error",
            "response": "Rate limit exceeded"
        });

        let events = parse_amplifier_response(&json);

        assert_eq!(events.len(), 2);

        match &events[0] {
            BackendEvent::Error(msg) => {
                assert_eq!(msg, "Rate limit exceeded");
            }
            other => panic!("Expected Error, got {:?}", other),
        }

        match &events[1] {
            BackendEvent::Done { full_response } => {
                assert_eq!(full_response, "");
            }
            other => panic!("Expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_amplifier_error_with_error_field() {
        let json: Value = serde_json::json!({
            "status": "error",
            "error": "Connection failed"
        });

        let events = parse_amplifier_response(&json);

        assert_eq!(events.len(), 2);

        match &events[0] {
            BackendEvent::Error(msg) => {
                assert_eq!(msg, "Connection failed");
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_amplifier_tool_error() {
        let json: Value = serde_json::json!({
            "status": "success",
            "response": "The file was not found.",
            "session_id": "sess-amp-789",
            "execution_trace": [
                {
                    "tool_name": "read_file",
                    "tool_id": "call-err-1",
                    "input": {"path": "/nonexistent"},
                    "output": "file not found",
                    "is_error": true
                }
            ],
            "metadata": {}
        });

        let events = parse_amplifier_response(&json);

        // SessionInit + ToolUse + ToolResult + Text + Done = 5
        assert_eq!(events.len(), 5);

        match &events[2] {
            BackendEvent::ToolResult {
                id,
                output,
                is_error,
            } => {
                assert_eq!(id, "call-err-1");
                assert_eq!(output, "file not found");
                assert!(is_error);
            }
            other => panic!("Expected ToolResult with error, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_amplifier_no_session_id() {
        let json: Value = serde_json::json!({
            "status": "success",
            "response": "Hello!",
            "execution_trace": []
        });

        let events = parse_amplifier_response(&json);

        // No SessionInit, just Text + Done = 2
        assert_eq!(events.len(), 2);

        match &events[0] {
            BackendEvent::Text(t) => assert_eq!(t, "Hello!"),
            other => panic!("Expected Text, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_amplifier_empty_response() {
        let json: Value = serde_json::json!({
            "status": "success",
            "response": "",
            "session_id": "sess-1",
            "execution_trace": []
        });

        let events = parse_amplifier_response(&json);

        // SessionInit + Done (no Text because response is empty)
        assert_eq!(events.len(), 2);

        match &events[0] {
            BackendEvent::SessionInit { session_id } => {
                assert_eq!(session_id, "sess-1");
            }
            other => panic!("Expected SessionInit, got {:?}", other),
        }

        match &events[1] {
            BackendEvent::Done { full_response } => {
                assert_eq!(full_response, "");
            }
            other => panic!("Expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_extract_json_from_lines_simple() {
        let lines = vec![
            "Preparing bundle...".to_string(),
            "Thinking...".to_string(),
            r#"{"status": "success", "response": "Hello"}"#.to_string(),
        ];

        let json = extract_json_from_lines(&lines);
        assert!(json.is_some());
        let parsed: Value = serde_json::from_str(&json.unwrap()).unwrap();
        assert_eq!(parsed["status"], "success");
    }

    #[test]
    fn test_extract_json_from_lines_with_ansi() {
        let lines = vec![
            "\x1b[32mPreparing...\x1b[0m".to_string(),
            "\x1b[33mThinking...\x1b[0m".to_string(),
            r#"{"status": "success", "response": "Hello"}"#.to_string(),
        ];

        let json = extract_json_from_lines(&lines);
        assert!(json.is_some());
        let parsed: Value = serde_json::from_str(&json.unwrap()).unwrap();
        assert_eq!(parsed["response"], "Hello");
    }

    #[test]
    fn test_extract_json_from_lines_multiline_json() {
        let lines = vec![
            "Preparing...".to_string(),
            "{".to_string(),
            r#"  "status": "success","#.to_string(),
            r#"  "response": "Hello""#.to_string(),
            "}".to_string(),
        ];

        let json = extract_json_from_lines(&lines);
        assert!(json.is_some());
        let parsed: Value = serde_json::from_str(&json.unwrap()).unwrap();
        assert_eq!(parsed["status"], "success");
    }

    #[test]
    fn test_extract_json_from_lines_no_json() {
        let lines = vec!["Preparing...".to_string(), "Done.".to_string()];

        let json = extract_json_from_lines(&lines);
        assert!(json.is_none());
    }

    #[test]
    fn test_strip_ansi_codes() {
        assert_eq!(strip_ansi_codes("\x1b[32mHello\x1b[0m"), "Hello");
        assert_eq!(strip_ansi_codes("No codes here"), "No codes here");
        assert_eq!(strip_ansi_codes("\x1b[1;31mBold Red\x1b[0m"), "Bold Red");
        assert_eq!(strip_ansi_codes(""), "");
    }

    #[test]
    fn test_strip_ansi_codes_osc_sequence() {
        // OSC with BEL terminator (terminal title)
        assert_eq!(strip_ansi_codes("\x1b]0;My Title\x07Hello"), "Hello");
        // OSC with ST terminator
        assert_eq!(strip_ansi_codes("\x1b]0;My Title\x1b\\Hello"), "Hello");
    }

    #[test]
    fn test_strip_ansi_codes_two_char_escape() {
        // ESC7 (save cursor), ESC8 (restore cursor)
        assert_eq!(strip_ansi_codes("\x1b7Hello\x1b8"), "Hello");
        // Character set designation ESC(B
        assert_eq!(strip_ansi_codes("\x1b(BHello"), "Hello");
    }

    #[test]
    fn test_parse_amplifier_null_response() {
        let json: Value = serde_json::json!({
            "status": "success",
            "response": null,
            "session_id": "sess-1"
        });

        let events = parse_amplifier_response(&json);
        // SessionInit + Done (no Text because response is null -> empty)
        assert_eq!(events.len(), 2);
        match &events[1] {
            BackendEvent::Done { full_response } => {
                assert_eq!(full_response, "");
            }
            other => panic!("Expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_amplifier_numeric_response() {
        let json: Value = serde_json::json!({
            "status": "success",
            "response": 42
        });

        let events = parse_amplifier_response(&json);
        // Done only (numeric response falls through as_str to empty)
        assert_eq!(events.len(), 1);
        match &events[0] {
            BackendEvent::Done { full_response } => {
                assert_eq!(full_response, "");
            }
            other => panic!("Expected Done, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_amplifier_trace_missing_fields() {
        // Trace entry with completely missing fields should use defaults
        let json: Value = serde_json::json!({
            "status": "success",
            "response": "done",
            "session_id": "s1",
            "execution_trace": [
                {}
            ]
        });

        let events = parse_amplifier_response(&json);
        // SessionInit + ToolUse + ToolResult + Text + Done = 5
        assert_eq!(events.len(), 5);

        match &events[1] {
            BackendEvent::ToolUse { id, name, input } => {
                assert_eq!(id, "unknown");
                assert_eq!(name, "unknown");
                assert_eq!(*input, Value::Null);
            }
            other => panic!("Expected ToolUse, got {:?}", other),
        }

        match &events[2] {
            BackendEvent::ToolResult {
                id,
                output,
                is_error,
            } => {
                assert_eq!(id, "unknown");
                assert_eq!(output, "");
                assert!(!is_error);
            }
            other => panic!("Expected ToolResult, got {:?}", other),
        }
    }

    #[test]
    fn test_extract_json_from_lines_trailing_content() {
        let lines = vec![
            "Preparing...".to_string(),
            r#"{"status": "success", "response": "Hello"}"#.to_string(),
            "Done processing.".to_string(),
        ];

        let json = extract_json_from_lines(&lines);
        assert!(json.is_some());
        let parsed: Value = serde_json::from_str(&json.unwrap()).unwrap();
        assert_eq!(parsed["status"], "success");
    }

    #[test]
    fn test_extract_json_from_lines_false_positive_brace() {
        // A non-JSON line with '{' after the real JSON
        let lines = vec![
            r#"{"status": "success", "response": "Hello"}"#.to_string(),
            "{not json at all".to_string(),
        ];

        // The last '{' line is not valid JSON, but extract should still find the real JSON
        // by trying parse and falling back to brace matching
        let json = extract_json_from_lines(&lines);
        // This should find something (the last line starts with '{')
        // The brace matcher will find the incomplete brace and not match,
        // so it returns the raw content. The caller handles parse errors.
        assert!(json.is_some());
    }
}
