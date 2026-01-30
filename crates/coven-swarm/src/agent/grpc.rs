// ABOUTME: gRPC client for connecting to coven-gateway.
// ABOUTME: Handles registration, message receiving, and real-time response streaming.

use anyhow::{Context, Result};
use std::time::Duration;
use tokio::sync::mpsc;
use tonic::service::Interceptor;
use tonic::transport::Channel;

// Use shared proto types from coven-proto
pub use coven_proto::coven;

use coven_proto::client::CovenControlClient;
use coven_proto::{AgentMessage, AgentMetadata, RegisterAgent, ToolDefinition};

use super::pack_tool::{handle_pack_tool_result, PendingPackTools};

/// Load auth token from ~/.config/coven/token
fn load_token() -> Result<String> {
    let token_path = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?
        .join(".config/coven/token");

    let token = std::fs::read_to_string(&token_path)
        .with_context(|| format!("No coven token found at {}. Run 'coven link' first.", token_path.display()))?
        .trim()
        .to_string();

    if token.is_empty() {
        anyhow::bail!("Coven token file is empty. Run 'coven link' first.");
    }

    Ok(token)
}

/// Auth interceptor that adds Bearer token to requests
#[derive(Clone)]
struct AuthInterceptor {
    token: String,
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> std::result::Result<tonic::Request<()>, tonic::Status> {
        let auth_value = format!("Bearer {}", self.token)
            .parse()
            .map_err(|_| tonic::Status::internal("invalid token format"))?;
        req.metadata_mut().insert("authorization", auth_value);
        Ok(req)
    }
}

/// Sender for streaming responses back to the gateway in real-time.
/// Clone this and use it to send responses as they arrive from the backend.
pub type ResponseSender = mpsc::Sender<coven::MessageResponse>;

/// Information extracted from Welcome message for configuring pack tools
#[derive(Debug, Clone)]
pub struct WelcomeInfo {
    pub agent_id: String,
    pub instance_id: String,
    pub mcp_endpoint: String,
    pub mcp_token: String,
    pub available_tools: Vec<ToolDefinition>,
}

impl WelcomeInfo {
    /// Build the full MCP URL with token for direct-cli backends
    pub fn mcp_url(&self) -> Option<String> {
        if self.mcp_endpoint.is_empty() || self.mcp_token.is_empty() {
            return None;
        }
        // Build URL: endpoint?token=xxx or endpoint&token=xxx if already has query params
        let separator = if self.mcp_endpoint.contains('?') {
            '&'
        } else {
            '?'
        };
        Some(format!(
            "{}{}token={}",
            self.mcp_endpoint, separator, self.mcp_token
        ))
    }
}

pub fn format_agent_id(prefix: &str, workspace: &str) -> String {
    format!("{}_{}", prefix, workspace)
}

pub struct GatewayClient {
    client: CovenControlClient<tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>>,
    agent_id: String,
    workspace: String,
    working_dir: String,
    backend: String,
}

impl GatewayClient {
    pub async fn connect(
        gateway_url: &str,
        prefix: &str,
        workspace: &str,
        working_dir: &str,
        backend: &str,
    ) -> Result<Self> {
        // Load auth token
        let token = load_token()?;

        let channel = Channel::from_shared(gateway_url.to_string())?
            .http2_keep_alive_interval(Duration::from_secs(10))
            .keep_alive_timeout(Duration::from_secs(20))
            .keep_alive_while_idle(true)
            .connect()
            .await
            .context("Failed to connect to coven-gateway")?;

        let interceptor = AuthInterceptor { token };
        let client = CovenControlClient::with_interceptor(channel, interceptor);
        let agent_id = format_agent_id(prefix, workspace);

        Ok(Self {
            client,
            agent_id,
            workspace: workspace.to_string(),
            working_dir: working_dir.to_string(),
            backend: backend.to_string(),
        })
    }

    /// Run the agent, handling messages from the gateway.
    ///
    /// The handler receives messages and a ResponseSender to stream responses
    /// back to the gateway in real-time (not batched).
    ///
    /// For pack tool support, use `run_with_pack_tools()` instead.
    pub async fn run<F, Fut>(self, message_handler: F) -> Result<()>
    where
        F: FnMut(coven::SendMessage, ResponseSender) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        // Run without pack tool support
        self.run_with_pack_tools(message_handler, None, |_, _| {})
            .await
    }

    /// Run the agent with pack tool support.
    ///
    /// - `message_handler`: Handles incoming messages
    /// - `pending_pack_tools`: Optional pending pack tools registry for gRPC-routed pack tools (mux backend)
    /// - `on_welcome`: Callback with Welcome info for configuring backends (e.g., setting MCP endpoint)
    ///
    /// The `on_welcome` callback receives WelcomeInfo and a sender for ExecutePackTool messages.
    /// For direct-cli backends, use WelcomeInfo::mcp_url() to configure the backend.
    /// For mux backends, register PackTool instances using the provided sender.
    pub async fn run_with_pack_tools<F, Fut, W>(
        mut self,
        mut message_handler: F,
        pending_pack_tools: Option<PendingPackTools>,
        on_welcome: W,
    ) -> Result<()>
    where
        F: FnMut(coven::SendMessage, ResponseSender) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
        W: FnOnce(WelcomeInfo, mpsc::Sender<AgentMessage>),
    {
        let (tx, rx) = mpsc::channel::<AgentMessage>(32);
        let outbound = tokio_stream::wrappers::ReceiverStream::new(rx);

        // Channel for responses from handlers - these get forwarded to the gRPC stream
        let (resp_tx, mut resp_rx) = mpsc::channel::<coven::MessageResponse>(64);

        // Send registration
        let register = AgentMessage {
            payload: Some(coven::agent_message::Payload::Register(RegisterAgent {
                agent_id: self.agent_id.clone(),
                name: self.workspace.clone(),
                capabilities: vec!["base".to_string(), "chat".to_string()],
                metadata: Some(AgentMetadata {
                    working_directory: self.working_dir.clone(),
                    git: None,
                    hostname: hostname::get()?.to_string_lossy().to_string(),
                    os: std::env::consts::OS.to_string(),
                    workspaces: vec![],
                    backend: self.backend.clone(),
                }),
                protocol_features: vec!["pack_tools".to_string()],
            })),
        };
        tx.send(register).await?;

        // Start bidirectional stream
        let response = self.client.agent_stream(outbound).await?;
        let mut inbound = response.into_inner();

        // Spawn task to forward responses from handlers to the gRPC stream
        let tx_clone = tx.clone();
        let response_forwarder = tokio::spawn(async move {
            while let Some(response) = resp_rx.recv().await {
                let msg = AgentMessage {
                    payload: Some(coven::agent_message::Payload::Response(response)),
                };
                if tx_clone.send(msg).await.is_err() {
                    tracing::warn!("Failed to send response - channel closed");
                    break;
                }
            }
        });

        // Wrap on_welcome in Option so we can take() it (FnOnce can only be called once)
        let mut on_welcome = Some(on_welcome);

        while let Some(msg) = inbound.message().await? {
            match msg.payload {
                Some(coven::server_message::Payload::Welcome(welcome)) => {
                    tracing::info!(
                        agent_id = %welcome.agent_id,
                        instance_id = %welcome.instance_id,
                        mcp_endpoint = %welcome.mcp_endpoint,
                        tool_count = welcome.available_tools.len(),
                        "Registered with coven-gateway"
                    );

                    // Extract welcome info and call setup callback (only once)
                    if let Some(callback) = on_welcome.take() {
                        let welcome_info = WelcomeInfo {
                            agent_id: welcome.agent_id,
                            instance_id: welcome.instance_id,
                            mcp_endpoint: welcome.mcp_endpoint,
                            mcp_token: welcome.mcp_token,
                            available_tools: welcome.available_tools,
                        };

                        callback(welcome_info, tx.clone());
                    } else {
                        tracing::warn!("Received duplicate Welcome message - ignoring");
                    }
                }
                Some(coven::server_message::Payload::SendMessage(send_msg)) => {
                    // Check if Welcome has been processed (on_welcome taken = None)
                    if on_welcome.is_some() {
                        tracing::warn!("Received message before Welcome - ignoring");
                        continue;
                    }

                    let request_id = send_msg.request_id.clone();
                    tracing::info!(request_id = %request_id, "Received message");

                    // Pass the response sender to the handler for real-time streaming
                    if let Err(e) = message_handler(send_msg, resp_tx.clone()).await {
                        tracing::error!(error = %e, "Handler error");
                        // Send error response
                        let error_response = coven::MessageResponse {
                            request_id,
                            event: Some(coven::message_response::Event::Error(e.to_string())),
                        };
                        let _ = resp_tx.send(error_response).await;
                    }
                }
                Some(coven::server_message::Payload::Shutdown(shutdown)) => {
                    tracing::info!(reason = %shutdown.reason, "Received shutdown");
                    break;
                }
                Some(coven::server_message::Payload::InjectContext(_)) => {
                    // TODO: Handle context injection
                }
                Some(coven::server_message::Payload::CancelRequest(_)) => {
                    // TODO: Handle cancellation
                }
                Some(coven::server_message::Payload::ToolApproval(_)) => {
                    // TODO: Handle tool approval
                }
                Some(coven::server_message::Payload::RegistrationError(err)) => {
                    tracing::error!(error = %err.reason, "Registration failed");
                    break;
                }
                Some(coven::server_message::Payload::PackToolResult(result)) => {
                    // Route pack tool result to waiting caller
                    if let Some(ref pending) = pending_pack_tools {
                        let status = match &result.result {
                            Some(coven_proto::pack_tool_result::Result::OutputJson(_)) => {
                                "✓ success"
                            }
                            Some(coven_proto::pack_tool_result::Result::Error(_)) => "✗ error",
                            None => "? empty",
                        };
                        tracing::debug!(
                            request_id = %result.request_id,
                            status = %status,
                            "← Pack tool result"
                        );

                        if !handle_pack_tool_result(pending, result).await {
                            tracing::warn!("Pack tool result for unknown request");
                        }
                    } else {
                        tracing::debug!("Pack tool result received but no handler registered");
                    }
                }
                None => {}
            }
        }

        // Clean up
        drop(resp_tx);
        response_forwarder.abort();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_id_formatting() {
        let id = format_agent_id("home", "research");
        assert_eq!(id, "home_research");
    }
}
