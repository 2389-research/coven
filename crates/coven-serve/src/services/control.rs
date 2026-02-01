// ABOUTME: CovenControl gRPC service implementation for agent connections
// ABOUTME: Handles agent registration, heartbeats, and message routing

use crate::store::{Agent, Store};
use chrono::Utc;
use coven_proto::server::CovenControl;
use coven_proto::{
    AgentMessage, MessageResponse, SendMessage, ServerMessage, ToolApprovalResponse, Welcome,
};
use futures::StreamExt;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Message to send to an agent
#[derive(Debug, Clone)]
pub struct OutboundMessage {
    pub agent_id: String,
    pub request_id: String,
    pub thread_id: String,
    pub sender: String,
    pub content: String,
}

/// Response from an agent
#[derive(Debug, Clone)]
pub struct AgentResponse {
    pub agent_id: String,
    pub request_id: String,
    pub response: MessageResponse,
}

/// Connected agent handle
struct ConnectedAgent {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    name: String,
    tx: mpsc::Sender<ServerMessage>,
}

/// Shared state for the control service
pub struct ControlState {
    pub store: Store,
    /// Connected agents by ID
    agents: RwLock<HashMap<String, ConnectedAgent>>,
    /// Channel for sending messages to agents (reserved for future broadcast use)
    #[allow(dead_code)]
    outbound_tx: broadcast::Sender<OutboundMessage>,
    /// Channel for receiving responses from agents
    response_tx: broadcast::Sender<AgentResponse>,
}

impl ControlState {
    pub fn new(store: Store) -> Arc<Self> {
        let (outbound_tx, _) = broadcast::channel(256);
        let (response_tx, _) = broadcast::channel(256);

        Arc::new(Self {
            store,
            agents: RwLock::new(HashMap::new()),
            outbound_tx,
            response_tx,
        })
    }

    /// Send a message to an agent
    pub async fn send_to_agent(&self, msg: OutboundMessage) -> Result<(), Status> {
        let agents = self.agents.read().await;
        if let Some(agent) = agents.get(&msg.agent_id) {
            let server_msg = ServerMessage {
                payload: Some(coven_proto::server_message::Payload::SendMessage(
                    SendMessage {
                        request_id: msg.request_id,
                        thread_id: msg.thread_id,
                        sender: msg.sender,
                        content: msg.content,
                        attachments: vec![],
                    },
                )),
            };
            agent
                .tx
                .send(server_msg)
                .await
                .map_err(|_| Status::internal("failed to send to agent"))?;
            Ok(())
        } else {
            Err(Status::not_found(format!(
                "agent not connected: {}",
                msg.agent_id
            )))
        }
    }

    /// Subscribe to responses from agents
    pub fn subscribe_responses(&self) -> broadcast::Receiver<AgentResponse> {
        self.response_tx.subscribe()
    }

    /// List connected agent IDs
    pub async fn list_connected(&self) -> Vec<String> {
        self.agents.read().await.keys().cloned().collect()
    }

    /// Check if agent is connected
    pub async fn is_connected(&self, agent_id: &str) -> bool {
        self.agents.read().await.contains_key(agent_id)
    }

    /// Forward tool approval to an agent
    pub async fn approve_tool(
        &self,
        agent_id: &str,
        tool_id: &str,
        approved: bool,
        approve_all: bool,
    ) -> Result<(), Status> {
        let agents = self.agents.read().await;
        if let Some(agent) = agents.get(agent_id) {
            let server_msg = ServerMessage {
                payload: Some(coven_proto::server_message::Payload::ToolApproval(
                    ToolApprovalResponse {
                        id: tool_id.to_string(),
                        approved,
                        approve_all,
                    },
                )),
            };
            agent
                .tx
                .send(server_msg)
                .await
                .map_err(|_| Status::internal("failed to send approval to agent"))?;
            debug!(agent_id = %agent_id, tool_id = %tool_id, approved = approved, "Tool approval forwarded");
            Ok(())
        } else {
            Err(Status::not_found(format!(
                "agent not connected: {}",
                agent_id
            )))
        }
    }
}

/// CovenControl service implementation
pub struct CovenControlService {
    state: Arc<ControlState>,
}

impl CovenControlService {
    pub fn new(state: Arc<ControlState>) -> Self {
        Self { state }
    }

    pub fn state(&self) -> Arc<ControlState> {
        self.state.clone()
    }
}

#[tonic::async_trait]
impl CovenControl for CovenControlService {
    type AgentStreamStream =
        Pin<Box<dyn futures::Stream<Item = Result<ServerMessage, Status>> + Send>>;

    async fn agent_stream(
        &self,
        request: Request<Streaming<AgentMessage>>,
    ) -> Result<Response<Self::AgentStreamStream>, Status> {
        let mut inbound = request.into_inner();

        // Wait for registration message
        let first_msg = inbound
            .next()
            .await
            .ok_or_else(|| Status::invalid_argument("no registration message"))?
            .map_err(|e| Status::internal(format!("stream error: {}", e)))?;

        let register = match first_msg.payload {
            Some(coven_proto::agent_message::Payload::Register(r)) => r,
            _ => {
                return Err(Status::invalid_argument(
                    "first message must be registration",
                ))
            }
        };

        let agent_id = if register.agent_id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            register.agent_id.clone()
        };
        let agent_name = if register.name.is_empty() {
            agent_id.clone()
        } else {
            register.name.clone()
        };

        info!(agent_id = %agent_id, name = %agent_name, "Agent connecting");

        // Create channel for outbound messages to this agent
        let (tx, rx) = mpsc::channel::<ServerMessage>(32);

        // Register agent in store
        let metadata = register.metadata.as_ref();
        let agent = Agent {
            id: agent_id.clone(),
            name: agent_name.clone(),
            backend: metadata.map(|m| m.backend.clone()).unwrap_or_default(),
            working_dir: metadata
                .map(|m| m.working_directory.clone())
                .unwrap_or_default(),
            connected: true,
            connected_at: Some(Utc::now()),
            last_seen: Some(Utc::now()),
        };

        if let Err(e) = self.state.store.upsert_agent(&agent).await {
            error!(agent_id = %agent_id, error = %e, "Failed to save agent");
        }

        // Add to connected agents
        {
            let mut agents = self.state.agents.write().await;
            agents.insert(
                agent_id.clone(),
                ConnectedAgent {
                    id: agent_id.clone(),
                    name: agent_name.clone(),
                    tx: tx.clone(),
                },
            );
        }

        // Send welcome message
        let welcome = ServerMessage {
            payload: Some(coven_proto::server_message::Payload::Welcome(Welcome {
                server_id: "local-gateway".to_string(),
                agent_id: agent_id.clone(),
                instance_id: agent_id[..8.min(agent_id.len())].to_string(),
                principal_id: agent_id.clone(), // No principals in local mode
                available_tools: vec![],        // TODO: populate from packs
                mcp_token: String::new(),
                mcp_endpoint: String::new(),
                secrets: HashMap::new(),
            })),
        };
        tx.send(welcome)
            .await
            .map_err(|_| Status::internal("failed to send welcome"))?;

        info!(agent_id = %agent_id, "Agent registered");

        // Clone state for the inbound handler
        let state = self.state.clone();
        let agent_id_clone = agent_id.clone();

        // Spawn task to handle inbound messages from agent
        tokio::spawn(async move {
            while let Some(result) = inbound.next().await {
                match result {
                    Ok(msg) => {
                        if let Some(payload) = msg.payload {
                            match payload {
                                coven_proto::agent_message::Payload::Heartbeat(_) => {
                                    debug!(agent_id = %agent_id_clone, "Heartbeat received");
                                    // Update last_seen
                                    let _ = state
                                        .store
                                        .set_agent_connected(&agent_id_clone, true)
                                        .await;
                                }
                                coven_proto::agent_message::Payload::Response(resp) => {
                                    debug!(agent_id = %agent_id_clone, request_id = %resp.request_id, "Response received");
                                    let _ = state.response_tx.send(AgentResponse {
                                        agent_id: agent_id_clone.clone(),
                                        request_id: resp.request_id.clone(),
                                        response: resp,
                                    });
                                }
                                _ => {
                                    debug!(agent_id = %agent_id_clone, "Other message received");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!(agent_id = %agent_id_clone, error = %e, "Stream error");
                        break;
                    }
                }
            }

            // Agent disconnected
            info!(agent_id = %agent_id_clone, "Agent disconnected");
            {
                let mut agents = state.agents.write().await;
                agents.remove(&agent_id_clone);
            }
            let _ = state
                .store
                .set_agent_connected(&agent_id_clone, false)
                .await;
        });

        // Return stream of outbound messages
        let stream = ReceiverStream::new(rx).map(Ok);
        Ok(Response::new(Box::pin(stream)))
    }
}
