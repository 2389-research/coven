// ABOUTME: ACP protocol backend - communicates with claude-code-acp or codex-acp.
// ABOUTME: Keeps ACP process alive across prompts for session persistence.

use crate::{Backend, BackendEvent};
use acp::Agent as _;
use agent_client_protocol as acp;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use tokio::process::{Child, Command as ProcessCommand};
use tokio::sync::mpsc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// Configuration for the ACP backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpConfig {
    /// Path to the ACP binary (codex-acp or claude-code-acp)
    pub binary: String,
    /// Timeout in seconds for prompts
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Working directory for the agent
    pub working_dir: PathBuf,
    /// Extra CLI arguments to pass to the ACP binary
    #[serde(default)]
    pub extra_args: Vec<String>,
}

fn default_timeout() -> u64 {
    300 // 5 minutes
}

/// Commands sent to the persistent ACP worker thread
enum WorkerCommand {
    /// Combined send: handles new_session/load_session + prompt in one command
    Send {
        session_id: String,
        message: String,
        is_new_session: bool,
        event_tx: mpsc::Sender<BackendEvent>,
    },
    Cancel {
        session_id: String,
    },
    Shutdown,
}

/// Handler for ACP client-side callbacks
/// Sends events directly to the provided channel for true streaming
struct AcpClientHandler {
    event_tx: Arc<std::sync::RwLock<mpsc::Sender<BackendEvent>>>,
    working_dir: PathBuf,
    /// Buffer for accumulating text to parse **status** patterns across chunks
    text_buffer: std::sync::Mutex<String>,
}

impl AcpClientHandler {
    fn new(
        event_tx: Arc<std::sync::RwLock<mpsc::Sender<BackendEvent>>>,
        working_dir: PathBuf,
    ) -> Self {
        Self {
            event_tx,
            working_dir,
            text_buffer: std::sync::Mutex::new(String::new()),
        }
    }

    fn update_event_tx(&self, new_tx: mpsc::Sender<BackendEvent>) {
        let mut tx = self.event_tx.write().unwrap_or_else(|e| e.into_inner());
        *tx = new_tx;
    }

    /// Create a new dummy sender and drop the old one to close the channel.
    /// This signals to the receiver that no more events are coming.
    fn close_event_channel(&self) {
        let (dummy_tx, _dummy_rx) = mpsc::channel(1);
        let mut tx = self.event_tx.write().unwrap_or_else(|e| e.into_inner());
        *tx = dummy_tx;
        // Old tx is dropped here, closing the channel for the receiver
    }

    fn send_event(&self, event: BackendEvent) {
        let tx = self.event_tx.read().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = tx.try_send(event) {
            match e {
                mpsc::error::TrySendError::Full(dropped_event) => {
                    tracing::warn!(
                        event = ?dropped_event,
                        "Event channel buffer full (2048), dropping event"
                    );
                }
                mpsc::error::TrySendError::Closed(_) => {
                    tracing::debug!("Event channel closed, receiver dropped");
                }
            }
        }
    }

    /// Buffer text and parse **status** patterns (used by codex).
    /// Emits complete patterns as Thinking events, text as Text events.
    fn buffer_and_parse_text(&self, text: &str) {
        let mut buffer = self.text_buffer.lock().unwrap_or_else(|e| e.into_inner());
        buffer.push_str(text);

        // Process complete **...** patterns from the buffer
        loop {
            if let Some(start) = buffer.find("**") {
                // Look for closing **
                let after_start = &buffer[start + 2..];
                if let Some(end) = after_start.find("**") {
                    // Found complete pattern
                    // Emit any text before the **
                    let before = &buffer[..start];
                    if !before.is_empty() {
                        self.send_event(BackendEvent::Text(before.to_string()));
                    }

                    // Emit the status as a Thinking event
                    self.send_event(BackendEvent::Thinking);

                    // Remove processed text from buffer
                    let consumed = start + 2 + end + 2;
                    *buffer = buffer[consumed..].to_string();
                } else {
                    // Have opening ** but no closing yet - keep buffering
                    // But emit any text before the ** to avoid buffering too much
                    if start > 0 {
                        let before = buffer[..start].to_string();
                        self.send_event(BackendEvent::Text(before));
                        *buffer = buffer[start..].to_string();
                    }
                    break;
                }
            } else {
                // No ** pattern found
                // Check if buffer ends with a single * (might be start of **)
                if buffer.ends_with('*') && buffer.len() > 1 {
                    // Keep the trailing * in buffer, emit the rest
                    let emit_len = buffer.len() - 1;
                    if emit_len > 0 {
                        let to_emit = buffer[..emit_len].to_string();
                        self.send_event(BackendEvent::Text(to_emit));
                        *buffer = buffer[emit_len..].to_string();
                    }
                } else if !buffer.is_empty() && !buffer.contains('*') {
                    // No asterisks at all, safe to emit everything
                    let to_emit = std::mem::take(&mut *buffer);
                    self.send_event(BackendEvent::Text(to_emit));
                }
                break;
            }
        }
    }

    /// Flush any remaining buffered text (call when message is complete)
    fn flush_text_buffer(&self) {
        let mut buffer = self.text_buffer.lock().unwrap_or_else(|e| e.into_inner());
        if !buffer.is_empty() {
            let remaining = std::mem::take(&mut *buffer);
            // Try one more parse in case we have a complete pattern
            if let Some(start) = remaining.find("**") {
                let after_start = &remaining[start + 2..];
                if let Some(end) = after_start.find("**") {
                    if start > 0 {
                        self.send_event(BackendEvent::Text(remaining[..start].to_string()));
                    }
                    self.send_event(BackendEvent::Thinking);
                    let after = &after_start[end + 2..];
                    if !after.is_empty() {
                        self.send_event(BackendEvent::Text(after.to_string()));
                    }
                    return;
                }
            }
            // No complete pattern, emit as text
            self.send_event(BackendEvent::Text(remaining));
        }
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Client for AcpClientHandler {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        tracing::debug!(
            session_id = %args.session_id,
            tool_call_id = %args.tool_call.tool_call_id,
            "Auto-approving permission request"
        );

        // Find an "allow once" option to approve
        let allow_option = args
            .options
            .iter()
            .find(|opt| matches!(opt.kind, acp::PermissionOptionKind::AllowOnce))
            .or_else(|| args.options.first());

        if let Some(option) = allow_option {
            Ok(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                    option.option_id.clone(),
                )),
            ))
        } else {
            Ok(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Cancelled,
            ))
        }
    }

    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        tracing::debug!(session_id = %args.session_id, "Received session notification");
        match args.update {
            acp::SessionUpdate::AgentMessageChunk(chunk) => {
                let text = match chunk.content {
                    acp::ContentBlock::Text(t) => t.text,
                    acp::ContentBlock::Image(_) => "<image>".into(),
                    acp::ContentBlock::Audio(_) => "<audio>".into(),
                    acp::ContentBlock::ResourceLink(r) => r.uri,
                    acp::ContentBlock::Resource(_) => "<resource>".into(),
                    _ => String::new(),
                };
                if !text.is_empty() {
                    // Buffer and parse **status** patterns (used by codex for thinking/status)
                    self.buffer_and_parse_text(&text);
                }
            }
            acp::SessionUpdate::ToolCall(tool_call) => {
                // Flush text buffer before tool call
                self.flush_text_buffer();
                let name = tool_call.title.clone();
                let id = tool_call.tool_call_id.to_string();
                let input = tool_call.raw_input.clone().unwrap_or(serde_json::json!({}));

                self.send_event(BackendEvent::ToolUse { id, name, input });
            }
            acp::SessionUpdate::AgentThoughtChunk(chunk) => {
                let text = match chunk.content {
                    acp::ContentBlock::Text(t) => t.text,
                    acp::ContentBlock::Image(_) => "<image>".into(),
                    acp::ContentBlock::Audio(_) => "<audio>".into(),
                    acp::ContentBlock::ResourceLink(r) => r.uri,
                    acp::ContentBlock::Resource(_) => "<resource>".into(),
                    _ => String::new(),
                };
                if !text.is_empty() {
                    tracing::debug!(text_len = text.len(), "Received AgentThoughtChunk");
                    // Buffer and parse **status** patterns (used by codex for thinking/status)
                    self.buffer_and_parse_text(&text);
                }
            }
            other => {
                // Flush text buffer on any other event type
                self.flush_text_buffer();
                tracing::debug!(?other, "Ignoring unhandled session update type");
            }
        }
        Ok(())
    }

    async fn write_text_file(
        &self,
        args: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        let path = self.working_dir.join(&args.path);

        // Security: ensure path stays within working directory
        let canonical_working_dir = self
            .working_dir
            .canonicalize()
            .unwrap_or_else(|_| self.working_dir.clone());

        if let Ok(canonical) = path.canonicalize() {
            // File exists - verify canonical path is within working directory
            if !canonical.starts_with(&canonical_working_dir) {
                tracing::warn!(path = %args.path.display(), "Write attempt outside working directory");
                return Err(acp::Error::invalid_params());
            }
        } else {
            // File doesn't exist - verify parent is within working directory
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
                if let Ok(canonical_parent) = parent.canonicalize() {
                    if !canonical_parent.starts_with(&canonical_working_dir) {
                        tracing::warn!(path = %args.path.display(), "Write attempt outside working directory");
                        return Err(acp::Error::invalid_params());
                    }
                } else {
                    tracing::warn!(path = %args.path.display(), "Cannot resolve parent directory");
                    return Err(acp::Error::invalid_params());
                }
            }
            // Also check filename doesn't contain .. or path separators
            if let Some(filename) = path.file_name() {
                let filename_str = filename.to_string_lossy();
                if filename_str.contains("..") || filename_str.contains('/') {
                    tracing::warn!(path = %args.path.display(), "Invalid filename");
                    return Err(acp::Error::invalid_params());
                }
            }
        }

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::error!(path = %parent.display(), error = %e, "Failed to create parent directories");
                return Err(acp::Error::internal_error());
            }
        }

        if let Err(e) = std::fs::write(&path, &args.content) {
            tracing::error!(path = %path.display(), error = %e, "Failed to write file");
            return Err(acp::Error::internal_error());
        }

        tracing::debug!(path = %args.path.display(), len = args.content.len(), "Wrote file");
        Ok(acp::WriteTextFileResponse::new())
    }

    async fn read_text_file(
        &self,
        args: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        let path = self.working_dir.join(&args.path);

        let canonical_working_dir = self
            .working_dir
            .canonicalize()
            .unwrap_or_else(|_| self.working_dir.clone());

        let canonical = match path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(path = %args.path.display(), error = %e, "Failed to canonicalize path");
                return Err(acp::Error::invalid_params());
            }
        };

        if !canonical.starts_with(&canonical_working_dir) {
            tracing::warn!(path = %args.path.display(), "Read attempt outside working directory");
            return Err(acp::Error::invalid_params());
        }

        let content = match std::fs::read_to_string(&canonical) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(path = %args.path.display(), error = %e, "Failed to read file");
                return Err(acp::Error::invalid_params());
            }
        };

        tracing::debug!(path = %args.path.display(), len = content.len(), "Read file");
        Ok(acp::ReadTextFileResponse::new(content))
    }

    async fn create_terminal(
        &self,
        _args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        // Stub implementation - the ACP agent handles terminal commands internally.
        // We provide a terminal ID for protocol compliance without spawning a process.
        let terminal_id = format!("term-{}", uuid::Uuid::new_v4().as_simple());
        tracing::info!(terminal_id = %terminal_id, "Created terminal (stub)");

        Ok(acp::CreateTerminalResponse::new(acp::TerminalId::new(
            terminal_id,
        )))
    }

    async fn terminal_output(
        &self,
        _args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        tracing::debug!("terminal_output called - not fully implemented");
        Ok(acp::TerminalOutputResponse::new(String::new(), false))
    }

    async fn release_terminal(
        &self,
        args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        tracing::debug!(terminal_id = %args.terminal_id, "Releasing terminal");
        Ok(acp::ReleaseTerminalResponse::new())
    }

    async fn wait_for_terminal_exit(
        &self,
        args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        tracing::debug!(terminal_id = %args.terminal_id, "Waiting for terminal exit");
        Ok(acp::WaitForTerminalExitResponse::new(
            acp::TerminalExitStatus::new(),
        ))
    }

    async fn kill_terminal_command(
        &self,
        args: acp::KillTerminalCommandRequest,
    ) -> acp::Result<acp::KillTerminalCommandResponse> {
        tracing::debug!(terminal_id = %args.terminal_id, "Killing terminal");
        Ok(acp::KillTerminalCommandResponse::new())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<()> {
        Ok(())
    }
}

/// Persistent ACP client that stays alive across prompts
struct PersistentAcpClient {
    child: Child,
    conn: acp::ClientSideConnection,
    handler: Arc<AcpClientHandler>,
    working_dir: PathBuf,
    /// Currently active session ID
    current_session: Option<String>,
}

impl Drop for PersistentAcpClient {
    fn drop(&mut self) {
        if let Err(e) = self.child.start_kill() {
            tracing::warn!(error = %e, "Failed to kill ACP agent process during Drop");
        }
    }
}

impl PersistentAcpClient {
    async fn spawn(
        working_dir: &Path,
        agent_binary: &str,
        extra_args: &[String],
        initial_event_tx: mpsc::Sender<BackendEvent>,
        env_vars: &HashMap<String, String>,
    ) -> Result<Self> {
        if agent_binary.contains("..") || agent_binary.contains('\0') {
            anyhow::bail!("Invalid agent binary path");
        }
        if !working_dir.exists() {
            anyhow::bail!(
                "Working directory does not exist: {}",
                working_dir.display()
            );
        }

        tracing::info!(binary = %agent_binary, ?extra_args, cwd = %working_dir.display(), "Spawning persistent ACP agent");

        let mut child = ProcessCommand::new(agent_binary)
            .args(extra_args)
            .current_dir(working_dir)
            .envs(env_vars)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn ACP agent")?;

        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;

        let shared_event_tx = Arc::new(std::sync::RwLock::new(initial_event_tx));
        let handler = Arc::new(AcpClientHandler::new(
            Arc::clone(&shared_event_tx),
            working_dir.to_path_buf(),
        ));

        // Clone handler for the connection (it implements Client)
        let handler_for_conn = HandlerWrapper(Arc::clone(&handler));

        let (conn, handle_io) = acp::ClientSideConnection::new(
            handler_for_conn,
            stdin.compat_write(),
            stdout.compat(),
            |fut| {
                tokio::task::spawn_local(fut);
            },
        );

        tokio::task::spawn_local(handle_io);

        Ok(Self {
            child,
            conn,
            handler,
            working_dir: working_dir.to_path_buf(),
            current_session: None,
        })
    }

    async fn initialize(&self) -> Result<()> {
        self.conn
            .initialize(
                acp::InitializeRequest::new(acp::ProtocolVersion::V1)
                    .client_capabilities(acp::ClientCapabilities::default())
                    .client_info(
                        acp::Implementation::new("fold-swarm-acp", env!("CARGO_PKG_VERSION"))
                            .title("fold-swarm ACP Backend"),
                    ),
            )
            .await
            .context("ACP initialization failed")?;

        tracing::info!("ACP connection initialized");
        Ok(())
    }

    async fn new_session(&mut self) -> Result<String> {
        tracing::info!(cwd = %self.working_dir.display(), "Calling ACP new_session");
        let response = self
            .conn
            .new_session(acp::NewSessionRequest::new(self.working_dir.clone()))
            .await
            .context("Failed to create new ACP session")?;

        let session_id = response.session_id.to_string();
        self.current_session = Some(session_id.clone());
        tracing::info!(session_id = %session_id, "Created new ACP session");

        Ok(session_id)
    }

    async fn load_session(&mut self, session_id: &str) -> Result<()> {
        self.conn
            .load_session(acp::LoadSessionRequest::new(
                acp::SessionId::new(session_id.to_string()),
                self.working_dir.clone(),
            ))
            .await
            .context("Failed to load ACP session")?;

        self.current_session = Some(session_id.to_string());
        tracing::info!(session_id = %session_id, "Loaded ACP session");
        Ok(())
    }

    fn update_event_tx(&self, new_tx: mpsc::Sender<BackendEvent>) {
        self.handler.update_event_tx(new_tx);
    }

    /// Close the event channel to signal that no more events are coming.
    fn close_event_channel(&self) {
        self.handler.close_event_channel();
    }

    async fn prompt(&self, session_id: &str, text: &str) -> Result<()> {
        tracing::debug!(session_id = %session_id, prompt_len = text.len(), "Sending prompt");

        let result = self
            .conn
            .prompt(acp::PromptRequest::new(
                acp::SessionId::new(session_id.to_string()),
                vec![acp::ContentBlock::Text(acp::TextContent::new(
                    text.to_string(),
                ))],
            ))
            .await;

        // Flush any remaining buffered text
        self.handler.flush_text_buffer();

        match result {
            Ok(_response) => {
                self.handler.send_event(BackendEvent::Done {
                    full_response: String::new(),
                });
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("ACP prompt error: {}", e);
                tracing::error!(%error_msg);
                self.handler
                    .send_event(BackendEvent::Error(error_msg.clone()));
                Err(anyhow::anyhow!(error_msg))
            }
        }
    }

    async fn cancel(&self, session_id: &str) -> Result<()> {
        self.conn
            .cancel(acp::CancelNotification::new(acp::SessionId::new(
                session_id.to_string(),
            )))
            .await
            .context("Failed to cancel ACP operation")?;
        Ok(())
    }
}

/// Wrapper to implement acp::Client for Arc<AcpClientHandler>
struct HandlerWrapper(Arc<AcpClientHandler>);

#[async_trait::async_trait(?Send)]
impl acp::Client for HandlerWrapper {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        self.0.request_permission(args).await
    }

    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        self.0.session_notification(args).await
    }

    async fn write_text_file(
        &self,
        args: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        self.0.write_text_file(args).await
    }

    async fn read_text_file(
        &self,
        args: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        self.0.read_text_file(args).await
    }

    async fn create_terminal(
        &self,
        args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        self.0.create_terminal(args).await
    }

    async fn terminal_output(
        &self,
        args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        self.0.terminal_output(args).await
    }

    async fn release_terminal(
        &self,
        args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        self.0.release_terminal(args).await
    }

    async fn wait_for_terminal_exit(
        &self,
        args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        self.0.wait_for_terminal_exit(args).await
    }

    async fn kill_terminal_command(
        &self,
        args: acp::KillTerminalCommandRequest,
    ) -> acp::Result<acp::KillTerminalCommandResponse> {
        self.0.kill_terminal_command(args).await
    }

    async fn ext_method(&self, args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        self.0.ext_method(args).await
    }

    async fn ext_notification(&self, args: acp::ExtNotification) -> acp::Result<()> {
        self.0.ext_notification(args).await
    }
}

/// Run the persistent ACP worker on a dedicated thread.
/// Handles the combined send() semantics: new_session/load_session + prompt.
fn run_persistent_worker(config: AcpConfig, mut cmd_rx: mpsc::Receiver<WorkerCommand>) {
    // Create a new runtime for this thread
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!(error = %e, "Failed to create tokio runtime for ACP worker");
            return;
        }
    };

    rt.block_on(async {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let env_vars: HashMap<String, String> = std::env::vars().collect();

                // Create a dummy channel for initial spawn - will be replaced on first prompt
                let (dummy_tx, _dummy_rx) = mpsc::channel(1);

                // Spawn the ACP client
                let mut client = match PersistentAcpClient::spawn(
                    &config.working_dir,
                    &config.binary,
                    &config.extra_args,
                    dummy_tx,
                    &env_vars,
                )
                .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to spawn persistent ACP client");
                        return;
                    }
                };

                // Initialize the connection
                if let Err(e) = client.initialize().await {
                    tracing::error!(error = %e, "Failed to initialize ACP connection");
                    return;
                }

                tracing::info!("Persistent ACP worker started");

                // Process commands
                while let Some(cmd) = cmd_rx.recv().await {
                    match cmd {
                        WorkerCommand::Send {
                            session_id,
                            message,
                            is_new_session,
                            event_tx,
                        } => {
                            // Update the event channel for this prompt
                            client.update_event_tx(event_tx.clone());

                            // Handle session management based on is_new_session flag
                            let actual_session_id = if is_new_session {
                                match client.new_session().await {
                                    Ok(new_id) => {
                                        // Emit SessionInit so the caller knows the real session ID
                                        let _ = event_tx
                                            .send(BackendEvent::SessionInit {
                                                session_id: new_id.clone(),
                                            })
                                            .await;
                                        new_id
                                    }
                                    Err(e) => {
                                        let _ = event_tx
                                            .send(BackendEvent::Error(format!(
                                                "Failed to create session: {}",
                                                e
                                            )))
                                            .await;
                                        client.close_event_channel();
                                        continue;
                                    }
                                }
                            } else {
                                match client.load_session(&session_id).await {
                                    Ok(()) => session_id.clone(),
                                    Err(e) => {
                                        tracing::warn!(
                                            error = %e,
                                            session_id = %session_id,
                                            "Failed to load session, treating as orphaned"
                                        );
                                        // Emit SessionOrphaned so the caller can reset
                                        let _ = event_tx
                                            .send(BackendEvent::SessionOrphaned)
                                            .await;
                                        client.close_event_channel();
                                        continue;
                                    }
                                }
                            };

                            // Send the prompt with timeout
                            let timeout_duration =
                                std::time::Duration::from_secs(config.timeout_secs);
                            match tokio::time::timeout(
                                timeout_duration,
                                client.prompt(&actual_session_id, &message),
                            )
                            .await
                            {
                                Ok(Ok(())) => {
                                    tracing::debug!("Prompt completed successfully");
                                }
                                Ok(Err(e)) => {
                                    tracing::error!(error = %e, "Prompt failed");
                                    let _ = event_tx
                                        .send(BackendEvent::Error(format!(
                                            "ACP prompt error: {}",
                                            e
                                        )))
                                        .await;
                                }
                                Err(_) => {
                                    tracing::error!(
                                        timeout_secs = config.timeout_secs,
                                        "Prompt timed out"
                                    );
                                    let _ = event_tx
                                        .send(BackendEvent::Error(format!(
                                            "ACP prompt timed out after {} seconds",
                                            config.timeout_secs
                                        )))
                                        .await;
                                }
                            }

                            // Close the event channel to signal that this prompt is complete.
                            client.close_event_channel();
                        }
                        WorkerCommand::Cancel { session_id } => {
                            if let Err(e) = client.cancel(&session_id).await {
                                tracing::warn!(error = %e, "Cancel failed");
                            }
                        }
                        WorkerCommand::Shutdown => {
                            tracing::info!("ACP worker shutting down");
                            break;
                        }
                    }
                }
            })
            .await;
    });
}

/// ACP backend implementation that implements fold-core's Backend trait.
///
/// This backend spawns a persistent ACP binary (claude-code-acp or codex-acp)
/// and maintains session state across prompts using a dedicated worker thread.
pub struct AcpBackend {
    #[allow(dead_code)]
    config: AcpConfig,
    /// Channel to send commands to the worker thread
    worker_tx: mpsc::Sender<WorkerCommand>,
    /// Ensures worker thread only started once
    _worker_started: Arc<()>,
}

impl AcpBackend {
    /// Create a new ACP backend with the given config.
    /// This spawns a persistent worker thread that manages the ACP process.
    pub fn new(config: AcpConfig) -> Self {
        let (worker_tx, worker_rx) = mpsc::channel::<WorkerCommand>(32);

        // Spawn the persistent worker on a dedicated thread
        let worker_config = config.clone();
        thread::spawn(move || {
            run_persistent_worker(worker_config, worker_rx);
        });

        Self {
            config,
            worker_tx,
            _worker_started: Arc::new(()),
        }
    }

    /// Create from a working directory with default settings
    pub fn from_working_dir(working_dir: PathBuf) -> Self {
        Self::new(AcpConfig {
            binary: "claude-code-acp".to_string(),
            timeout_secs: default_timeout(),
            working_dir,
            extra_args: vec![],
        })
    }

    /// Cancel an ongoing operation for a session
    pub async fn cancel(&self, session_id: &str) -> Result<()> {
        self.worker_tx
            .send(WorkerCommand::Cancel {
                session_id: session_id.to_string(),
            })
            .await
            .map_err(|_| anyhow::anyhow!("Worker channel closed"))?;
        Ok(())
    }
}

impl Drop for AcpBackend {
    fn drop(&mut self) {
        // Send shutdown to worker (best effort)
        let _ = self.worker_tx.try_send(WorkerCommand::Shutdown);
    }
}

#[async_trait]
impl Backend for AcpBackend {
    fn name(&self) -> &'static str {
        "acp"
    }

    async fn send(
        &self,
        session_id: &str,
        message: &str,
        is_new_session: bool,
    ) -> Result<BoxStream<'static, BackendEvent>> {
        // Create channel for events - large buffer for streaming
        let (event_tx, event_rx) = mpsc::channel::<BackendEvent>(2048);

        // Send combined command to worker
        self.worker_tx
            .send(WorkerCommand::Send {
                session_id: session_id.to_string(),
                message: message.to_string(),
                is_new_session,
                event_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Worker channel closed"))?;

        // Convert the receiver into a stream
        let stream = tokio_stream::wrappers::ReceiverStream::new(event_rx);
        Ok(stream.boxed())
    }
}
