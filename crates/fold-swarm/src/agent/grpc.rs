// ABOUTME: gRPC client for connecting to fold-gateway.
// ABOUTME: Handles registration, message receiving, and real-time response streaming.

use anyhow::{Context, Result};
use std::time::Duration;
use tokio::sync::mpsc;
use tonic::transport::Channel;

// Use shared proto types from fold-proto
pub use fold_proto::fold;

use fold_proto::client::FoldControlClient;
use fold_proto::{AgentMessage, AgentMetadata, RegisterAgent};

/// Sender for streaming responses back to the gateway in real-time.
/// Clone this and use it to send responses as they arrive from the backend.
pub type ResponseSender = mpsc::Sender<fold::MessageResponse>;

pub fn format_agent_id(prefix: &str, workspace: &str) -> String {
    format!("{}_{}", prefix, workspace)
}

pub struct GatewayClient {
    client: FoldControlClient<Channel>,
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
        let channel = Channel::from_shared(gateway_url.to_string())?
            .http2_keep_alive_interval(Duration::from_secs(10))
            .keep_alive_timeout(Duration::from_secs(20))
            .keep_alive_while_idle(true)
            .connect()
            .await
            .context("Failed to connect to fold-gateway")?;

        let client = FoldControlClient::new(channel);
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
    pub async fn run<F, Fut>(mut self, mut message_handler: F) -> Result<()>
    where
        F: FnMut(fold::SendMessage, ResponseSender) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let (tx, rx) = mpsc::channel::<AgentMessage>(32);
        let outbound = tokio_stream::wrappers::ReceiverStream::new(rx);

        // Channel for responses from handlers - these get forwarded to the gRPC stream
        let (resp_tx, mut resp_rx) = mpsc::channel::<fold::MessageResponse>(64);

        // Send registration
        let register = AgentMessage {
            payload: Some(fold::agent_message::Payload::Register(RegisterAgent {
                agent_id: self.agent_id.clone(),
                name: self.workspace.clone(),
                capabilities: vec!["prompt".to_string()],
                metadata: Some(AgentMetadata {
                    working_directory: self.working_dir.clone(),
                    git: None,
                    hostname: hostname::get()?.to_string_lossy().to_string(),
                    os: std::env::consts::OS.to_string(),
                    workspaces: vec![],
                    backend: self.backend.clone(),
                }),
                protocol_features: vec![],
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
                    payload: Some(fold::agent_message::Payload::Response(response)),
                };
                if tx_clone.send(msg).await.is_err() {
                    tracing::warn!("Failed to send response - channel closed");
                    break;
                }
            }
        });

        while let Some(msg) = inbound.message().await? {
            match msg.payload {
                Some(fold::server_message::Payload::Welcome(welcome)) => {
                    tracing::info!(
                        agent_id = %welcome.agent_id,
                        instance_id = %welcome.instance_id,
                        "Registered with fold-gateway"
                    );
                }
                Some(fold::server_message::Payload::SendMessage(send_msg)) => {
                    let request_id = send_msg.request_id.clone();
                    tracing::info!(request_id = %request_id, "Received message");

                    // Pass the response sender to the handler for real-time streaming
                    if let Err(e) = message_handler(send_msg, resp_tx.clone()).await {
                        tracing::error!(error = %e, "Handler error");
                        // Send error response
                        let error_response = fold::MessageResponse {
                            request_id,
                            event: Some(fold::message_response::Event::Error(e.to_string())),
                        };
                        let _ = resp_tx.send(error_response).await;
                    }
                }
                Some(fold::server_message::Payload::Shutdown(shutdown)) => {
                    tracing::info!(reason = %shutdown.reason, "Received shutdown");
                    break;
                }
                Some(fold::server_message::Payload::InjectContext(_)) => {
                    // TODO: Handle context injection
                }
                Some(fold::server_message::Payload::CancelRequest(_)) => {
                    // TODO: Handle cancellation
                }
                Some(fold::server_message::Payload::ToolApproval(_)) => {
                    // TODO: Handle tool approval
                }
                Some(fold::server_message::Payload::RegistrationError(err)) => {
                    tracing::error!(error = %err.reason, "Registration failed");
                    break;
                }
                Some(fold::server_message::Payload::PackToolResult(_)) => {
                    // Pack tool results are not handled by swarm agents
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
