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
/// When an MCP endpoint is provided, passes --mcp-config to connect the CLI
/// to the gateway's Streamable HTTP MCP server for pack tools.
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

    // Pass MCP config inline so Claude CLI discovers pack tools via Streamable HTTP
    if let Some(endpoint) = mcp_endpoint {
        let mcp_config = serde_json::json!({
            "mcpServers": {
                "fold-gateway": {
                    "type": "http",
                    "url": endpoint
                }
            }
        });
        args.push("--mcp-config".to_string());
        args.push(mcp_config.to_string());
    }

    // Only use --resume for existing sessions, not new ones
    if !is_new_session {
        args.push("--resume".to_string());
        args.push(session_id.to_string());
    }

    args.push(text.to_string());

    // Redact --mcp-config values to avoid logging credentials (tokens in MCP URLs)
    let redacted_args: Vec<String> = args
        .iter()
        .enumerate()
        .map(|(i, arg)| {
            if i > 0 && args[i - 1] == "--mcp-config" {
                "[REDACTED]".to_string()
            } else {
                arg.clone()
            }
        })
        .collect();
    tracing::debug!(args = ?redacted_args, cwd = %config.working_dir.display(), "Spawning Claude CLI");

    let child = ProcessCommand::new(&config.binary)
        .args(&args)
        .current_dir(&config.working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn Claude CLI")?;

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
                let display_line = if line.len() > 200 {
                    format!("{}...[truncated {} chars]", &line[..200], line.len() - 200)
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
                let message = json
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("Unknown error")
                    .to_string();

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

                    if input_tokens > 0 || output_tokens > 0 {
                        events.push(BackendEvent::Usage {
                            input_tokens,
                            output_tokens,
                            cache_read_tokens,
                            cache_write_tokens,
                            thinking_tokens: 0, // CLI doesn't expose this
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
        _ => None,
    }
}
