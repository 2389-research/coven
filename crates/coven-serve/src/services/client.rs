// ABOUTME: ClientService gRPC implementation for TUI/client connections
// ABOUTME: Handles listing agents, sending messages, and streaming responses

use super::control::{ControlState, OutboundMessage};
use crate::store::{Message, Store};
use chrono::Utc;
use coven_proto::server::ClientService;
use coven_proto::{
    client_stream_event, AgentInfo, ApproveToolRequest, ApproveToolResponse,
    ClientSendMessageRequest, ClientSendMessageResponse, ClientStreamEvent, GetEventsRequest,
    GetEventsResponse, ListAgentsRequest, ListAgentsResponse, MeResponse, RegisterAgentRequest,
    RegisterAgentResponse, RegisterClientRequest, RegisterClientResponse, StreamDone, StreamError,
    StreamEventsRequest, TextChunk, ThinkingChunk,
};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// ClientService implementation
pub struct ClientServiceImpl {
    store: Store,
    control: Arc<ControlState>,
}

impl ClientServiceImpl {
    pub fn new(store: Store, control: Arc<ControlState>) -> Self {
        Self { store, control }
    }
}

#[tonic::async_trait]
impl ClientService for ClientServiceImpl {
    async fn get_me(&self, _request: Request<()>) -> Result<Response<MeResponse>, Status> {
        // In local mode, everyone is admin
        Ok(Response::new(MeResponse {
            principal_id: "local-user".to_string(),
            principal_type: "client".to_string(),
            display_name: "Local User".to_string(),
            status: "approved".to_string(),
            roles: vec!["owner".to_string()],
            member_id: None,
            member_display_name: None,
        }))
    }

    async fn get_events(
        &self,
        request: Request<GetEventsRequest>,
    ) -> Result<Response<GetEventsResponse>, Status> {
        let req = request.into_inner();
        let conversation_id = &req.conversation_key;

        let limit = req.limit.unwrap_or(100).min(500) as i64;
        let messages = self
            .store
            .get_messages(conversation_id, limit)
            .await
            .map_err(|e| Status::internal(format!("database error: {}", e)))?;

        let events = messages
            .into_iter()
            .map(|m| coven_proto::Event {
                id: m.id,
                conversation_key: m.conversation_id,
                direction: m.direction,
                author: m.author,
                timestamp: m.created_at.to_rfc3339(),
                r#type: m.message_type,
                text: Some(m.content),
                raw_transport: None,
                raw_payload_ref: None,
                actor_principal_id: None,
                actor_member_id: None,
            })
            .collect();

        Ok(Response::new(GetEventsResponse {
            events,
            next_cursor: None,
            has_more: false,
        }))
    }

    async fn send_message(
        &self,
        request: Request<ClientSendMessageRequest>,
    ) -> Result<Response<ClientSendMessageResponse>, Status> {
        let req = request.into_inner();
        let agent_id = &req.conversation_key;

        // Check if agent is connected
        if !self.control.is_connected(agent_id).await {
            return Err(Status::not_found(format!(
                "agent not connected: {}",
                agent_id
            )));
        }

        // Get or create conversation
        let conversation = self
            .store
            .get_or_create_conversation(agent_id)
            .await
            .map_err(|e| Status::internal(format!("database error: {}", e)))?;

        // Generate request ID
        let request_id = if req.idempotency_key.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            req.idempotency_key.clone()
        };

        // Save inbound message
        let msg = Message {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation.id.clone(),
            direction: "inbound".to_string(),
            author: "user".to_string(),
            content: req.content.clone(),
            message_type: "message".to_string(),
            created_at: Utc::now(),
        };
        self.store
            .save_message(&msg)
            .await
            .map_err(|e| Status::internal(format!("database error: {}", e)))?;

        // Send to agent
        self.control
            .send_to_agent(OutboundMessage {
                agent_id: agent_id.clone(),
                request_id: request_id.clone(),
                thread_id: conversation.id.clone(),
                sender: "user".to_string(),
                content: req.content,
            })
            .await?;

        info!(agent_id = %agent_id, request_id = %request_id, "Message sent to agent");

        Ok(Response::new(ClientSendMessageResponse {
            status: "accepted".to_string(),
            message_id: request_id,
        }))
    }

    type StreamEventsStream =
        Pin<Box<dyn futures::Stream<Item = Result<ClientStreamEvent, Status>> + Send>>;

    async fn stream_events(
        &self,
        request: Request<StreamEventsRequest>,
    ) -> Result<Response<Self::StreamEventsStream>, Status> {
        let req = request.into_inner();
        let agent_id = req.conversation_key.clone();

        // Subscribe to agent responses
        let mut response_rx = self.control.subscribe_responses();
        let (tx, rx) = mpsc::channel(32);

        let store = self.store.clone();

        // Spawn task to filter and forward responses
        tokio::spawn(async move {
            loop {
                match response_rx.recv().await {
                    Ok(resp) => {
                        // Only forward responses for this agent
                        if resp.agent_id != agent_id {
                            continue;
                        }

                        let event = match &resp.response.event {
                            Some(coven_proto::message_response::Event::Text(text)) => {
                                ClientStreamEvent {
                                    conversation_key: agent_id.clone(),
                                    timestamp: Utc::now().to_rfc3339(),
                                    payload: Some(client_stream_event::Payload::Text(TextChunk {
                                        content: text.clone(),
                                    })),
                                }
                            }
                            Some(coven_proto::message_response::Event::Thinking(text)) => {
                                ClientStreamEvent {
                                    conversation_key: agent_id.clone(),
                                    timestamp: Utc::now().to_rfc3339(),
                                    payload: Some(client_stream_event::Payload::Thinking(
                                        ThinkingChunk {
                                            content: text.clone(),
                                        },
                                    )),
                                }
                            }
                            Some(coven_proto::message_response::Event::ToolUse(tool)) => {
                                ClientStreamEvent {
                                    conversation_key: agent_id.clone(),
                                    timestamp: Utc::now().to_rfc3339(),
                                    payload: Some(client_stream_event::Payload::ToolUse(
                                        tool.clone(),
                                    )),
                                }
                            }
                            Some(coven_proto::message_response::Event::ToolResult(result)) => {
                                ClientStreamEvent {
                                    conversation_key: agent_id.clone(),
                                    timestamp: Utc::now().to_rfc3339(),
                                    payload: Some(client_stream_event::Payload::ToolResult(
                                        result.clone(),
                                    )),
                                }
                            }
                            Some(coven_proto::message_response::Event::ToolApprovalRequest(
                                approval,
                            )) => ClientStreamEvent {
                                conversation_key: agent_id.clone(),
                                timestamp: Utc::now().to_rfc3339(),
                                payload: Some(client_stream_event::Payload::ToolApproval(
                                    coven_proto::ClientToolApprovalRequest {
                                        agent_id: agent_id.clone(),
                                        request_id: resp.request_id.clone(),
                                        tool_id: approval.id.clone(),
                                        tool_name: approval.name.clone(),
                                        input_json: approval.input_json.clone(),
                                    },
                                )),
                            },
                            Some(coven_proto::message_response::Event::Done(done)) => {
                                // Save the complete response to store
                                if !done.full_response.is_empty() {
                                    let msg = Message {
                                        id: Uuid::new_v4().to_string(),
                                        conversation_id: agent_id.clone(),
                                        direction: "outbound".to_string(),
                                        author: "agent".to_string(),
                                        content: done.full_response.clone(),
                                        message_type: "message".to_string(),
                                        created_at: Utc::now(),
                                    };
                                    let _ = store.save_message(&msg).await;
                                }

                                ClientStreamEvent {
                                    conversation_key: agent_id.clone(),
                                    timestamp: Utc::now().to_rfc3339(),
                                    payload: Some(client_stream_event::Payload::Done(StreamDone {
                                        full_response: Some(done.full_response.clone()),
                                    })),
                                }
                            }
                            Some(coven_proto::message_response::Event::Error(err)) => {
                                ClientStreamEvent {
                                    conversation_key: agent_id.clone(),
                                    timestamp: Utc::now().to_rfc3339(),
                                    payload: Some(client_stream_event::Payload::Error(
                                        StreamError {
                                            message: err.clone(),
                                            recoverable: false,
                                        },
                                    )),
                                }
                            }
                            Some(coven_proto::message_response::Event::Usage(usage)) => {
                                ClientStreamEvent {
                                    conversation_key: agent_id.clone(),
                                    timestamp: Utc::now().to_rfc3339(),
                                    payload: Some(client_stream_event::Payload::Usage(*usage)),
                                }
                            }
                            Some(coven_proto::message_response::Event::ToolState(state)) => {
                                ClientStreamEvent {
                                    conversation_key: agent_id.clone(),
                                    timestamp: Utc::now().to_rfc3339(),
                                    payload: Some(client_stream_event::Payload::ToolState(
                                        state.clone(),
                                    )),
                                }
                            }
                            _ => continue,
                        };

                        if tx.send(Ok(event)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(agent_id = %agent_id, missed = n, "Client stream lagged, missed messages");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        });

        let stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream)))
    }

    async fn list_agents(
        &self,
        _request: Request<ListAgentsRequest>,
    ) -> Result<Response<ListAgentsResponse>, Status> {
        let agents = self
            .store
            .list_agents()
            .await
            .map_err(|e| Status::internal(format!("database error: {}", e)))?;

        let agent_infos: Vec<AgentInfo> = agents
            .into_iter()
            .map(|a| AgentInfo {
                id: a.id.clone(),
                name: a.name,
                backend: a.backend,
                working_dir: a.working_dir,
                connected: a.connected,
                metadata: None,
            })
            .collect();

        Ok(Response::new(ListAgentsResponse {
            agents: agent_infos,
        }))
    }

    async fn register_agent(
        &self,
        request: Request<RegisterAgentRequest>,
    ) -> Result<Response<RegisterAgentResponse>, Status> {
        let req = request.into_inner();
        // In local mode, auto-approve everything
        Ok(Response::new(RegisterAgentResponse {
            principal_id: format!("local-{}", req.fingerprint),
            status: "approved".to_string(),
        }))
    }

    async fn register_client(
        &self,
        request: Request<RegisterClientRequest>,
    ) -> Result<Response<RegisterClientResponse>, Status> {
        let req = request.into_inner();
        // In local mode, auto-approve everything
        Ok(Response::new(RegisterClientResponse {
            principal_id: format!("local-{}", req.fingerprint),
            status: "approved".to_string(),
        }))
    }

    async fn approve_tool(
        &self,
        request: Request<ApproveToolRequest>,
    ) -> Result<Response<ApproveToolResponse>, Status> {
        let req = request.into_inner();
        debug!(
            agent_id = %req.agent_id,
            tool_id = %req.tool_id,
            approved = req.approved,
            "Tool approval request"
        );

        // Forward approval to agent
        match self
            .control
            .approve_tool(&req.agent_id, &req.tool_id, req.approved, req.approve_all)
            .await
        {
            Ok(()) => Ok(Response::new(ApproveToolResponse {
                success: true,
                error: None,
            })),
            Err(e) => Ok(Response::new(ApproveToolResponse {
                success: false,
                error: Some(e.message().to_string()),
            })),
        }
    }
}
