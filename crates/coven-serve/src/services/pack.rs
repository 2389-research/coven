// ABOUTME: PackService gRPC implementation for tool pack connections
// ABOUTME: Handles pack registration and tool execution routing

use crate::store::{Pack, Store};
use chrono::Utc;
use coven_proto::server::PackService;
use coven_proto::{ExecuteToolRequest, ExecuteToolResponse, PackManifest, ToolDefinition};
use futures::Stream;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Connected pack handle
struct ConnectedPack {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    version: String,
    tools: Vec<ToolDefinition>,
    tx: mpsc::Sender<ExecuteToolRequest>,
}

/// Pending tool execution
struct PendingExecution {
    response_tx: oneshot::Sender<ExecuteToolResponse>,
}

/// Stream wrapper that triggers cleanup when the pack disconnects
struct PackStream {
    inner: ReceiverStream<ExecuteToolRequest>,
    pack_id: String,
    state: Arc<PackState>,
}

impl Stream for PackStream {
    type Item = Result<ExecuteToolRequest, Status>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let inner = Pin::new(&mut self.inner);
        match inner.poll_next(cx) {
            Poll::Ready(Some(item)) => Poll::Ready(Some(Ok(item))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for PackStream {
    fn drop(&mut self) {
        let pack_id = self.pack_id.clone();
        let state = self.state.clone();

        // Spawn cleanup task - can't do async in drop directly
        tokio::spawn(async move {
            state.disconnect_pack(&pack_id).await;
        });
    }
}

/// Shared state for pack service
pub struct PackState {
    pub store: Store,
    /// Connected packs by ID
    packs: RwLock<HashMap<String, ConnectedPack>>,
    /// Pending tool executions: request_id -> sender
    pending: RwLock<HashMap<String, PendingExecution>>,
}

impl PackState {
    pub fn new(store: Store) -> Arc<Self> {
        Arc::new(Self {
            store,
            packs: RwLock::new(HashMap::new()),
            pending: RwLock::new(HashMap::new()),
        })
    }

    /// Get all available tools from all connected packs
    pub async fn list_tools(&self) -> Vec<(String, ToolDefinition)> {
        let packs = self.packs.read().await;
        let mut tools = Vec::new();
        for (pack_id, pack) in packs.iter() {
            for tool in &pack.tools {
                tools.push((pack_id.clone(), tool.clone()));
            }
        }
        tools
    }

    /// Execute a tool on a pack
    pub async fn execute_tool(
        &self,
        tool_name: &str,
        input_json: &str,
    ) -> Result<ExecuteToolResponse, Status> {
        // Find which pack has this tool and extract the sender
        // Release the lock before any async operations to avoid race conditions
        let tx = {
            let packs = self.packs.read().await;
            let pack = packs
                .values()
                .find(|p| p.tools.iter().any(|t| t.name == tool_name));

            match pack {
                Some(p) => p.tx.clone(),
                None => return Err(Status::not_found(format!("tool not found: {}", tool_name))),
            }
        };

        let request_id = Uuid::new_v4().to_string();

        // Create oneshot channel for response
        let (response_tx, response_rx) = oneshot::channel();

        // Store pending execution
        {
            let mut pending = self.pending.write().await;
            pending.insert(request_id.clone(), PendingExecution { response_tx });
        }

        // Send request to pack (lock is already released)
        let request = ExecuteToolRequest {
            tool_name: tool_name.to_string(),
            input_json: input_json.to_string(),
            request_id: request_id.clone(),
        };

        if tx.send(request).await.is_err() {
            // Cleanup pending on send failure
            let mut pending = self.pending.write().await;
            pending.remove(&request_id);
            return Err(Status::internal("pack disconnected"));
        }

        // Wait for response with timeout
        match tokio::time::timeout(std::time::Duration::from_secs(60), response_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(Status::internal("pack handler dropped")),
            Err(_) => {
                // Cleanup pending
                let mut pending = self.pending.write().await;
                pending.remove(&request_id);
                Err(Status::deadline_exceeded("tool execution timed out"))
            }
        }
    }

    /// Handle tool result from pack
    pub async fn handle_tool_result(&self, response: ExecuteToolResponse) {
        let mut pending = self.pending.write().await;
        if let Some(execution) = pending.remove(&response.request_id) {
            let _ = execution.response_tx.send(response);
        }
    }

    /// Cleanup when pack disconnects
    pub async fn disconnect_pack(&self, pack_id: &str) {
        info!(pack_id = %pack_id, "Pack disconnecting");

        // Get the tools this pack was providing to identify orphaned pending requests
        let tool_names: Vec<String> = {
            let packs = self.packs.read().await;
            if let Some(pack) = packs.get(pack_id) {
                pack.tools.iter().map(|t| t.name.clone()).collect()
            } else {
                vec![]
            }
        };

        // Remove the pack
        {
            let mut packs = self.packs.write().await;
            packs.remove(pack_id);
        }

        // Note: We can't easily clean up pending executions tied to this pack
        // because we don't track which pack a pending request went to.
        // The pending requests will timeout naturally (60s) if the pack disconnected.
        // This is acceptable for a local gateway - pending requests fail gracefully.
        if !tool_names.is_empty() {
            warn!(
                pack_id = %pack_id,
                tools = ?tool_names,
                "Pack disconnected; any pending tool executions will timeout"
            );
        }

        let _ = self.store.set_pack_connected(pack_id, false).await;
    }
}

/// PackService implementation
pub struct PackServiceImpl {
    state: Arc<PackState>,
}

impl PackServiceImpl {
    pub fn new(state: Arc<PackState>) -> Self {
        Self { state }
    }

    pub fn state(&self) -> Arc<PackState> {
        self.state.clone()
    }
}

#[tonic::async_trait]
impl PackService for PackServiceImpl {
    type RegisterStream =
        Pin<Box<dyn futures::Stream<Item = Result<ExecuteToolRequest, Status>> + Send>>;

    async fn register(
        &self,
        request: Request<PackManifest>,
    ) -> Result<Response<Self::RegisterStream>, Status> {
        let manifest = request.into_inner();
        let pack_id = manifest.pack_id.clone();
        let version = manifest.version.clone();

        info!(pack_id = %pack_id, version = %version, tools = manifest.tools.len(), "Pack registering");

        // Create channel for tool execution requests
        let (tx, rx) = mpsc::channel::<ExecuteToolRequest>(32);

        // Save pack to store
        let pack = Pack {
            id: pack_id.clone(),
            version: version.clone(),
            connected: true,
            connected_at: Some(Utc::now()),
        };
        if let Err(e) = self.state.store.upsert_pack(&pack).await {
            error!(pack_id = %pack_id, error = %e, "Failed to save pack");
        }

        // Add to connected packs
        {
            let mut packs = self.state.packs.write().await;
            packs.insert(
                pack_id.clone(),
                ConnectedPack {
                    id: pack_id.clone(),
                    version,
                    tools: manifest.tools,
                    tx,
                },
            );
        }

        info!(pack_id = %pack_id, "Pack registered");

        // Return stream with cleanup on disconnect
        let stream = PackStream {
            inner: ReceiverStream::new(rx),
            pack_id,
            state: self.state.clone(),
        };
        Ok(Response::new(Box::pin(stream)))
    }

    async fn tool_result(
        &self,
        request: Request<ExecuteToolResponse>,
    ) -> Result<Response<()>, Status> {
        let response = request.into_inner();
        debug!(request_id = %response.request_id, "Tool result received");
        self.state.handle_tool_result(response).await;
        Ok(Response::new(()))
    }
}
