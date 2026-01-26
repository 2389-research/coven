// ABOUTME: Mux-compatible dispatch tools for swarm management.
// ABOUTME: Provides list_agents, create_workspace, delete_workspace tools via Unix socket.

use async_trait::async_trait;
use mux::tool::{Tool, ToolResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Socket request types (must match supervisor/socket.rs)
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum Request {
    #[serde(rename = "list")]
    List,
    #[serde(rename = "create")]
    Create { name: String },
    #[serde(rename = "delete")]
    Delete { name: String },
}

/// Socket response (must match supervisor/socket.rs)
#[derive(Debug, Deserialize)]
struct Response {
    success: bool,
    error: Option<String>,
    workspaces: Option<Vec<String>>,
    agent_id: Option<String>,
}

/// Get the socket path for a given prefix
fn socket_path(prefix: &str) -> PathBuf {
    PathBuf::from(format!("/tmp/fold-swarm-{}.sock", prefix))
}

/// Send a request to the supervisor socket and get the response
async fn send_request(prefix: &str, request: Request) -> Result<Response, anyhow::Error> {
    let path = socket_path(prefix);
    let stream = UnixStream::connect(&path).await?;
    let (reader, mut writer) = stream.into_split();

    let request_json = serde_json::to_string(&request)? + "\n";
    writer.write_all(request_json.as_bytes()).await?;

    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    reader.read_line(&mut line).await?;

    let response: Response = serde_json::from_str(&line)?;
    Ok(response)
}

/// List all agents in the swarm
pub struct ListAgentsTool {
    prefix: String,
}

impl ListAgentsTool {
    pub fn new(prefix: String) -> Self {
        Self { prefix }
    }
}

#[async_trait]
impl Tool for ListAgentsTool {
    fn name(&self) -> &str {
        "list_agents"
    }

    fn description(&self) -> &str {
        "List all agents currently running in the swarm. Returns agent IDs in the format 'prefix_workspace'."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        match send_request(&self.prefix, Request::List).await {
            Ok(response) => {
                if response.success {
                    let agents = response.workspaces.unwrap_or_default();
                    let agent_ids: Vec<String> = agents
                        .iter()
                        .map(|w| format!("{}_{}", self.prefix, w))
                        .collect();

                    if agent_ids.is_empty() {
                        Ok(ToolResult::text("No agents currently running."))
                    } else {
                        Ok(ToolResult::text(format!(
                            "Running agents ({}):\n{}",
                            agent_ids.len(),
                            agent_ids.join("\n")
                        )))
                    }
                } else {
                    Ok(ToolResult::error(
                        response.error.unwrap_or_else(|| "Unknown error".to_string())
                    ))
                }
            }
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to connect to supervisor: {}. Is fold-swarm supervisor running?",
                e
            ))),
        }
    }
}

/// Create a new workspace and spawn an agent for it
pub struct CreateWorkspaceTool {
    prefix: String,
}

impl CreateWorkspaceTool {
    pub fn new(prefix: String) -> Self {
        Self { prefix }
    }
}

#[async_trait]
impl Tool for CreateWorkspaceTool {
    fn name(&self) -> &str {
        "create_workspace"
    }

    fn description(&self) -> &str {
        "Create a new workspace directory and spawn an agent for it. The workspace name should be a simple identifier (letters, numbers, underscores, hyphens)."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the workspace to create (e.g., 'research', 'frontend', 'api-server')"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            name: String,
        }
        let params: Params = serde_json::from_value(params)?;

        // Validate workspace name
        if params.name.is_empty() {
            return Ok(ToolResult::error("Workspace name cannot be empty"));
        }
        if params.name.contains('/') || params.name.contains("..") {
            return Ok(ToolResult::error("Workspace name cannot contain '/' or '..'"));
        }

        match send_request(&self.prefix, Request::Create { name: params.name.clone() }).await {
            Ok(response) => {
                if response.success {
                    let agent_id = response.agent_id.unwrap_or_else(|| {
                        format!("{}_{}", self.prefix, params.name)
                    });
                    Ok(ToolResult::text(format!(
                        "Successfully created workspace '{}' and spawned agent '{}'",
                        params.name, agent_id
                    )))
                } else {
                    Ok(ToolResult::error(
                        response.error.unwrap_or_else(|| "Unknown error".to_string())
                    ))
                }
            }
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to connect to supervisor: {}. Is fold-swarm supervisor running?",
                e
            ))),
        }
    }
}

/// Delete a workspace (stops the agent, does not delete files)
pub struct DeleteWorkspaceTool {
    prefix: String,
}

impl DeleteWorkspaceTool {
    pub fn new(prefix: String) -> Self {
        Self { prefix }
    }
}

#[async_trait]
impl Tool for DeleteWorkspaceTool {
    fn name(&self) -> &str {
        "delete_workspace"
    }

    fn description(&self) -> &str {
        "Stop an agent and remove its workspace from the swarm. This does NOT delete the workspace files on disk - only stops the agent process."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the workspace to delete (the workspace part, not the full agent ID)"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            name: String,
        }
        let params: Params = serde_json::from_value(params)?;

        if params.name.is_empty() {
            return Ok(ToolResult::error("Workspace name cannot be empty"));
        }

        match send_request(&self.prefix, Request::Delete { name: params.name.clone() }).await {
            Ok(response) => {
                if response.success {
                    Ok(ToolResult::text(format!(
                        "Successfully stopped agent for workspace '{}'. The workspace files remain on disk.",
                        params.name
                    )))
                } else {
                    Ok(ToolResult::error(
                        response.error.unwrap_or_else(|| "Unknown error".to_string())
                    ))
                }
            }
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to connect to supervisor: {}. Is fold-swarm supervisor running?",
                e
            ))),
        }
    }
}
