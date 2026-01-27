// ABOUTME: GRPC client implementation for coven-agent
// ABOUTME: Handles connection, registration, message processing loop

use anyhow::{Result, bail};
use coven_core::backend::{
    ApprovalCallback, Backend, DirectCliBackend, DirectCliConfig, MuxBackend, MuxConfig,
};
use coven_core::{Config, Coven, IncomingMessage, OutgoingEvent};
use coven_proto::coven_control_client::CovenControlClient;
use coven_proto::client_service_client::ClientServiceClient;
use coven_proto::{AgentMessage, MessageResponse, RegisterAgent, RegisterAgentRequest, agent_message, server_message};
use coven_ssh::{
    SshAuthCredentials, compute_fingerprint, default_agent_key_path, load_or_generate_key,
};
use futures::StreamExt;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore, mpsc, oneshot};
use tonic::Code;
use tonic::transport::Channel;

use crate::pack_tool::{
    PackTool, PendingPackTools, handle_pack_tool_result, new_pending_pack_tools,
};

/// Maximum number of registration attempts before giving up
const MAX_REGISTRATION_ATTEMPTS: usize = 100;

/// Try to self-register the agent using JWT auth from coven-link config
async fn try_self_register(server_addr: &str, fingerprint: &str, display_name: &str) -> Result<bool> {
    // Load token from ~/.config/coven/token
    let token_path = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?
        .join(".config/coven/token");

    let token = match std::fs::read_to_string(&token_path) {
        Ok(t) => t.trim().to_string(),
        Err(_) => {
            eprintln!("  No coven token found at {}. Run 'coven-link' first.", token_path.display());
            return Ok(false);
        }
    };

    if token.is_empty() {
        eprintln!("  Coven token file is empty. Run 'coven-link' first.");
        return Ok(false);
    }

    eprintln!("  Attempting auto-registration...");

    // Connect to ClientService with JWT auth
    let channel = Channel::from_shared(server_addr.to_string())?
        .connect()
        .await?;

    let token_clone = token.clone();
    let jwt_interceptor = move |mut req: tonic::Request<()>| -> std::result::Result<tonic::Request<()>, tonic::Status> {
        let auth_value = format!("Bearer {}", token_clone)
            .parse()
            .map_err(|_| tonic::Status::internal("invalid token format"))?;
        req.metadata_mut().insert("authorization", auth_value);
        Ok(req)
    };

    let mut client = ClientServiceClient::with_interceptor(channel, jwt_interceptor);

    // Call RegisterAgent
    let request = RegisterAgentRequest {
        display_name: display_name.to_string(),
        fingerprint: fingerprint.to_string(),
    };

    match client.register_agent(request).await {
        Ok(response) => {
            let resp = response.into_inner();
            eprintln!("  ✓ Agent registered! Principal ID: {}", resp.principal_id);
            Ok(true)
        }
        Err(e) if e.code() == Code::AlreadyExists => {
            // Already registered - this is fine, just means we can proceed
            eprintln!("  Agent fingerprint already registered.");
            Ok(true)
        }
        Err(e) => {
            eprintln!("  Auto-registration failed: {}", e.message());
            Ok(false)
        }
    }
}

/// Maximum file size allowed for OutgoingEvent::File (10 MB)
const MAX_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024;

/// Maximum concurrent message processing tasks (backpressure)
const MAX_CONCURRENT_MESSAGES: usize = 8;

/// Shared state for pending tool approvals - maps tool_id to response sender
type PendingApprovals = Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>;

/// Per-thread locks ensuring messages to the same thread are processed sequentially
type ThreadLocks = Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>;

pub async fn run(
    server_addr: &str,
    agent_id: &str,
    backend_type: &str,
    working_dir: &std::path::Path,
    verbose: bool,
    metadata: crate::metadata::AgentMetadata,
) -> Result<()> {
    // Initialize coven core components
    let config = Config::load()?;

    // Create shared state for pending approvals
    let pending_approvals: PendingApprovals = Arc::new(Mutex::new(HashMap::new()));

    // Create shared state for pending pack tool requests
    let pending_pack_tools: PendingPackTools = new_pending_pack_tools();

    // Create backend based on type - use the provided working_dir
    // For mux backend, keep a reference for registering pack tools later
    // For cli backend, keep a reference to set MCP endpoint after receiving token
    let mut mux_backend: Option<Arc<MuxBackend>> = None;
    let mut cli_backend: Option<Arc<DirectCliBackend>> = None;
    let backend: Arc<dyn Backend> = match backend_type {
        "mux" => {
            eprintln!("Using MuxBackend (direct Anthropic API)");
            eprintln!("  Working dir: {}", working_dir.display());
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

            // Create approval callback that waits for gateway response
            // Timeout after 5 minutes to prevent infinite hangs
            const APPROVAL_TIMEOUT_SECS: u64 = 300;

            let approvals = pending_approvals.clone();
            let approval_callback: ApprovalCallback =
                Arc::new(move |tool_id, tool_name, _tool_input| {
                    let approvals = approvals.clone();
                    Box::pin(async move {
                        // Create oneshot channel for this approval
                        let (tx, rx) = oneshot::channel();

                        // Store the sender for when we receive the response
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
                                eprintln!("  WARNING: Approval channel closed, denying tool");
                                false
                            }
                            Err(_) => {
                                // Timeout - clean up and deny
                                eprintln!(
                                    "  WARNING: Approval timeout for '{}', denying tool",
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

            let backend = Arc::new(
                MuxBackend::new(mux_config)
                    .await?
                    .with_approval_callback(approval_callback),
            );
            mux_backend = Some(backend.clone());
            backend
        }
        "cli" => {
            eprintln!("Using DirectCliBackend (Claude CLI subprocess)");
            eprintln!("  Binary: {}", config.claude.binary);
            eprintln!("  Working dir: {}", working_dir.display());
            eprintln!("  Timeout: {}s", config.claude.timeout_secs);
            eprintln!("  MCP endpoint: (will be set after gateway connection)");

            // Don't set MCP endpoint yet - we'll get the token from Welcome message
            let cli_config = DirectCliConfig {
                binary: config.claude.binary.clone(),
                working_dir: working_dir.to_path_buf(),
                timeout_secs: config.claude.timeout_secs,
                mcp_endpoint: None,
            };
            let backend = Arc::new(DirectCliBackend::new(cli_config));
            cli_backend = Some(backend.clone());
            backend
        }
        _ => bail!("Unknown backend '{}'. Use 'mux' or 'cli'", backend_type),
    };

    let coven = Coven::new(&config, backend).await?;

    // Load or generate SSH key for authentication
    let key_path = default_agent_key_path()
        .ok_or_else(|| anyhow::anyhow!("could not determine config directory for SSH key"))?;

    eprintln!("[1/5] Loading SSH key from {}...", key_path.display());
    let private_key = load_or_generate_key(&key_path)?;
    let fingerprint = compute_fingerprint(private_key.public_key())?;
    eprintln!("  Fingerprint: {}", fingerprint);
    eprintln!("  (Register this fingerprint with the gateway using coven-admin)");

    let private_key = Arc::new(private_key);

    // Registration retry loop - try with incrementing suffix if name is taken
    // Also handles auto-registration if fingerprint is unknown
    let mut suffix: usize = 0;
    let mut needs_reconnect = false;
    let (tx, mut inbound, registered_id) = loop {
        let current_id = if suffix == 0 {
            agent_id.to_string()
        } else {
            format!("{}-{}", agent_id, suffix)
        };

        // Connect to server (or reconnect after auto-registration)
        if !needs_reconnect {
            eprintln!("[2/5] Connecting to gateway at {}...", server_addr);
        }
        let channel = Channel::from_shared(server_addr.to_string())?
            .connect()
            .await?;
        if !needs_reconnect {
            eprintln!("[3/5] TCP connection established");
        }
        needs_reconnect = false;

        // Create SSH auth interceptor (fresh each iteration)
        let private_key_clone = private_key.clone();
        let ssh_auth_interceptor = move |mut req: tonic::Request<()>| -> std::result::Result<tonic::Request<()>, tonic::Status> {
            match SshAuthCredentials::new(&private_key_clone) {
                Ok(creds) => {
                    if let Err(e) = creds.apply_to_request(&mut req) {
                        return Err(tonic::Status::internal(format!("failed to apply SSH auth: {}", e)));
                    }
                }
                Err(e) => {
                    return Err(tonic::Status::internal(format!("failed to create SSH auth credentials: {}", e)));
                }
            }
            Ok(req)
        };

        let mut client = CovenControlClient::with_interceptor(channel, ssh_auth_interceptor);

        // Create bidirectional stream
        let (tx, rx) = mpsc::channel::<AgentMessage>(100);
        let outbound = tokio_stream::wrappers::ReceiverStream::new(rx);

        eprintln!("[4/5] Opening bidirectional stream...");
        let response = match client.agent_stream(outbound).await {
            Ok(r) => r,
            Err(e) if e.code() == Code::Unauthenticated && e.message().contains("unknown public key") => {
                // Try to self-register using coven-link token
                eprintln!("  Fingerprint not registered. Attempting auto-registration...");
                if try_self_register(server_addr, &fingerprint, agent_id).await? {
                    // Set flag to reconnect and restart the loop
                    eprintln!("  Reconnecting with registered key...");
                    needs_reconnect = true;
                    continue;
                } else {
                    return Err(e.into());
                }
            }
            Err(e) => return Err(e.into()),
        };
        let mut inbound = response.into_inner();
        eprintln!("[5/5] Stream established, sending registration...");

        // Send registration
        tx.send(AgentMessage {
            payload: Some(agent_message::Payload::Register(RegisterAgent {
                agent_id: current_id.clone(),
                name: current_id.clone(),
                capabilities: vec!["chat".to_string()],
                metadata: Some(metadata.clone().into()),
                protocol_features: vec!["token_usage".to_string(), "tool_states".to_string()],
            })),
        })
        .await?;

        eprintln!("Registration sent! Waiting for welcome from gateway...");

        // Wait for Welcome or RegistrationError
        match inbound.next().await {
            Some(Ok(msg)) => match msg.payload {
                Some(server_message::Payload::Welcome(welcome)) => {
                    eprintln!(
                        "✓ Connected to gateway '{}' as agent '{}'",
                        welcome.server_id, welcome.agent_id
                    );
                    eprintln!("  Instance ID: {}", welcome.instance_id);

                    // Set MCP endpoint for CLI backend if endpoint and token provided
                    if let Some(ref cli) = cli_backend {
                        if !welcome.mcp_endpoint.is_empty() && !welcome.mcp_token.is_empty() {
                            let mcp_url =
                                crate::build_mcp_url(&welcome.mcp_endpoint, &welcome.mcp_token);
                            // Don't log full URL - it contains the auth token
                            eprintln!("  MCP endpoint: configured (token received)");
                            cli.set_mcp_endpoint(mcp_url);
                        } else if welcome.mcp_endpoint.is_empty() {
                            eprintln!("  MCP endpoint: (not provided by gateway)");
                        } else {
                            eprintln!(
                                "  MCP endpoint: (no token provided, pack tools unavailable)"
                            );
                        }
                    }

                    // Register pack tools if available
                    let tool_count = welcome.available_tools.len();
                    if tool_count > 0 {
                        eprintln!("  Pack tools: {} available", tool_count);
                        if let Some(ref mux) = mux_backend {
                            for tool_def in &welcome.available_tools {
                                let pack_tool =
                                    PackTool::new(tool_def, tx.clone(), pending_pack_tools.clone());
                                eprintln!("    - {}", tool_def.name);
                                mux.register_tool(pack_tool).await;
                            }
                        } else {
                            eprintln!("    (skipped - not using mux backend)");
                        }
                    }

                    eprintln!();
                    eprintln!("To bind a channel to this agent:");
                    eprintln!("  Slack:  /coven bind {}", welcome.instance_id);
                    eprintln!("  Matrix: !coven bind {}", welcome.instance_id);
                    eprintln!();
                    eprintln!("Ready and waiting for messages...");
                    break (tx, inbound, welcome.agent_id);
                }
                Some(server_message::Payload::RegistrationError(err)) => {
                    eprintln!("Registration rejected: {} (trying with suffix)", err.reason);
                    suffix += 1;
                    if suffix >= MAX_REGISTRATION_ATTEMPTS {
                        bail!(
                            "Failed to register after {} attempts - all IDs taken",
                            MAX_REGISTRATION_ATTEMPTS
                        );
                    }
                    // Stream is dropped here, will create new one in next iteration
                    continue;
                }
                Some(server_message::Payload::Shutdown(shutdown)) => {
                    bail!("Server shutdown during registration: {}", shutdown.reason);
                }
                other => {
                    bail!("Unexpected message during registration: {:?}", other);
                }
            },
            Some(Err(e)) => {
                // Check if this is an AlreadyExists error - if so, retry with suffix
                if e.code() == Code::AlreadyExists {
                    eprintln!(
                        "Registration rejected: {} (trying with suffix)",
                        e.message()
                    );
                    suffix += 1;
                    if suffix >= MAX_REGISTRATION_ATTEMPTS {
                        bail!(
                            "Failed to register after {} attempts - all IDs taken",
                            MAX_REGISTRATION_ATTEMPTS
                        );
                    }
                    continue;
                }
                return Err(e.into());
            }
            None => {
                bail!("Stream closed before registration completed");
            }
        }
    };

    // Log if we had to use a suffix
    if suffix > 0 {
        eprintln!("  (Registered as '{}' due to name conflict)", registered_id);
    }

    // Wrap coven in Arc for sharing with spawned tasks
    let coven = Arc::new(coven);

    // Backpressure: limit concurrent message processing tasks
    let message_semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_MESSAGES));

    // Per-thread locks: ensure messages to the same thread are processed sequentially
    let thread_locks: ThreadLocks = Arc::new(Mutex::new(HashMap::new()));

    // Process server messages
    // IMPORTANT: Message processing is spawned in separate tasks so this loop
    // can continue receiving PackToolResult and ToolApproval messages that
    // need to be delivered while message processing is in progress.
    while let Some(msg) = inbound.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                eprintln!("ERROR: gRPC stream error: {}", e);
                return Err(e.into());
            }
        };
        match msg.payload {
            Some(server_message::Payload::Welcome(_)) => {
                // Already handled during registration, ignore duplicate
            }
            Some(server_message::Payload::RegistrationError(_)) => {
                // Already handled during registration, ignore late arrival
            }
            Some(server_message::Payload::SendMessage(send_msg)) => {
                eprintln!(
                    "→ Received message [req={}] thread={}: \"{}\"",
                    send_msg.request_id,
                    send_msg.thread_id,
                    send_msg.content.chars().take(100).collect::<String>()
                );

                // Convert to IncomingMessage
                let incoming = IncomingMessage {
                    thread_id: send_msg.thread_id.clone(),
                    sender: send_msg.sender.clone(),
                    content: send_msg.content.clone(),
                    frontend: "grpc".to_string(),
                    attachments: vec![], // TODO: handle file attachments from proto
                };

                // Spawn message processing in separate task so this loop can
                // continue receiving PackToolResult and ToolApproval messages.
                // Per-thread lock acquired first (no permit consumed while waiting),
                // then semaphore permit limits actual concurrent processing.
                let tx_clone = tx.clone();
                let request_id = send_msg.request_id.clone();
                let coven_clone = Arc::clone(&coven);
                let sem_clone = Arc::clone(&message_semaphore);
                let locks_clone = Arc::clone(&thread_locks);
                let thread_id = send_msg.thread_id.clone();
                eprintln!("  Processing with backend...");
                tokio::spawn(async move {
                    // Acquire per-thread lock first (serializes same-thread messages
                    // without consuming a semaphore permit while waiting)
                    let thread_lock = {
                        let mut locks = locks_clone.lock().await;
                        locks
                            .entry(thread_id.clone())
                            .or_insert_with(|| Arc::new(Mutex::new(())))
                            .clone()
                    };
                    let thread_guard = thread_lock.lock().await;

                    // Now acquire semaphore permit for global backpressure
                    let permit = sem_clone.acquire().await.expect("semaphore closed");

                    process_message(coven_clone, incoming, request_id, tx_clone, verbose).await;
                    eprintln!("Ready and waiting for messages...");

                    // Release guards before eviction check
                    drop(permit);
                    drop(thread_guard);
                    drop(thread_lock);

                    // Evict unused thread lock to prevent unbounded memory growth
                    let mut locks = locks_clone.lock().await;
                    if let Some(lock) = locks.get(&thread_id) {
                        if Arc::strong_count(lock) == 1 {
                            locks.remove(&thread_id);
                        }
                    }
                });
            }
            Some(server_message::Payload::Shutdown(shutdown)) => {
                eprintln!("Server requested shutdown: {}", shutdown.reason);
                break;
            }
            Some(server_message::Payload::ToolApproval(approval)) => {
                let status = if approval.approved {
                    "✓ approved"
                } else {
                    "✗ denied"
                };
                eprintln!("← Tool approval [id={}]: {}", approval.id, status);

                // Find and resolve the pending approval
                let mut pending = pending_approvals.lock().await;
                if let Some(sender) = pending.remove(&approval.id) {
                    // Send the approval decision
                    if sender.send(approval.approved).is_err() {
                        eprintln!(
                            "  WARNING: Approval receiver dropped for id={}",
                            approval.id
                        );
                    }
                } else {
                    eprintln!(
                        "  WARNING: No pending approval found for id={}",
                        approval.id
                    );
                }
            }
            Some(server_message::Payload::InjectContext(inject)) => {
                eprintln!(
                    "← Context injection [id={}]: {} chars from {:?}",
                    inject.injection_id,
                    inject.content.len(),
                    inject.source
                );
                // TODO: Implement context injection - acknowledge receipt
            }
            Some(server_message::Payload::CancelRequest(cancel)) => {
                eprintln!(
                    "← Cancel request [id={}]: {:?}",
                    cancel.request_id, cancel.reason
                );
                // TODO: Implement request cancellation
            }
            Some(server_message::Payload::PackToolResult(result)) => {
                let status = match &result.result {
                    Some(coven_proto::pack_tool_result::Result::OutputJson(_)) => "✓ success",
                    Some(coven_proto::pack_tool_result::Result::Error(_)) => "✗ error",
                    None => "? empty",
                };
                eprintln!("← Pack tool result [id={}]: {}", result.request_id, status);

                // Route result to waiting PackTool
                if !handle_pack_tool_result(&pending_pack_tools, result).await {
                    eprintln!("  WARNING: No pending pack tool request found");
                }
            }
            None => {}
        }
    }

    Ok(())
}

/// Process a single message from the gateway.
/// Runs in a spawned task so the main loop can continue receiving
/// PackToolResult and ToolApproval messages.
async fn process_message(
    coven: Arc<Coven>,
    incoming: IncomingMessage,
    request_id: String,
    tx: mpsc::Sender<AgentMessage>,
    verbose: bool,
) {
    match coven.handle(incoming).await {
        Ok(mut stream) => {
            let mut event_count = 0;
            while let Some(event) = stream.next().await {
                event_count += 1;
                log_event(event_count, &event, verbose);
                let response = convert_event_to_response(&request_id, event).await;
                if let Err(e) = tx.send(response).await {
                    eprintln!("ERROR: Failed to send response: {}", e);
                    break;
                }
            }
            eprintln!("← Response complete ({} events sent)", event_count);
        }
        Err(e) => {
            eprintln!("ERROR: Processing message: {}", e);
            // Send error event
            let error_response = AgentMessage {
                payload: Some(agent_message::Payload::Response(MessageResponse {
                    request_id: request_id.clone(),
                    event: Some(coven_proto::message_response::Event::Error(e.to_string())),
                })),
            };
            if let Err(send_err) = tx.send(error_response).await {
                eprintln!("ERROR: Failed to send error response: {}", send_err);
            }

            // Send done event to signal request completion
            let done_response = AgentMessage {
                payload: Some(agent_message::Payload::Response(MessageResponse {
                    request_id: request_id.clone(),
                    event: Some(coven_proto::message_response::Event::Done(
                        coven_proto::Done {
                            full_response: format!("Error: {}", e),
                        },
                    )),
                })),
            };
            if let Err(send_err) = tx.send(done_response).await {
                eprintln!("ERROR: Failed to send done response: {}", send_err);
            }
        }
    }
}

async fn convert_event_to_response(request_id: &str, event: OutgoingEvent) -> AgentMessage {
    use coven_proto::message_response::Event;

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
                state: outgoing_tool_state_to_proto(&state),
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

/// Log an event with meaningful content for auditing
fn log_event(n: usize, event: &OutgoingEvent, verbose: bool) {
    if verbose {
        // Normal mode: emojis and detailed output
        match event {
            OutgoingEvent::Thinking => {
                eprintln!("  [{n}] 💭 Thinking...");
            }
            OutgoingEvent::Text(text) => {
                let preview = truncate(text, 100);
                eprintln!("  [{n}] 📝 Text: {preview}");
            }
            OutgoingEvent::ToolUse { id, name, input } => {
                let input_preview = truncate(&input.to_string(), 80);
                eprintln!("  [{n}] 🔧 Tool: {name} (id={id})");
                eprintln!("        Input: {input_preview}");
            }
            OutgoingEvent::ToolResult {
                id,
                output,
                is_error,
            } => {
                let status = if *is_error { "❌" } else { "✓" };
                let output_preview = truncate(output, 80);
                eprintln!("  [{n}] {status} Result (id={id}): {output_preview}");
            }
            OutgoingEvent::Done { full_response } => {
                let len = full_response.len();
                eprintln!("  [{n}] ✅ Done ({len} chars total)");
            }
            OutgoingEvent::Error(e) => {
                eprintln!("  [{n}] ⚠️  Error: {e}");
            }
            OutgoingEvent::File { filename, path, .. } => {
                eprintln!("  [{n}] 📎 File: {filename} -> {}", path.display());
            }
            OutgoingEvent::ToolApprovalRequest { id, name, input } => {
                let input_preview = truncate(&input.to_string(), 80);
                eprintln!("  [{n}] ⏳ Approval needed: {name} (id={id})");
                eprintln!("        Input: {input_preview}");
            }
            OutgoingEvent::SessionInit { session_id } => {
                eprintln!("  [{n}] 🔗 Session: {session_id}");
            }
            OutgoingEvent::SessionOrphaned => {
                eprintln!("  [{n}] ⚠️  Session orphaned (expired)");
            }
            OutgoingEvent::Usage {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                thinking_tokens,
                ..
            } => {
                eprintln!(
                    "  [{n}] 📊 Usage: in={input_tokens} out={output_tokens} cache_read={cache_read_tokens} thinking={thinking_tokens}"
                );
            }
            OutgoingEvent::ToolState { id, state, detail } => {
                let detail_str = detail.as_deref().unwrap_or("");
                eprintln!("  [{n}] 🔄 Tool state: {id} -> {state} {detail_str}");
            }
        }
    } else {
        // Headless mode: minimal output for servers
        match event {
            OutgoingEvent::Thinking => {}
            OutgoingEvent::Text(_) => {}
            OutgoingEvent::ToolUse { name, .. } => {
                eprintln!("  tool: {name}");
            }
            OutgoingEvent::ToolResult { is_error, .. } => {
                if *is_error {
                    eprintln!("  tool: error");
                }
            }
            OutgoingEvent::Done { .. } => {}
            OutgoingEvent::Error(e) => {
                eprintln!("  error: {e}");
            }
            OutgoingEvent::File { filename, .. } => {
                eprintln!("  file: {filename}");
            }
            OutgoingEvent::ToolApprovalRequest { name, .. } => {
                eprintln!("  approval: {name}");
            }
            OutgoingEvent::SessionInit { .. } => {}
            OutgoingEvent::SessionOrphaned => {
                eprintln!("  session: orphaned");
            }
            OutgoingEvent::Usage { .. } => {}
            OutgoingEvent::ToolState { id, state, .. } => {
                eprintln!("  tool_state: {id} -> {state}");
            }
        }
    }
}

/// Convert OutgoingEvent tool state string to coven_proto ToolState enum value
fn outgoing_tool_state_to_proto(state: &str) -> i32 {
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

/// Truncate a string for display
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.replace('\n', "\\n")
    } else {
        let truncated: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", truncated.replace('\n', "\\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::time::{Duration, sleep};

    #[test]
    fn test_max_concurrent_messages_is_reasonable() {
        // Ensure the constant is a reasonable positive value
        assert!(MAX_CONCURRENT_MESSAGES > 0);
        assert!(MAX_CONCURRENT_MESSAGES <= 64);
    }

    #[tokio::test]
    async fn test_semaphore_limits_concurrency() {
        let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_MESSAGES));
        let active = Arc::new(AtomicUsize::new(0));
        let max_observed = Arc::new(AtomicUsize::new(0));

        let mut handles = vec![];
        // Spawn more tasks than the semaphore allows
        for _ in 0..(MAX_CONCURRENT_MESSAGES * 3) {
            let sem_clone = Arc::clone(&sem);
            let active_clone = Arc::clone(&active);
            let max_clone = Arc::clone(&max_observed);
            handles.push(tokio::spawn(async move {
                let _permit = sem_clone.acquire().await.unwrap();
                let current = active_clone.fetch_add(1, Ordering::SeqCst) + 1;
                // Update max observed concurrency
                max_clone.fetch_max(current, Ordering::SeqCst);
                // Simulate work
                sleep(Duration::from_millis(5)).await;
                active_clone.fetch_sub(1, Ordering::SeqCst);
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        let max = max_observed.load(Ordering::SeqCst);
        assert!(
            max <= MAX_CONCURRENT_MESSAGES,
            "observed {} concurrent tasks, limit is {}",
            max,
            MAX_CONCURRENT_MESSAGES
        );
    }

    #[tokio::test]
    async fn test_thread_locks_serialize_same_thread() {
        let thread_locks: ThreadLocks = Arc::new(Mutex::new(HashMap::new()));
        let execution_order = Arc::new(Mutex::new(Vec::<usize>::new()));

        let mut handles = vec![];
        // Spawn 5 tasks all targeting the same thread_id
        for i in 0..5 {
            let locks_clone = Arc::clone(&thread_locks);
            let order_clone = Arc::clone(&execution_order);
            handles.push(tokio::spawn(async move {
                let thread_lock = {
                    let mut locks = locks_clone.lock().await;
                    locks
                        .entry("same-thread".to_string())
                        .or_insert_with(|| Arc::new(Mutex::new(())))
                        .clone()
                };
                let _guard = thread_lock.lock().await;

                // Record execution start and do work
                order_clone.lock().await.push(i);
                sleep(Duration::from_millis(10)).await;
            }));
            // Small delay to ensure spawn order matches intended order
            sleep(Duration::from_millis(1)).await;
        }

        for h in handles {
            h.await.unwrap();
        }

        // All 5 tasks should have executed (serialized, not concurrent)
        let order = execution_order.lock().await;
        assert_eq!(order.len(), 5, "all tasks should complete");
    }

    #[tokio::test]
    async fn test_thread_locks_allow_different_threads_concurrently() {
        let thread_locks: ThreadLocks = Arc::new(Mutex::new(HashMap::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let max_observed = Arc::new(AtomicUsize::new(0));

        let mut handles = vec![];
        // Spawn tasks each targeting different thread_ids
        for i in 0..4 {
            let locks_clone = Arc::clone(&thread_locks);
            let active_clone = Arc::clone(&active);
            let max_clone = Arc::clone(&max_observed);
            handles.push(tokio::spawn(async move {
                let thread_id = format!("thread-{}", i);
                let thread_lock = {
                    let mut locks = locks_clone.lock().await;
                    locks
                        .entry(thread_id)
                        .or_insert_with(|| Arc::new(Mutex::new(())))
                        .clone()
                };
                let _guard = thread_lock.lock().await;

                let current = active_clone.fetch_add(1, Ordering::SeqCst) + 1;
                max_clone.fetch_max(current, Ordering::SeqCst);
                sleep(Duration::from_millis(20)).await;
                active_clone.fetch_sub(1, Ordering::SeqCst);
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        // Different threads should run concurrently (max > 1)
        let max = max_observed.load(Ordering::SeqCst);
        assert!(
            max > 1,
            "expected concurrent execution for different threads, max was {}",
            max
        );
    }

    #[tokio::test]
    async fn test_thread_locks_eviction() {
        let thread_locks: ThreadLocks = Arc::new(Mutex::new(HashMap::new()));

        // Simulate a task that acquires lock, does work, then evicts
        let locks_clone = Arc::clone(&thread_locks);
        let thread_id = "evict-me".to_string();

        let handle = tokio::spawn({
            let thread_id = thread_id.clone();
            let locks_clone = locks_clone.clone();
            async move {
                let thread_lock = {
                    let mut locks = locks_clone.lock().await;
                    locks
                        .entry(thread_id.clone())
                        .or_insert_with(|| Arc::new(Mutex::new(())))
                        .clone()
                };
                let guard = thread_lock.lock().await;

                // Simulate work
                sleep(Duration::from_millis(5)).await;

                // Release and evict
                drop(guard);
                drop(thread_lock);

                let mut locks = locks_clone.lock().await;
                if let Some(lock) = locks.get(&thread_id) {
                    if Arc::strong_count(lock) == 1 {
                        locks.remove(&thread_id);
                    }
                }
            }
        });

        handle.await.unwrap();

        // After eviction, the map should be empty
        let locks = thread_locks.lock().await;
        assert!(
            !locks.contains_key("evict-me"),
            "thread lock should be evicted after use"
        );
    }

    #[tokio::test]
    async fn test_thread_locks_no_eviction_while_in_use() {
        let thread_locks: ThreadLocks = Arc::new(Mutex::new(HashMap::new()));

        // Task 1 acquires lock and holds it
        let locks_clone = Arc::clone(&thread_locks);
        let thread_id = "busy-thread".to_string();

        // Pre-create the lock entry and hold a reference
        let held_lock = {
            let mut locks = locks_clone.lock().await;
            locks
                .entry(thread_id.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _held_guard = held_lock.lock().await;

        // Simulate another task trying to evict - should NOT remove because strong_count > 1
        {
            let mut locks = locks_clone.lock().await;
            if let Some(lock) = locks.get(&thread_id) {
                if Arc::strong_count(lock) == 1 {
                    locks.remove(&thread_id);
                }
            }
        }

        // Lock should still be in the map
        let locks = thread_locks.lock().await;
        assert!(
            locks.contains_key("busy-thread"),
            "thread lock should NOT be evicted while in use"
        );
    }
}
