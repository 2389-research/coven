// ABOUTME: gRPC client wrapper for communicating with coven-gateway.
// ABOUTME: Handles authentication, message sending, and event streaming.

use crate::error::{BridgeError, Result};
use coven_proto::client::ClientServiceClient;
use coven_proto::{
    AgentInfo, ApproveToolRequest, ClientSendMessageRequest, ClientSendMessageResponse,
    ClientStreamEvent, ListAgentsRequest, StreamEventsRequest,
};
use tonic::transport::Channel;
use tonic::{Request, Status};
use tracing::{debug, error, info};

/// gRPC client for communicating with coven-gateway's ClientService.
pub struct GatewayClient {
    client: ClientServiceClient<
        tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>,
    >,
}

/// Interceptor that adds Bearer token authentication to outgoing requests.
#[derive(Clone)]
struct AuthInterceptor {
    token: Option<String>,
}

impl tonic::service::Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: Request<()>) -> std::result::Result<Request<()>, Status> {
        if let Some(ref token) = self.token {
            let auth_value = format!("Bearer {}", token)
                .parse()
                .map_err(|_| Status::internal("invalid token format"))?;
            req.metadata_mut().insert("authorization", auth_value);
        }
        Ok(req)
    }
}

impl GatewayClient {
    /// Connect to the gateway at the given URL with optional authentication token.
    pub async fn connect(url: &str, token: Option<String>) -> Result<Self> {
        info!(url = %url, "Connecting to gateway");

        let channel = Channel::from_shared(url.to_string())
            .map_err(|e| BridgeError::Config(format!("invalid gateway URL: {}", e)))?
            .connect()
            .await?;

        let interceptor = AuthInterceptor { token };
        let client = ClientServiceClient::with_interceptor(channel, interceptor);

        Ok(Self { client })
    }

    /// List all available agents from the gateway.
    pub async fn list_agents(&mut self) -> Result<Vec<AgentInfo>> {
        debug!("Listing agents");
        let response = self
            .client
            .list_agents(ListAgentsRequest { workspace: None })
            .await?;
        Ok(response.into_inner().agents)
    }

    /// Send a message to the gateway for a given conversation.
    pub async fn send_message(
        &mut self,
        conversation_key: String,
        content: String,
        idempotency_key: String,
    ) -> Result<ClientSendMessageResponse> {
        info!(
            conversation_key = %conversation_key,
            content_len = content.len(),
            idempotency_key = %idempotency_key,
            "Sending message to gateway"
        );

        let request = ClientSendMessageRequest {
            conversation_key: conversation_key.clone(),
            content,
            attachments: vec![],
            idempotency_key,
        };

        let response = self.client.send_message(request).await;
        match &response {
            Ok(r) => {
                let inner = r.get_ref();
                info!(
                    status = %inner.status,
                    message_id = %inner.message_id,
                    "Gateway accepted message"
                );
            }
            Err(e) => {
                error!(
                    error = %e,
                    conversation_key = %conversation_key,
                    "Gateway rejected message"
                );
            }
        }
        Ok(response?.into_inner())
    }

    /// Stream events from the gateway for a given conversation.
    pub async fn stream_events(
        &mut self,
        conversation_key: String,
    ) -> Result<impl futures::Stream<Item = std::result::Result<ClientStreamEvent, Status>>> {
        debug!(conversation_key = %conversation_key, "Starting event stream");

        let request = StreamEventsRequest {
            conversation_key,
            since_event_id: None,
        };

        let response = self.client.stream_events(request).await?;
        Ok(response.into_inner())
    }

    /// Respond to a tool approval request.
    pub async fn approve_tool(
        &mut self,
        agent_id: String,
        tool_id: String,
        approved: bool,
        approve_all: bool,
    ) -> Result<()> {
        debug!(agent_id = %agent_id, tool_id = %tool_id, approved = %approved, "Responding to tool approval");

        let request = ApproveToolRequest {
            agent_id,
            tool_id,
            approved,
            approve_all,
        };

        let response = self.client.approve_tool(request).await?;
        let inner = response.into_inner();
        if !inner.success {
            let err_msg = inner.error.unwrap_or_else(|| "unknown error".to_string());
            return Err(BridgeError::Config(format!("tool approval failed: {}", err_msg)).into());
        }
        Ok(())
    }
}
