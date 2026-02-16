// ABOUTME: MCP bridge pack that wraps any MCP server.
// ABOUTME: Dynamically discovers tools from MCP and exposes them to coven agents.

mod mcp_client;
mod tools;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use coven_pack::{ManifestBuilder, PackClient, ToolError, ToolHandler};
use coven_ssh::{load_or_generate_key, xdg_config_dir};
use mcp_client::McpClient;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

const DEFAULT_PACK_ID: &str = "mcp-bridge";

/// Get the default SSH key path for this pack (~/.config/coven/packs/<pack-id>/id_ed25519).
fn default_pack_key_path(pack_id: &str) -> Option<PathBuf> {
    xdg_config_dir().map(|p| p.join("packs").join(pack_id).join("id_ed25519"))
}

/// Handler that proxies tool calls to the underlying MCP server.
struct McpBridgeHandler {
    client: Arc<RwLock<McpClient>>,
    mcp_tool_names: Vec<String>,
}

#[async_trait]
impl ToolHandler for McpBridgeHandler {
    async fn execute(&self, tool_name: &str, input_json: &str) -> Result<String, ToolError> {
        info!(tool = %tool_name, "Executing MCP tool");

        let client = self.client.read().await;
        let input: Value =
            serde_json::from_str(input_json).map_err(|e| ToolError::InvalidInput(e.to_string()))?;

        let result = match tool_name {
            "mcp_list_resources" => {
                let resources = client
                    .list_resources()
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                serde_json::to_value(&resources)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            }
            "mcp_read_resource" => {
                let uri = input
                    .get("uri")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::InvalidInput("uri required".to_string()))?;
                let result = client
                    .read_resource(uri)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                serde_json::to_value(&result)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            }
            "mcp_list_prompts" => {
                let prompts = client
                    .list_prompts()
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                serde_json::to_value(&prompts)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            }
            "mcp_get_prompt" => {
                let name = input
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::InvalidInput("name required".to_string()))?;
                let arguments: Option<HashMap<String, String>> = input
                    .get("arguments")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                let result = client
                    .get_prompt(name, arguments)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                serde_json::to_value(&result)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            }
            _ => {
                // All other tools are proxied to MCP tools/call
                if !self.mcp_tool_names.contains(&tool_name.to_string()) {
                    return Err(ToolError::UnknownTool(tool_name.to_string()));
                }

                let arguments = if input.is_object() && !input.as_object().unwrap().is_empty() {
                    Some(input)
                } else {
                    None
                };

                let result = client
                    .call_tool(tool_name, arguments)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

                serde_json::to_value(&result)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            }
        };

        serde_json::to_string(&result).map_err(|e| ToolError::ExecutionFailed(e.to_string()))
    }

    async fn on_registered(&self, pack_id: &str, rejected_tools: &[String]) {
        info!(pack_id = %pack_id, "MCP bridge pack registered");
        if !rejected_tools.is_empty() {
            warn!(rejected = ?rejected_tools, "Some tools were rejected due to name collisions");
        }
    }

    async fn on_closing(&self, reason: Option<&str>) {
        info!(reason = ?reason, "MCP bridge pack closing");
        // Shutdown the MCP client
        let mut client = self.client.write().await;
        if let Err(e) = client.shutdown().await {
            error!(error = %e, "Failed to shutdown MCP client");
        }
    }
}

/// Parse the MCP server command from environment variable.
/// Expected format: "command arg1 arg2 ..." or just "command"
fn parse_mcp_command(cmd_str: &str) -> Result<(String, Vec<String>)> {
    let parts: Vec<&str> = cmd_str.split_whitespace().collect();
    if parts.is_empty() {
        return Err(anyhow!("MCP_SERVER_COMMAND is empty"));
    }

    let command = parts[0].to_string();
    let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

    Ok((command, args))
}

#[tokio::main]
async fn main() -> Result<()> {
    coven_log::init();

    // Read required environment variables
    let mcp_server_command = std::env::var("MCP_SERVER_COMMAND")
        .map_err(|_| anyhow!("MCP_SERVER_COMMAND environment variable is required"))?;

    // Optional: pack ID override (defaults to "mcp-bridge")
    let pack_id = std::env::var("MCP_PACK_ID").unwrap_or_else(|_| DEFAULT_PACK_ID.to_string());

    let gateway_addr =
        std::env::var("GATEWAY_ADDR").unwrap_or_else(|_| "http://localhost:50051".to_string());

    // Use PACK_SSH_KEY env var if set, otherwise use pack-specific XDG path
    let ssh_key_path = std::env::var("PACK_SSH_KEY")
        .map(PathBuf::from)
        .or_else(|_| {
            default_pack_key_path(&pack_id)
                .ok_or_else(|| anyhow!("Could not determine config directory for SSH key"))
        })?;

    info!(pack_id = %pack_id, "Starting MCP bridge pack");
    info!(gateway = %gateway_addr, "Gateway address");
    info!(ssh_key = %ssh_key_path.display(), "SSH key path");
    info!(command = %mcp_server_command, "MCP server command");

    // Load existing key or generate one
    let _private_key = load_or_generate_key(&ssh_key_path)?;

    // Parse the MCP server command
    let (command, args) = parse_mcp_command(&mcp_server_command)?;
    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    // Spawn and initialize the MCP client
    info!(command = %command, args = ?args_refs, "Spawning MCP server");
    let mut client = McpClient::spawn(&command, &args_refs, None).await?;

    let init_result = client.initialize().await?;
    info!(
        server_name = %init_result.server_info.name,
        server_version = ?init_result.server_info.version,
        "MCP server initialized"
    );

    // Discover tools from MCP server
    let mcp_tools = client.list_tools().await?;
    info!(count = mcp_tools.len(), "Discovered MCP tools");

    // Convert MCP tools to coven tool definitions
    let tool_definitions = tools::mcp_tools_to_definitions(&mcp_tools);
    let mcp_tool_names: Vec<String> = mcp_tools.iter().map(|t| t.name.clone()).collect();

    // Build the manifest
    let mut builder = ManifestBuilder::new(&pack_id, env!("CARGO_PKG_VERSION"));

    // Add discovered MCP tools
    for tool in tool_definitions {
        builder = builder.add_tool(tool);
    }

    // Add synthetic resource tools if server supports resources
    if client.has_resources() {
        info!("Server supports resources, adding resource tools");
        for tool in tools::resource_tools() {
            builder = builder.add_tool(tool);
        }
    }

    // Add synthetic prompt tools if server supports prompts
    if client.has_prompts() {
        info!("Server supports prompts, adding prompt tools");
        for tool in tools::prompt_tools() {
            builder = builder.add_tool(tool);
        }
    }

    let manifest = builder.build();
    info!(tools = manifest.tools.len(), "Built manifest with tools");

    // Create the handler with shared client
    let client = Arc::new(RwLock::new(client));
    let handler = McpBridgeHandler {
        client,
        mcp_tool_names,
    };

    // Connect to gateway and run
    let pack_client = PackClient::connect(&gateway_addr, &ssh_key_path).await?;
    pack_client.run(manifest, handler).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mcp_command_simple() {
        let (cmd, args) = parse_mcp_command("node").unwrap();
        assert_eq!(cmd, "node");
        assert!(args.is_empty());
    }

    #[test]
    fn test_parse_mcp_command_with_args() {
        let (cmd, args) = parse_mcp_command("npx -y @modelcontextprotocol/server-memory").unwrap();
        assert_eq!(cmd, "npx");
        assert_eq!(args, vec!["-y", "@modelcontextprotocol/server-memory"]);
    }

    #[test]
    fn test_parse_mcp_command_empty() {
        let result = parse_mcp_command("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_mcp_command_whitespace_only() {
        let result = parse_mcp_command("   ");
        assert!(result.is_err());
    }
}
