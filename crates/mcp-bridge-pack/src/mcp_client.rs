// ABOUTME: MCP JSON-RPC client for communicating with MCP servers via stdio.
// ABOUTME: Implements the Model Context Protocol for tool discovery and invocation.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tracing::{debug, trace, warn};

/// MCP protocol version we support.
const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// JSON-RPC 2.0 request structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::Number(id.into())),
            method: method.into(),
            params,
        }
    }

    pub fn notification(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 response structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for JsonRpcError {}

/// MCP tool definition from the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
}

/// MCP resource definition from the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// MCP prompt definition from the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPrompt {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<McpPromptArgument>>,
}

/// Argument definition for an MCP prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// Server capabilities returned from initialize.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    #[serde(default)]
    pub tools: Option<ToolsCapability>,
    #[serde(default)]
    pub resources: Option<ResourcesCapability>,
    #[serde(default)]
    pub prompts: Option<PromptsCapability>,
    #[serde(default)]
    pub logging: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCapability {
    #[serde(default)]
    pub list_changed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcesCapability {
    #[serde(default)]
    pub subscribe: bool,
    #[serde(default)]
    pub list_changed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptsCapability {
    #[serde(default)]
    pub list_changed: bool,
}

/// Client capabilities we advertise to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<Value>,
}

impl Default for ClientCapabilities {
    fn default() -> Self {
        Self {
            roots: Some(RootsCapability { list_changed: true }),
            sampling: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootsCapability {
    #[serde(default)]
    pub list_changed: bool,
}

/// Client information sent during initialize.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

impl Default for ClientInfo {
    fn default() -> Self {
        Self {
            name: "coven-mcp-bridge".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Server information received from initialize.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Initialize request params.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    pub client_info: ClientInfo,
}

/// Initialize response result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
}

/// Result of tools/list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListToolsResult {
    pub tools: Vec<McpTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Result of tools/call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    pub content: Vec<ToolContent>,
    #[serde(default)]
    pub is_error: bool,
}

/// Content item in a tool result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ToolContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    #[serde(rename = "resource")]
    Resource { resource: ResourceContent },
}

/// Embedded resource content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceContent {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

/// Result of resources/list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResourcesResult {
    pub resources: Vec<McpResource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Result of resources/read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadResourceResult {
    pub contents: Vec<ResourceContent>,
}

/// Result of prompts/list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPromptsResult {
    pub prompts: Vec<McpPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Result of prompts/get.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPromptResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub messages: Vec<PromptMessage>,
}

/// A message in a prompt result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptMessage {
    pub role: String,
    pub content: PromptContent,
}

/// Content of a prompt message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PromptContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    #[serde(rename = "resource")]
    Resource { resource: ResourceContent },
}

/// MCP client for communicating with an MCP server via stdio.
pub struct McpClient {
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: Arc<Mutex<BufReader<ChildStdout>>>,
    child: Arc<Mutex<Child>>,
    request_id: AtomicU64,
    server_capabilities: ServerCapabilities,
    server_info: Option<ServerInfo>,
    initialized: bool,
}

impl McpClient {
    /// Spawn an MCP server as a subprocess and return a client.
    pub async fn spawn(
        command: &str,
        args: &[&str],
        env: Option<HashMap<String, String>>,
    ) -> Result<Self> {
        debug!(
            command = command,
            args = ?args,
            "Spawning MCP server process"
        );

        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        if let Some(env_vars) = env {
            for (key, value) in env_vars {
                cmd.env(key, value);
            }
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn MCP server: {}", command))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to capture stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to capture stdout"))?;

        Ok(Self {
            stdin: Arc::new(Mutex::new(stdin)),
            stdout: Arc::new(Mutex::new(BufReader::new(stdout))),
            child: Arc::new(Mutex::new(child)),
            request_id: AtomicU64::new(1),
            server_capabilities: ServerCapabilities::default(),
            server_info: None,
            initialized: false,
        })
    }

    /// Perform the initialize handshake with the server.
    pub async fn initialize(&mut self) -> Result<InitializeResult> {
        let params = InitializeParams {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: ClientInfo::default(),
        };

        let response: InitializeResult = self.call("initialize", Some(params)).await?;

        debug!(
            server_name = %response.server_info.name,
            protocol_version = %response.protocol_version,
            "MCP server initialized"
        );

        self.server_capabilities = response.capabilities.clone();
        self.server_info = Some(response.server_info.clone());

        // Send initialized notification
        self.notify("notifications/initialized", None::<()>).await?;

        self.initialized = true;

        Ok(response)
    }

    /// Check if the client has been initialized.
    #[allow(dead_code)]
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Get the server capabilities.
    #[allow(dead_code)]
    pub fn capabilities(&self) -> &ServerCapabilities {
        &self.server_capabilities
    }

    /// Get the server info.
    #[allow(dead_code)]
    pub fn server_info(&self) -> Option<&ServerInfo> {
        self.server_info.as_ref()
    }

    /// Check if the server supports tools.
    pub fn has_tools(&self) -> bool {
        self.server_capabilities.tools.is_some()
    }

    /// Check if the server supports resources.
    pub fn has_resources(&self) -> bool {
        self.server_capabilities.resources.is_some()
    }

    /// Check if the server supports prompts.
    pub fn has_prompts(&self) -> bool {
        self.server_capabilities.prompts.is_some()
    }

    /// List available tools from the server.
    pub async fn list_tools(&self) -> Result<Vec<McpTool>> {
        if !self.has_tools() {
            return Ok(vec![]);
        }

        let result: ListToolsResult = self.call("tools/list", None::<()>).await?;
        Ok(result.tools)
    }

    /// Call a tool on the server.
    pub async fn call_tool(&self, name: &str, arguments: Option<Value>) -> Result<CallToolResult> {
        #[derive(Serialize)]
        struct CallToolParams {
            name: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            arguments: Option<Value>,
        }

        let params = CallToolParams {
            name: name.to_string(),
            arguments,
        };

        self.call("tools/call", Some(params)).await
    }

    /// List available resources from the server.
    pub async fn list_resources(&self) -> Result<Vec<McpResource>> {
        if !self.has_resources() {
            return Ok(vec![]);
        }

        let result: ListResourcesResult = self.call("resources/list", None::<()>).await?;
        Ok(result.resources)
    }

    /// Read a resource from the server.
    pub async fn read_resource(&self, uri: &str) -> Result<ReadResourceResult> {
        #[derive(Serialize)]
        struct ReadResourceParams {
            uri: String,
        }

        let params = ReadResourceParams {
            uri: uri.to_string(),
        };

        self.call("resources/read", Some(params)).await
    }

    /// List available prompts from the server.
    pub async fn list_prompts(&self) -> Result<Vec<McpPrompt>> {
        if !self.has_prompts() {
            return Ok(vec![]);
        }

        let result: ListPromptsResult = self.call("prompts/list", None::<()>).await?;
        Ok(result.prompts)
    }

    /// Get a prompt from the server.
    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<HashMap<String, String>>,
    ) -> Result<GetPromptResult> {
        #[derive(Serialize)]
        struct GetPromptParams {
            name: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            arguments: Option<HashMap<String, String>>,
        }

        let params = GetPromptParams {
            name: name.to_string(),
            arguments,
        };

        self.call("prompts/get", Some(params)).await
    }

    /// Make a JSON-RPC call and wait for the response.
    pub async fn call<P, R>(&self, method: &str, params: Option<P>) -> Result<R>
    where
        P: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let params_value = params
            .map(|p| serde_json::to_value(p))
            .transpose()
            .context("Failed to serialize params")?;

        let request = JsonRpcRequest::new(id, method, params_value);
        let response = self.send_request(request).await?;

        if let Some(error) = response.error {
            return Err(error.into());
        }

        let result = response
            .result
            .ok_or_else(|| anyhow!("Response missing result"))?;

        serde_json::from_value(result).context("Failed to deserialize response")
    }

    /// Send a notification (no response expected).
    pub async fn notify<P>(&self, method: &str, params: Option<P>) -> Result<()>
    where
        P: Serialize,
    {
        let params_value = params
            .map(|p| serde_json::to_value(p))
            .transpose()
            .context("Failed to serialize params")?;

        let notification = JsonRpcRequest::notification(method, params_value);
        self.send_message(&notification).await
    }

    /// Send a request and read the response.
    async fn send_request(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        let expected_id = request.id.clone();

        self.send_message(&request).await?;

        // Read responses until we get the one we're looking for
        loop {
            let response = self.read_response().await?;

            // Check if this is a notification (no id)
            if response.id.is_none() {
                trace!("Received notification, continuing to wait for response");
                continue;
            }

            // Check if this is our response
            if response.id == expected_id {
                return Ok(response);
            }

            warn!(
                expected = ?expected_id,
                received = ?response.id,
                "Received response with unexpected id"
            );
        }
    }

    /// Send a message to the server.
    async fn send_message<T: Serialize>(&self, message: &T) -> Result<()> {
        let json = serde_json::to_string(message).context("Failed to serialize message")?;
        trace!(message = %json, "Sending message");

        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(json.as_bytes())
            .await
            .context("Failed to write message")?;
        stdin
            .write_all(b"\n")
            .await
            .context("Failed to write newline")?;
        stdin.flush().await.context("Failed to flush stdin")?;

        Ok(())
    }

    /// Read a response from the server.
    async fn read_response(&self) -> Result<JsonRpcResponse> {
        let mut stdout = self.stdout.lock().await;
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = stdout
                .read_line(&mut line)
                .await
                .context("Failed to read from stdout")?;

            if bytes_read == 0 {
                return Err(anyhow!("MCP server closed connection"));
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            trace!(message = %trimmed, "Received message");

            let response: JsonRpcResponse =
                serde_json::from_str(trimmed).context("Failed to parse response")?;

            return Ok(response);
        }
    }

    /// Shutdown the MCP server gracefully.
    pub async fn shutdown(&mut self) -> Result<()> {
        debug!("Shutting down MCP client");

        // Try to kill the child process
        let mut child = self.child.lock().await;
        if let Err(e) = child.kill().await {
            warn!(error = %e, "Failed to kill MCP server process");
        }

        Ok(())
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        // Best-effort cleanup - we can't do async in Drop
        debug!("McpClient dropped");
    }
}
