// ABOUTME: Single-user interactive TUI mode
// ABOUTME: Same backend as gRPC mode, just local input/output

mod app;
mod input;
mod messages;
mod theme;
mod ui;

use anyhow::{Result, bail};
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use coven_core::backend::{
    ApprovalCallback, Backend, DirectCliBackend, DirectCliConfig, MuxBackend, MuxConfig,
};
use coven_core::{Config, Coven, IncomingMessage, OutgoingEvent};
use futures::StreamExt;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc, oneshot};

use app::{App, AppStatus, PendingApproval};
use input::InputResult;
use messages::{ChatMessage, ToolExecution, ToolStatus};

/// Messages from background processing to main loop
enum BackendMsg {
    Event(OutgoingEvent),
    Done,
    Error(String),
}

/// Shared state for pending tool approvals - maps tool_id to response sender
type PendingApprovals = Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>;

/// RAII guard for terminal cleanup
struct TerminalGuard;

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

pub async fn run(name: &str, agent_id: &str, backend_type: &str, working_dir: &Path) -> Result<()> {
    // Initialize coven core
    let config = Config::load()?;

    // Create shared state for pending approvals (used by mux backend callback)
    let pending_approvals: PendingApprovals = Arc::new(Mutex::new(HashMap::new()));

    let backend = create_backend(
        &config,
        backend_type,
        working_dir,
        pending_approvals.clone(),
    )
    .await?;
    let coven = Arc::new(Coven::new(&config, backend).await?);

    // Setup terminal with cleanup guard
    let _guard = TerminalGuard::new()?;
    let term_backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(term_backend)?;

    // Create app state
    let mut app = App::new(
        name,
        agent_id,
        backend_type,
        &working_dir.display().to_string(),
    );

    // Channel for backend events
    let (event_tx, mut event_rx) = mpsc::channel::<BackendMsg>(100);

    // Pending tool state for approval flow
    let mut pending_tool: Option<(String, String, String)> = None; // (id, name, input)

    // Store agent_id for the loop
    let agent_id_owned = agent_id.to_string();

    // Main event loop
    loop {
        // Render UI
        terminal.draw(|f| ui::render(f, &app))?;

        // Poll for keyboard events with timeout
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                // Only handle key press events (not release)
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match input::handle_key(&mut app, key) {
                    InputResult::Quit => break,
                    InputResult::Cancel => {
                        // Cancel any pending operations
                        app.status = AppStatus::Ready;
                        app.pending_approval = None;
                        pending_tool = None;
                    }
                    InputResult::SendMessage(content) => {
                        // Add user message to chat
                        app.messages.push(ChatMessage::user(content.clone()));
                        app.status = AppStatus::Thinking;

                        // Create incoming message for backend
                        let incoming = IncomingMessage {
                            thread_id: format!("single-{}", agent_id_owned),
                            sender: "user".to_string(),
                            content,
                            frontend: "tui".to_string(),
                            attachments: vec![],
                        };

                        // Spawn task to process with backend
                        let tx = event_tx.clone();
                        let coven_clone = Arc::clone(&coven);
                        tokio::spawn(async move {
                            match coven_clone.handle(incoming).await {
                                Ok(mut stream) => {
                                    while let Some(event) = stream.next().await {
                                        if tx.send(BackendMsg::Event(event)).await.is_err() {
                                            break;
                                        }
                                    }
                                    let _ = tx.send(BackendMsg::Done).await;
                                }
                                Err(e) => {
                                    let _ = tx.send(BackendMsg::Error(e.to_string())).await;
                                }
                            }
                        });

                        // Add empty agent message for streaming
                        app.messages.push(ChatMessage::agent());
                    }
                    InputResult::ApproveTool => {
                        if let Some((tool_id, name, _input)) = pending_tool.take() {
                            // Update tool status to executing by ID
                            if let Some(msg) = app.messages.last_mut() {
                                if let Some(tool) =
                                    msg.tools.iter_mut().find(|t| t.id == tool_id)
                                {
                                    tool.status = ToolStatus::Executing;
                                }
                            }
                            // Remember this tool was approved for the session (lowercase for consistent lookup)
                            app.approved_tools_session.insert(name.to_lowercase());

                            // Send approval response to backend callback
                            let approvals = pending_approvals.clone();
                            let tool_id_clone = tool_id.clone();
                            tokio::spawn(async move {
                                let mut pending = approvals.lock().await;
                                if let Some(sender) = pending.remove(&tool_id_clone) {
                                    let _ = sender.send(true);
                                }
                            });

                            tracing::debug!("Approved tool: {} ({})", name, tool_id);
                            app.status = AppStatus::Streaming;
                            app.pending_approval = None;
                        }
                    }
                    InputResult::DenyTool => {
                        if let Some((tool_id, name, _)) = pending_tool.take() {
                            // Update tool status to denied by ID
                            if let Some(msg) = app.messages.last_mut() {
                                if let Some(tool) =
                                    msg.tools.iter_mut().find(|t| t.id == tool_id)
                                {
                                    tool.status = ToolStatus::Denied;
                                }
                            }

                            // Send denial response to backend callback
                            let approvals = pending_approvals.clone();
                            let tool_id_clone = tool_id.clone();
                            tokio::spawn(async move {
                                let mut pending = approvals.lock().await;
                                if let Some(sender) = pending.remove(&tool_id_clone) {
                                    let _ = sender.send(false);
                                }
                            });

                            tracing::debug!("Denied tool: {} ({})", name, tool_id);
                            app.status = AppStatus::Streaming;
                            app.pending_approval = None;
                        }
                    }
                    InputResult::ApproveAll => {
                        // Auto-approve is already set by handle_key
                        if let Some((tool_id, name, _input)) = pending_tool.take() {
                            if let Some(msg) = app.messages.last_mut() {
                                if let Some(tool) =
                                    msg.tools.iter_mut().find(|t| t.id == tool_id)
                                {
                                    tool.status = ToolStatus::Executing;
                                }
                            }

                            // Send approval response to backend callback
                            let approvals = pending_approvals.clone();
                            let tool_id_clone = tool_id.clone();
                            tokio::spawn(async move {
                                let mut pending = approvals.lock().await;
                                if let Some(sender) = pending.remove(&tool_id_clone) {
                                    let _ = sender.send(true);
                                }
                            });

                            tracing::debug!(
                                "Approved all tools, starting with: {} ({})",
                                name,
                                tool_id
                            );
                            app.status = AppStatus::Streaming;
                            app.pending_approval = None;
                        }
                    }
                    InputResult::Continue => {}
                }
            }
        }

        // Process backend events (non-blocking)
        while let Ok(msg) = event_rx.try_recv() {
            handle_backend_event(&mut app, msg, &mut pending_tool);
        }

        // Exit if quit flag is set
        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Handle an event from the backend and update app state
fn handle_backend_event(
    app: &mut App,
    msg: BackendMsg,
    pending_tool: &mut Option<(String, String, String)>,
) {
    match msg {
        BackendMsg::Event(event) => {
            app.status = AppStatus::Streaming;

            match event {
                OutgoingEvent::Thinking => {
                    app.status = AppStatus::Thinking;
                }
                OutgoingEvent::Text(text) => {
                    // Append text to the last agent message
                    if let Some(msg) = app.messages.last_mut() {
                        if msg.role == messages::Role::Agent {
                            msg.content.push_str(&text);
                        }
                    }
                }
                OutgoingEvent::ToolUse { id, name, input } => {
                    let input_preview = truncate_string(&input.to_string(), 100);

                    // Check if this tool needs approval
                    let needs_approval = app.needs_approval(&name);

                    let status = if needs_approval {
                        ToolStatus::Pending
                    } else {
                        ToolStatus::Executing
                    };

                    // Add tool to the last agent message
                    if let Some(msg) = app.messages.last_mut() {
                        if msg.role == messages::Role::Agent {
                            msg.tools.push(ToolExecution {
                                id: id.clone(),
                                name: name.clone(),
                                input_preview,
                                status,
                                output_preview: None,
                            });
                        }
                    }

                    if needs_approval {
                        // Set up approval flow
                        app.status = AppStatus::AwaitingApproval;
                        app.pending_approval = Some(PendingApproval {
                            tool_id: id.clone(),
                            tool_name: name.clone(),
                            input_json: input.to_string(),
                        });
                        *pending_tool = Some((id, name, input.to_string()));
                    }
                }
                OutgoingEvent::ToolResult {
                    id,
                    output,
                    is_error,
                } => {
                    // Update the tool status by matching tool ID
                    if let Some(msg) = app.messages.last_mut() {
                        if let Some(tool) = msg.tools.iter_mut().find(|t| t.id == id) {
                            tool.status = if is_error {
                                ToolStatus::Failed
                            } else {
                                ToolStatus::Completed
                            };
                            tool.output_preview = Some(truncate_string(&output, 100));
                        }
                    }
                }
                OutgoingEvent::Done { full_response: _ } => {
                    // Mark streaming as complete
                    if let Some(msg) = app.messages.last_mut() {
                        msg.is_streaming = false;
                    }
                    app.status = AppStatus::Ready;
                }
                OutgoingEvent::Error(e) => {
                    app.error_message = Some(e.clone());
                    app.status = AppStatus::Error;
                    // Add error as system message
                    app.messages
                        .push(ChatMessage::system(format!("Error: {}", e)));
                }
                OutgoingEvent::File {
                    path,
                    filename,
                    mime_type: _,
                } => {
                    // Add file info to the last agent message
                    if let Some(msg) = app.messages.last_mut() {
                        if msg.role == messages::Role::Agent {
                            msg.content.push_str(&format!(
                                "\n[File: {} -> {}]",
                                filename,
                                path.display()
                            ));
                        }
                    }
                }
                OutgoingEvent::ToolApprovalRequest { id, name, input } => {
                    // Handle approval request from MuxBackend
                    // In single mode, we already handle local approval via ToolUse events,
                    // but this event comes from the backend when using approval callbacks.
                    // Set up the approval flow similar to ToolUse with needs_approval.
                    let input_preview = truncate_string(&input.to_string(), 100);

                    // Add tool to the last agent message if not already present
                    if let Some(msg) = app.messages.last_mut() {
                        if msg.role == messages::Role::Agent
                            && !msg.tools.iter().any(|t| t.id == id)
                        {
                            msg.tools.push(ToolExecution {
                                id: id.clone(),
                                name: name.clone(),
                                input_preview,
                                status: ToolStatus::Pending,
                                output_preview: None,
                            });
                        }
                    }

                    // Set up approval flow
                    app.status = AppStatus::AwaitingApproval;
                    app.pending_approval = Some(PendingApproval {
                        tool_id: id.clone(),
                        tool_name: name.clone(),
                        input_json: input.to_string(),
                    });
                    *pending_tool = Some((id, name, input.to_string()));
                }
                OutgoingEvent::SessionInit { session_id } => {
                    // Add session info as system message
                    app.messages
                        .push(ChatMessage::system(format!("Session: {}", session_id)));
                }
                OutgoingEvent::SessionOrphaned => {
                    // Session expired - notify user
                    app.error_message = Some("Session expired - please retry".to_string());
                    app.messages.push(ChatMessage::system(
                        "Session expired - please retry".to_string(),
                    ));
                }
                OutgoingEvent::Usage {
                    input_tokens,
                    output_tokens,
                    cache_read_tokens,
                    thinking_tokens,
                    ..
                } => {
                    // Add usage info as system message
                    app.messages.push(ChatMessage::system(format!(
                        "Usage: in={} out={} cache={} think={}",
                        input_tokens, output_tokens, cache_read_tokens, thinking_tokens
                    )));
                }
                OutgoingEvent::ToolState { id, state, detail } => {
                    // Update tool status based on state
                    if let Some(msg) = app.messages.last_mut() {
                        if let Some(tool) = msg.tools.iter_mut().find(|t| t.id == id) {
                            tool.status = match state.as_str() {
                                "pending" => ToolStatus::Pending,
                                "awaiting_approval" => ToolStatus::Pending,
                                "running" => ToolStatus::Executing,
                                "completed" => ToolStatus::Completed,
                                "failed" | "denied" | "timeout" | "cancelled" => ToolStatus::Failed,
                                _ => tool.status,
                            };
                            if let Some(d) = detail {
                                tool.output_preview = Some(truncate_string(&d, 100));
                            }
                        }
                    }
                }
            }
        }
        BackendMsg::Done => {
            if let Some(msg) = app.messages.last_mut() {
                msg.is_streaming = false;
            }
            app.status = AppStatus::Ready;
        }
        BackendMsg::Error(e) => {
            // Only set error state if not already in error (avoids duplicate messages
            // since OutgoingEvent::Error may also be received)
            if app.status != AppStatus::Error {
                app.error_message = Some(e.clone());
                app.status = AppStatus::Error;
                app.messages
                    .push(ChatMessage::system(format!("Error: {}", e)));
            }
        }
    }
}

/// Create backend based on type - mirrors client.rs pattern
async fn create_backend(
    config: &Config,
    backend_type: &str,
    working_dir: &Path,
    pending_approvals: PendingApprovals,
) -> Result<Arc<dyn Backend>> {
    match backend_type {
        "mux" => {
            tracing::info!("Using MuxBackend (direct Anthropic API)");
            tracing::info!("  Working dir: {}", working_dir.display());
            let mux_settings = config.mux.clone();
            let mux_config = MuxConfig {
                model: std::env::var("ANTHROPIC_MODEL").unwrap_or(mux_settings.model),
                max_tokens: std::env::var("ANTHROPIC_MAX_TOKENS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(mux_settings.max_tokens),
                working_dir: working_dir.to_path_buf(),
                global_system_prompt_path: mux_settings
                    .global_system_prompt_path
                    .or_else(|| dirs::home_dir().map(|h| h.join(".mux/system.md"))),
                local_prompt_files: mux_settings.local_prompt_files,
                mcp_servers: vec![],
                skip_default_tools: false,
                gateway_mcp: None, // Set after gateway connection
            };

            // Create approval callback that waits for TUI user response
            // Timeout after 5 minutes to prevent infinite hangs
            const APPROVAL_TIMEOUT_SECS: u64 = 300;

            let approvals = pending_approvals;
            let approval_callback: ApprovalCallback =
                Arc::new(move |tool_id, tool_name, _tool_input| {
                    let approvals = approvals.clone();
                    Box::pin(async move {
                        // Create oneshot channel for this approval
                        let (tx, rx) = oneshot::channel();

                        // Store the sender for when we receive the TUI response
                        {
                            let mut pending = approvals.lock().await;
                            pending.insert(tool_id.clone(), tx);
                        }

                        // Wait for approval response with timeout
                        let timeout = tokio::time::Duration::from_secs(APPROVAL_TIMEOUT_SECS);
                        match tokio::time::timeout(timeout, rx).await {
                            Ok(Ok(approved)) => approved,
                            Ok(Err(_)) => {
                                // Channel closed without response - deny by default
                                tracing::warn!("Approval channel closed, denying tool");
                                false
                            }
                            Err(_) => {
                                // Timeout - clean up and deny
                                tracing::warn!(
                                    "Approval timeout for '{}', denying tool",
                                    tool_name
                                );
                                // Remove the pending entry to avoid memory leak
                                let mut pending = approvals.lock().await;
                                pending.remove(&tool_id);
                                false
                            }
                        }
                    })
                        as Pin<Box<dyn std::future::Future<Output = bool> + Send>>
                });

            Ok(Arc::new(
                MuxBackend::new(mux_config)
                    .await?
                    .with_approval_callback(approval_callback),
            ))
        }
        "cli" => {
            tracing::info!("Using DirectCliBackend (Claude CLI subprocess)");
            tracing::info!("  Binary: {}", config.claude.binary);
            tracing::info!("  Working dir: {}", working_dir.display());
            tracing::info!("  Timeout: {}s", config.claude.timeout_secs);
            let cli_config = DirectCliConfig {
                binary: config.claude.binary.clone(),
                working_dir: working_dir.to_path_buf(),
                timeout_secs: config.claude.timeout_secs,
                mcp_endpoint: None, // No gateway MCP in single-shot mode
            };
            // CLI backend handles its own approval via stdin - no callback needed
            let _ = pending_approvals; // Acknowledge unused parameter for CLI backend
            Ok(Arc::new(DirectCliBackend::new(cli_config)))
        }
        _ => bail!("Unknown backend '{}'. Use 'mux' or 'cli'", backend_type),
    }
}

/// Truncate a string to max_chars characters, adding "..." if truncated.
/// Uses character count rather than byte count for UTF-8 safety.
fn truncate_string(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}
