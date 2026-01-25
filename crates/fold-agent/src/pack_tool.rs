// ABOUTME: PackTool wraps pack tools received from the gateway for local execution.
// ABOUTME: Routes tool calls through gRPC to the gateway for pack execution.

use async_trait::async_trait;
use fold_proto::{AgentMessage, ExecutePackTool, PackToolResult, ToolDefinition, agent_message};
use mux::tool::{Tool, ToolResult};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};
use tracing::{info, warn};

/// Timeout for pack tool execution (5 minutes)
const PACK_TOOL_TIMEOUT_SECS: u64 = 300;

/// Shared state for pending pack tool requests - maps request_id to response sender
pub type PendingPackTools = Arc<Mutex<HashMap<String, oneshot::Sender<PackToolResult>>>>;

/// Creates a new empty pending pack tools map
pub fn new_pending_pack_tools() -> PendingPackTools {
    Arc::new(Mutex::new(HashMap::new()))
}

/// PackTool wraps a tool definition from the gateway and routes execution through gRPC
pub struct PackTool {
    name: String,
    description: String,
    schema: serde_json::Value,
    tx: mpsc::Sender<AgentMessage>,
    pending: PendingPackTools,
}

impl PackTool {
    /// Create a new PackTool from a gateway tool definition
    pub fn new(
        def: &ToolDefinition,
        tx: mpsc::Sender<AgentMessage>,
        pending: PendingPackTools,
    ) -> Self {
        // Parse the schema from JSON string
        let schema = serde_json::from_str(&def.input_schema_json).unwrap_or_else(|_| {
            serde_json::json!({
                "type": "object",
                "properties": {}
            })
        });

        Self {
            name: def.name.clone(),
            description: def.description.clone(),
            schema,
            tx,
            pending,
        }
    }
}

#[async_trait]
impl Tool for PackTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> serde_json::Value {
        self.schema.clone()
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        // Generate unique request ID
        let request_id = uuid::Uuid::new_v4().to_string();
        let started = std::time::Instant::now();

        info!(
            tool = %self.name,
            request_id = %request_id,
            "→ Pack tool execute"
        );

        // Create oneshot channel for response
        let (resp_tx, resp_rx) = oneshot::channel();

        // Register pending request
        {
            let mut pending = self.pending.lock().await;
            pending.insert(request_id.clone(), resp_tx);
        }

        // Serialize input to JSON
        let input_json = serde_json::to_string(&params)?;

        // Send execution request to gateway
        let msg = AgentMessage {
            payload: Some(agent_message::Payload::ExecutePackTool(ExecutePackTool {
                request_id: request_id.clone(),
                tool_name: self.name.clone(),
                input_json,
            })),
        };

        if let Err(e) = self.tx.send(msg).await {
            // Clean up pending entry
            let mut pending = self.pending.lock().await;
            pending.remove(&request_id);
            warn!(
                tool = %self.name,
                request_id = %request_id,
                error = %e,
                "✗ Pack tool send failed"
            );
            return Err(anyhow::anyhow!("Failed to send pack tool request: {}", e));
        }

        // Wait for response with timeout
        let timeout = tokio::time::Duration::from_secs(PACK_TOOL_TIMEOUT_SECS);
        match tokio::time::timeout(timeout, resp_rx).await {
            Ok(Ok(result)) => {
                let elapsed = started.elapsed();
                // Convert PackToolResult to ToolResult
                match result.result {
                    Some(fold_proto::pack_tool_result::Result::OutputJson(ref output)) => {
                        info!(
                            tool = %self.name,
                            request_id = %request_id,
                            duration_ms = elapsed.as_millis() as u64,
                            output_bytes = output.len(),
                            "← Pack tool result: success"
                        );
                        Ok(ToolResult::text(output.clone()))
                    }
                    Some(fold_proto::pack_tool_result::Result::Error(ref err)) => {
                        warn!(
                            tool = %self.name,
                            request_id = %request_id,
                            duration_ms = elapsed.as_millis() as u64,
                            error = %err,
                            "← Pack tool result: error"
                        );
                        Ok(ToolResult::error(err.clone()))
                    }
                    None => {
                        warn!(
                            tool = %self.name,
                            request_id = %request_id,
                            duration_ms = elapsed.as_millis() as u64,
                            "← Pack tool result: empty"
                        );
                        Ok(ToolResult::error("Empty result from pack".to_string()))
                    }
                }
            }
            Ok(Err(_)) => {
                let elapsed = started.elapsed();
                // Channel closed without response
                let mut pending = self.pending.lock().await;
                pending.remove(&request_id);
                warn!(
                    tool = %self.name,
                    request_id = %request_id,
                    duration_ms = elapsed.as_millis() as u64,
                    "✗ Pack tool channel closed unexpectedly"
                );
                Ok(ToolResult::error(
                    "Pack tool response channel closed unexpectedly".to_string(),
                ))
            }
            Err(_) => {
                // Timeout
                let mut pending = self.pending.lock().await;
                pending.remove(&request_id);
                warn!(
                    tool = %self.name,
                    request_id = %request_id,
                    timeout_secs = PACK_TOOL_TIMEOUT_SECS,
                    "✗ Pack tool timed out"
                );
                Ok(ToolResult::error(format!(
                    "Pack tool '{}' timed out after {} seconds",
                    self.name, PACK_TOOL_TIMEOUT_SECS
                )))
            }
        }
    }
}

/// Handle a PackToolResult message from the gateway.
/// Returns true if the result was delivered to a waiting caller.
pub async fn handle_pack_tool_result(pending: &PendingPackTools, result: PackToolResult) -> bool {
    let request_id = result.request_id.clone();
    let mut pending_guard = pending.lock().await;
    if let Some(sender) = pending_guard.remove(&request_id) {
        let delivered = sender.send(result).is_ok();
        if !delivered {
            warn!(request_id = %request_id, "Pack tool result receiver dropped");
        }
        delivered
    } else {
        warn!(request_id = %request_id, "Pack tool result for unknown request");
        false
    }
}
