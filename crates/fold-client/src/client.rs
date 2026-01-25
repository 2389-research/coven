// ABOUTME: Main FoldClient implementation using gRPC
// ABOUTME: Provides async API for gateway communication (used by both FFI and native Rust)

use crate::error::FoldError;
use crate::models::*;
use crate::{StateCallback, StreamCallback};
use fold_grpc::{create_channel, ChannelConfig};
use fold_proto::client::ClientServiceClient;
use fold_proto::{
    client_stream_event, ClientSendMessageRequest, ClientStreamEvent, GetEventsRequest,
    ListAgentsRequest, StreamEventsRequest,
};
use fold_ssh::{load_or_generate_key, PrivateKey, SshAuthCredentials};
use futures::StreamExt;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;

/// Counter for generating unique idempotency keys
static IDEMPOTENCY_COUNTER: AtomicU64 = AtomicU64::new(0);

fn generate_idempotency_key() -> String {
    let count = IDEMPOTENCY_COUNTER.fetch_add(1, Ordering::SeqCst);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("fold-client-{}-{}", timestamp, count)
}

/// Internal state for an active stream
struct ActiveStream {
    cancel: CancellationToken,
    buffer: String,
    agent_name: String,
}

/// Internal client state
struct ClientState {
    // Agents
    agents: Vec<Agent>,
    connection_status: ConnectionStatus,

    // Per-agent state (keyed by conversation_key which is typically agent_id)
    messages: HashMap<String, Vec<Message>>,
    queues: HashMap<String, Vec<String>>,
    unread: HashMap<String, u32>,

    // Active streams (keyed by conversation_key)
    streams: HashMap<String, ActiveStream>,

    // Callbacks
    stream_callback: Option<Box<dyn StreamCallback>>,
    state_callback: Option<Box<dyn StateCallback>>,

    // Session usage
    session_usage: UsageInfo,
}

/// The main fold gateway client (gRPC-based)
///
/// All methods are truly async - safe to call from any async context.
/// The client maintains its own Tokio runtime for FFI callers (iOS/Swift)
/// since UniFFI's async support doesn't provide the full runtime needed by hyper.
pub struct FoldClient {
    gateway_url: String,
    state: Arc<RwLock<ClientState>>,
    /// Tokio runtime for async operations (needed for hyper's I/O reactor)
    /// Wrapped in Option to allow graceful shutdown in Drop
    runtime: Option<tokio::runtime::Runtime>,
    /// Optional SSH private key for authentication
    ssh_key: Option<Arc<PrivateKey>>,
}

impl FoldClient {
    /// Create a new client connected to the gateway (without authentication)
    pub fn new(gateway_url: String) -> Self {
        Self::new_internal(gateway_url, None)
    }

    /// Create a new client with SSH key authentication
    ///
    /// Loads (or generates) the SSH key from the given path and uses it
    /// to sign all gRPC requests to the gateway.
    ///
    /// # Security Note
    /// When SSH auth is enabled, using TLS (https://) is strongly recommended
    /// to protect signed headers from observation and potential replay attacks.
    /// A warning is logged if the gateway URL uses plaintext HTTP.
    pub fn new_with_auth(gateway_url: String, ssh_key_path: &Path) -> Result<Self, FoldError> {
        Self::warn_if_insecure_with_auth(&gateway_url);
        let key = load_or_generate_key(ssh_key_path)
            .map_err(|e| FoldError::Connection(format!("failed to load SSH key: {}", e)))?;
        Ok(Self::new_internal(gateway_url, Some(Arc::new(key))))
    }

    /// Create a new client with a pre-loaded SSH private key
    ///
    /// # Security Note
    /// When SSH auth is enabled, using TLS (https://) is strongly recommended.
    pub fn new_with_key(gateway_url: String, key: PrivateKey) -> Self {
        Self::warn_if_insecure_with_auth(&gateway_url);
        Self::new_internal(gateway_url, Some(Arc::new(key)))
    }

    /// Warn if SSH auth is used over non-TLS connection
    fn warn_if_insecure_with_auth(gateway_url: &str) {
        // Parse URL to properly check the host, avoiding substring matching issues
        // (e.g., "localhost.evil.com" should NOT be treated as localhost)
        let url_lower = gateway_url.to_lowercase();
        if !url_lower.starts_with("http://") {
            return; // TLS enabled, no warning needed
        }

        // Parse to extract the actual host.
        // url::Url::host_str() returns IPv6 addresses without brackets.
        let is_loopback = if let Ok(url) = url::Url::parse(gateway_url) {
            matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
        } else {
            false
        };

        if !is_loopback {
            tracing::warn!(
                "SSH auth over non-TLS connection to {}. \
                 Signed headers could be observed and potentially replayed. \
                 Consider using https:// for production.",
                gateway_url
            );
        }
    }

    fn new_internal(gateway_url: String, ssh_key: Option<Arc<PrivateKey>>) -> Self {
        // Create a multi-threaded Tokio runtime with all features enabled
        // This is needed because hyper's HTTP client requires a full runtime
        // with I/O reactor, and UniFFI's async executor doesn't provide this.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime");

        Self {
            gateway_url,
            state: Arc::new(RwLock::new(ClientState {
                agents: vec![],
                connection_status: ConnectionStatus::Connecting,
                messages: HashMap::new(),
                queues: HashMap::new(),
                unread: HashMap::new(),
                streams: HashMap::new(),
                stream_callback: None,
                state_callback: None,
                session_usage: UsageInfo::default(),
            })),
            runtime: Some(runtime),
            ssh_key,
        }
    }

    /// Create an SSH auth interceptor closure
    ///
    /// Local interceptor errors are marked with `x-fold-local-error` metadata
    /// to reliably distinguish them from server responses.
    fn make_ssh_interceptor(
        key: Arc<PrivateKey>,
    ) -> impl Fn(tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> + Clone + Send + Sync
    {
        move |mut req: tonic::Request<()>| {
            match SshAuthCredentials::new(&key) {
                Ok(creds) => {
                    if let Err(e) = creds.apply_to_request(&mut req) {
                        return Err(Self::local_interceptor_error(format!(
                            "failed to apply SSH auth: {}",
                            e
                        )));
                    }
                }
                Err(e) => {
                    return Err(Self::local_interceptor_error(format!(
                        "failed to create SSH auth credentials: {}",
                        e
                    )));
                }
            }
            Ok(req)
        }
    }

    /// Create a tonic::Status for local interceptor errors with a marker metadata
    fn local_interceptor_error(message: String) -> tonic::Status {
        let mut status = tonic::Status::internal(message);
        // Add marker metadata to distinguish local errors from server errors
        status
            .metadata_mut()
            .insert("x-fold-local-error", "true".parse().unwrap());
        status
    }

    /// Create a gRPC channel
    async fn create_channel_internal(&self) -> Result<Channel, FoldError> {
        let config = ChannelConfig::new(&self.gateway_url);
        create_channel(&config)
            .await
            .map_err(|e| FoldError::Connection(e.to_string()))
    }

    /// Get reference to the internal runtime (panics if runtime was shut down)
    fn runtime(&self) -> &tokio::runtime::Runtime {
        self.runtime
            .as_ref()
            .expect("FoldClient runtime was shut down")
    }

    /// Set callback for stream events (synchronous - safe to call without runtime)
    pub fn set_stream_callback(&self, callback: Box<dyn StreamCallback>) {
        self.state.write().expect("lock poisoned").stream_callback = Some(callback);
    }

    /// Set callback for state changes (synchronous - safe to call without runtime)
    pub fn set_state_callback(&self, callback: Box<dyn StateCallback>) {
        self.state.write().expect("lock poisoned").state_callback = Some(callback);
    }

    // =========================================================================
    // Connection & Agents
    // =========================================================================

    /// Check gateway health by attempting to get identity
    ///
    /// Any gRPC response (even auth errors) proves the connection works.
    /// Only channel creation failure indicates a real connection problem.
    pub fn check_health(&self) -> Result<(), FoldError> {
        self.runtime().block_on(self.check_health_async())
    }

    /// Async implementation of check_health - use this from async contexts
    pub async fn check_health_async(&self) -> Result<(), FoldError> {
        let channel = self.create_channel_internal().await?;

        let result = if let Some(ref key) = self.ssh_key {
            let mut client = ClientServiceClient::with_interceptor(
                channel,
                Self::make_ssh_interceptor(key.clone()),
            );
            client.get_me(()).await
        } else {
            let mut client = ClientServiceClient::new(channel);
            client.get_me(()).await
        };

        // Distinguish between local interceptor errors and server responses
        match &result {
            Ok(_) => {
                // Successful response - definitely connected
                self.set_connection_status(ConnectionStatus::Connected);
            }
            Err(status) => {
                // Check if this is a local interceptor error vs a server response.
                // Local interceptor errors use Code::Internal + a metadata marker.
                // Both conditions must hold to prevent a malicious server from
                // spoofing the marker and triggering the wrong error path.
                let is_local_interceptor_error = status.code() == tonic::Code::Internal
                    && status.metadata().get("x-fold-local-error").is_some();

                if is_local_interceptor_error {
                    // Local interceptor failure - set Disconnected and return error
                    self.set_connection_status(ConnectionStatus::Disconnected);
                    tracing::warn!("check_health: local interceptor failed: {}", status);
                    return Err(FoldError::Connection(format!(
                        "SSH auth failed: {}",
                        status.message()
                    )));
                }
                // Server responded (even with auth error) - connection is healthy
                self.set_connection_status(ConnectionStatus::Connected);
                tracing::debug!("check_health got gRPC error (connection OK): {}", status);
            }
        }

        Ok(())
    }

    /// Helper to update connection status and notify callback
    fn set_connection_status(&self, status: ConnectionStatus) {
        let mut state_guard = self.state.write().expect("lock poisoned");
        state_guard.connection_status = status;
        if let Some(cb) = &state_guard.state_callback {
            cb.on_connection_status(status);
        }
    }

    /// Fetch available agents from gateway
    pub fn refresh_agents(&self) -> Result<Vec<Agent>, FoldError> {
        self.runtime().block_on(self.refresh_agents_async())
    }

    /// Async implementation of refresh_agents - use this from async contexts
    pub async fn refresh_agents_async(&self) -> Result<Vec<Agent>, FoldError> {
        let channel = self.create_channel_internal().await?;

        let response = if let Some(ref key) = self.ssh_key {
            let mut client = ClientServiceClient::with_interceptor(
                channel,
                Self::make_ssh_interceptor(key.clone()),
            );
            client
                .list_agents(ListAgentsRequest { workspace: None })
                .await
                .map_err(|e| FoldError::Api(e.to_string()))?
        } else {
            let mut client = ClientServiceClient::new(channel);
            client
                .list_agents(ListAgentsRequest { workspace: None })
                .await
                .map_err(|e| FoldError::Api(e.to_string()))?
        };

        let agents: Vec<Agent> = response
            .into_inner()
            .agents
            .into_iter()
            .map(Agent::from_proto)
            .collect();

        let mut state_guard = self.state.write().expect("lock poisoned");
        state_guard.agents = agents.clone();
        state_guard.connection_status = ConnectionStatus::Connected;
        if let Some(cb) = &state_guard.state_callback {
            cb.on_connection_status(state_guard.connection_status);
        }

        Ok(agents)
    }

    /// Get cached agents list
    pub fn get_agents(&self) -> Vec<Agent> {
        self.state.read().expect("lock poisoned").agents.clone()
    }

    /// Get a specific agent by ID
    pub fn get_agent(&self, agent_id: String) -> Option<Agent> {
        self.state
            .read()
            .expect("lock poisoned")
            .agents
            .iter()
            .find(|a| a.id == agent_id)
            .cloned()
    }

    // =========================================================================
    // Messages & History
    // =========================================================================

    /// Get messages for an agent (from cache)
    pub fn get_messages(&self, agent_id: String) -> Vec<Message> {
        self.state
            .read()
            .expect("lock poisoned")
            .messages
            .get(&agent_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Load history from server for a conversation
    pub fn load_history(&self, agent_id: String) -> Result<Vec<Message>, FoldError> {
        self.runtime().block_on(self.load_history_async(agent_id))
    }

    /// Async implementation of load_history - use this from async contexts
    pub async fn load_history_async(&self, agent_id: String) -> Result<Vec<Message>, FoldError> {
        // Get agent name for message attribution
        let agent_name = {
            let state_guard = self.state.read().expect("lock poisoned");
            state_guard
                .agents
                .iter()
                .find(|a| a.id == agent_id)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "Agent".into())
        };

        let channel = self.create_channel_internal().await?;

        // Use agent_id as conversation_key
        let request = GetEventsRequest {
            conversation_key: agent_id.clone(),
            since: None,
            until: None,
            limit: Some(500),
            cursor: None,
        };

        let response = if let Some(ref key) = self.ssh_key {
            let mut client = ClientServiceClient::with_interceptor(
                channel,
                Self::make_ssh_interceptor(key.clone()),
            );
            client
                .get_events(request)
                .await
                .map_err(|e| FoldError::Api(e.to_string()))?
        } else {
            let mut client = ClientServiceClient::new(channel);
            client
                .get_events(request)
                .await
                .map_err(|e| FoldError::Api(e.to_string()))?
        };

        let events = response.into_inner().events;
        let messages: Vec<Message> = events
            .into_iter()
            .filter_map(|e| Message::from_event(e, &agent_name))
            .collect();

        // Cache the messages
        let mut state_guard = self.state.write().expect("lock poisoned");
        state_guard
            .messages
            .insert(agent_id.clone(), messages.clone());
        if let Some(cb) = &state_guard.state_callback {
            cb.on_messages_changed(agent_id.clone());
        }

        Ok(messages)
    }

    /// Get unread count for an agent
    pub fn get_unread(&self, agent_id: String) -> u32 {
        self.state
            .read()
            .expect("lock poisoned")
            .unread
            .get(&agent_id)
            .copied()
            .unwrap_or(0)
    }

    /// Clear unread count (when user views agent)
    pub fn clear_unread(&self, agent_id: String) {
        let mut state = self.state.write().expect("lock poisoned");
        state.unread.insert(agent_id.clone(), 0);
        if let Some(cb) = &state.state_callback {
            cb.on_unread_changed(agent_id, 0);
        }
    }

    // =========================================================================
    // Sending Messages
    // =========================================================================

    /// Send a message to an agent (starts streaming response)
    pub fn send_message(&self, agent_id: String, content: String) -> Result<(), FoldError> {
        let mut state_guard = self.state.write().expect("lock poisoned");

        // Check if already streaming to this agent
        if state_guard.streams.contains_key(&agent_id) {
            // Queue the message
            state_guard
                .queues
                .entry(agent_id.clone())
                .or_default()
                .push(content);

            let count = state_guard
                .queues
                .get(&agent_id)
                .map(|q| q.len() as u32)
                .unwrap_or(0);
            if let Some(cb) = &state_guard.state_callback {
                cb.on_queue_changed(agent_id.clone(), count);
            }
            return Ok(());
        }

        // Get agent info
        let agent = state_guard
            .agents
            .iter()
            .find(|a| a.id == agent_id)
            .cloned()
            .ok_or_else(|| FoldError::AgentNotFound(agent_id.clone()))?;

        // Add user message to history
        let user_msg = Message::user(content.clone());
        state_guard
            .messages
            .entry(agent_id.clone())
            .or_default()
            .push(user_msg);
        if let Some(cb) = &state_guard.state_callback {
            cb.on_messages_changed(agent_id.clone());
        }

        // Create cancellation token for this stream
        let cancel = CancellationToken::new();
        state_guard.streams.insert(
            agent_id.clone(),
            ActiveStream {
                cancel: cancel.clone(),
                buffer: String::new(),
                agent_name: agent.name.clone(),
            },
        );
        if let Some(cb) = &state_guard.state_callback {
            cb.on_streaming_changed(agent_id.clone(), true);
        }

        drop(state_guard);

        // Start streaming in background task using our runtime
        let state_clone = self.state.clone();
        let gateway_url = self.gateway_url.clone();
        let agent_id_clone = agent_id.clone();
        let ssh_key = self.ssh_key.clone();
        self.runtime().spawn(async move {
            Self::run_grpc_stream(
                state_clone,
                gateway_url,
                agent_id_clone,
                content,
                cancel,
                ssh_key,
            )
            .await;
        });

        Ok(())
    }

    /// Check if an agent is currently streaming
    pub fn is_streaming(&self, agent_id: String) -> bool {
        self.state
            .read()
            .expect("lock poisoned")
            .streams
            .contains_key(&agent_id)
    }

    /// Get current stream buffer content
    pub fn get_stream_buffer(&self, agent_id: String) -> String {
        self.state
            .read()
            .expect("lock poisoned")
            .streams
            .get(&agent_id)
            .map(|s| s.buffer.clone())
            .unwrap_or_default()
    }

    /// Cancel an active stream
    pub fn cancel_stream(&self, agent_id: String) {
        let state = self.state.read().expect("lock poisoned");
        if let Some(stream) = state.streams.get(&agent_id) {
            stream.cancel.cancel();
        }
    }

    // =========================================================================
    // Queue Management
    // =========================================================================

    /// Get queued message count for an agent
    pub fn get_queue_count(&self, agent_id: String) -> u32 {
        self.state
            .read()
            .expect("lock poisoned")
            .queues
            .get(&agent_id)
            .map(|q| q.len() as u32)
            .unwrap_or(0)
    }

    // =========================================================================
    // Session Stats
    // =========================================================================

    /// Get accumulated token usage for the session
    pub fn get_session_usage(&self) -> UsageInfo {
        self.state
            .read()
            .expect("lock poisoned")
            .session_usage
            .clone()
    }

    // =========================================================================
    // Internal Streaming Implementation
    // =========================================================================

    /// Run the gRPC streaming request
    async fn run_grpc_stream(
        state: Arc<RwLock<ClientState>>,
        gateway_url: String,
        agent_id: String,
        content: String,
        cancel: CancellationToken,
        ssh_key: Option<Arc<PrivateKey>>,
    ) {
        // Create channel
        let config = ChannelConfig::new(&gateway_url);
        let channel = match create_channel(&config).await {
            Ok(c) => c,
            Err(e) => {
                Self::handle_stream_error(&state, &agent_id, e.to_string());
                return;
            }
        };

        // Run with or without auth based on SSH key presence
        if let Some(key) = ssh_key {
            Self::run_grpc_stream_with_auth(state, channel, agent_id, content, cancel, key).await;
        } else {
            Self::run_grpc_stream_no_auth(state, channel, agent_id, content, cancel).await;
        }
    }

    /// Run the gRPC streaming request with SSH auth
    async fn run_grpc_stream_with_auth(
        state: Arc<RwLock<ClientState>>,
        channel: Channel,
        agent_id: String,
        content: String,
        cancel: CancellationToken,
        key: Arc<PrivateKey>,
    ) {
        let mut client =
            ClientServiceClient::with_interceptor(channel, Self::make_ssh_interceptor(key));

        // Send the message
        let send_request = ClientSendMessageRequest {
            conversation_key: agent_id.clone(),
            content,
            attachments: vec![],
            idempotency_key: generate_idempotency_key(),
        };

        if let Err(e) = client.send_message(send_request).await {
            Self::handle_stream_error(&state, &agent_id, e.to_string());
            return;
        }

        // Start streaming events
        let stream_request = StreamEventsRequest {
            conversation_key: agent_id.clone(),
            since_event_id: None,
        };

        let stream = match client.stream_events(stream_request).await {
            Ok(response) => response.into_inner(),
            Err(e) => {
                Self::handle_stream_error(&state, &agent_id, e.to_string());
                return;
            }
        };

        tokio::pin!(stream);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    break;
                }
                event = stream.next() => {
                    match event {
                        Some(Ok(stream_event)) => {
                            let (is_done, is_error) =
                                Self::handle_grpc_stream_event(&state, &agent_id, stream_event).await;

                            if is_done || is_error {
                                break;
                            }
                        }
                        Some(Err(e)) => {
                            Self::handle_stream_error(&state, &agent_id, e.to_string());
                            break;
                        }
                        None => {
                            // Stream ended
                            Self::finalize_stream(&state, &agent_id);
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Run the gRPC streaming request without auth
    async fn run_grpc_stream_no_auth(
        state: Arc<RwLock<ClientState>>,
        channel: Channel,
        agent_id: String,
        content: String,
        cancel: CancellationToken,
    ) {
        let mut client = ClientServiceClient::new(channel);

        // Send the message
        let send_request = ClientSendMessageRequest {
            conversation_key: agent_id.clone(),
            content,
            attachments: vec![],
            idempotency_key: generate_idempotency_key(),
        };

        if let Err(e) = client.send_message(send_request).await {
            Self::handle_stream_error(&state, &agent_id, e.to_string());
            return;
        }

        // Start streaming events
        let stream_request = StreamEventsRequest {
            conversation_key: agent_id.clone(),
            since_event_id: None,
        };

        let stream = match client.stream_events(stream_request).await {
            Ok(response) => response.into_inner(),
            Err(e) => {
                Self::handle_stream_error(&state, &agent_id, e.to_string());
                return;
            }
        };

        tokio::pin!(stream);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    break;
                }
                event = stream.next() => {
                    match event {
                        Some(Ok(stream_event)) => {
                            let (is_done, is_error) =
                                Self::handle_grpc_stream_event(&state, &agent_id, stream_event).await;

                            if is_done || is_error {
                                break;
                            }
                        }
                        Some(Err(e)) => {
                            Self::handle_stream_error(&state, &agent_id, e.to_string());
                            break;
                        }
                        None => {
                            // Stream ended
                            Self::finalize_stream(&state, &agent_id);
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Handle a gRPC stream event, returns (is_done, is_error)
    async fn handle_grpc_stream_event(
        state: &Arc<RwLock<ClientState>>,
        agent_id: &str,
        event: ClientStreamEvent,
    ) -> (bool, bool) {
        let mut state_guard = state.write().expect("lock poisoned");

        let stream_event = match event.payload {
            Some(client_stream_event::Payload::Text(chunk)) => {
                // Accumulate text to buffer
                if let Some(stream) = state_guard.streams.get_mut(agent_id) {
                    stream.buffer.push_str(&chunk.content);
                }
                StreamEvent::Text {
                    content: chunk.content,
                }
            }
            Some(client_stream_event::Payload::Thinking(chunk)) => StreamEvent::Thinking {
                content: chunk.content,
            },
            Some(client_stream_event::Payload::ToolUse(tool)) => StreamEvent::ToolUse {
                name: tool.name,
                input: tool.input_json,
            },
            Some(client_stream_event::Payload::ToolResult(result)) => StreamEvent::ToolResult {
                tool_id: result.id,
                result: result.output,
            },
            Some(client_stream_event::Payload::ToolState(update)) => {
                let state_str = match update.state {
                    0 => "unspecified",
                    1 => "pending",
                    2 => "awaiting_approval",
                    3 => "running",
                    4 => "completed",
                    5 => "failed",
                    6 => "denied",
                    7 => "timeout",
                    8 => "cancelled",
                    _ => "unknown",
                };
                StreamEvent::ToolState {
                    state: state_str.to_string(),
                    detail: update.detail.unwrap_or_default(),
                }
            }
            Some(client_stream_event::Payload::Usage(usage)) => {
                let info = UsageInfo {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    cache_read_tokens: usage.cache_read_tokens,
                    cache_write_tokens: usage.cache_write_tokens,
                    thinking_tokens: usage.thinking_tokens,
                };
                state_guard.session_usage.accumulate(&info);
                StreamEvent::Usage { info }
            }
            Some(client_stream_event::Payload::Done(_)) => {
                Self::finalize_stream_internal(&mut state_guard, agent_id);
                // Notify callback before returning
                if let Some(cb) = &state_guard.stream_callback {
                    cb.on_event(agent_id.to_string(), StreamEvent::Done);
                }
                return (true, false);
            }
            Some(client_stream_event::Payload::Error(err)) => {
                let error_event = StreamEvent::Error {
                    message: err.message.clone(),
                };

                // Add error message to history
                let error_msg = Message::system(format!("Error: {}", err.message));
                state_guard
                    .messages
                    .entry(agent_id.to_string())
                    .or_default()
                    .push(error_msg);

                // Cleanup stream state
                state_guard.streams.remove(agent_id);

                // Notify callbacks
                if let Some(cb) = &state_guard.stream_callback {
                    cb.on_event(agent_id.to_string(), error_event);
                }
                if let Some(cb) = &state_guard.state_callback {
                    cb.on_streaming_changed(agent_id.to_string(), false);
                    cb.on_messages_changed(agent_id.to_string());
                }
                return (false, true);
            }
            Some(client_stream_event::Payload::Event(_ledger_event)) => {
                // Full event from history replay - skip for now
                return (false, false);
            }
            None => return (false, false),
        };

        // Notify callback
        if let Some(cb) = &state_guard.stream_callback {
            cb.on_event(agent_id.to_string(), stream_event);
        }

        (false, false)
    }

    fn handle_stream_error(state: &Arc<RwLock<ClientState>>, agent_id: &str, error: String) {
        let mut state_guard = state.write().expect("lock poisoned");

        // Notify via callback
        if let Some(cb) = &state_guard.stream_callback {
            cb.on_event(
                agent_id.to_string(),
                StreamEvent::Error {
                    message: error.clone(),
                },
            );
        }

        // Add error message to history
        let error_msg = Message::system(format!("Error: {}", error));
        state_guard
            .messages
            .entry(agent_id.to_string())
            .or_default()
            .push(error_msg);

        // Cleanup stream state
        state_guard.streams.remove(agent_id);

        // Notify state change
        if let Some(cb) = &state_guard.state_callback {
            cb.on_streaming_changed(agent_id.to_string(), false);
            cb.on_messages_changed(agent_id.to_string());
        }
    }

    fn finalize_stream(state: &Arc<RwLock<ClientState>>, agent_id: &str) {
        let mut state_guard = state.write().expect("lock poisoned");
        Self::finalize_stream_internal(&mut state_guard, agent_id);
    }

    fn finalize_stream_internal(state: &mut ClientState, agent_id: &str) {
        // Get and remove stream state
        let stream = match state.streams.remove(agent_id) {
            Some(s) => s,
            None => return,
        };

        // Save agent message if there's content
        if !stream.buffer.is_empty() {
            let agent_msg = Message::agent(stream.agent_name, stream.buffer);
            state
                .messages
                .entry(agent_id.to_string())
                .or_default()
                .push(agent_msg);
        }

        // Notify callbacks
        if let Some(cb) = &state.state_callback {
            cb.on_streaming_changed(agent_id.to_string(), false);
            cb.on_messages_changed(agent_id.to_string());
        }

        // Process queue - concatenate and send
        if let Some(queued) = state.queues.remove(agent_id) {
            if !queued.is_empty() {
                let combined = queued.join("\n");
                // Re-queue for sending
                state
                    .queues
                    .entry(agent_id.to_string())
                    .or_default()
                    .push(combined);

                if let Some(cb) = &state.state_callback {
                    cb.on_queue_changed(agent_id.to_string(), 1);
                }
            }
        }
    }
}

impl Drop for FoldClient {
    fn drop(&mut self) {
        // Take ownership of the runtime to shut it down
        if let Some(runtime) = self.runtime.take() {
            // Use shutdown_background to avoid blocking in async contexts
            // This allows the runtime's background tasks to finish gracefully
            runtime.shutdown_background();
        }
    }
}
