// ABOUTME: HTTP MCP client for connecting to gateway pack tools endpoint.
// ABOUTME: Implements JSON-RPC 2.0 over HTTP to access pack tools.

use anyhow::{Context, Result};
use async_trait::async_trait;
use mux::tool::{Tool, ToolResult};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// HTTP MCP client for connecting to gateway pack tools.
pub struct HttpMcpClient {
    client: Client,
    base_url: String,
    token: String,
}

/// JSON-RPC 2.0 request
#[derive(Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 response
#[derive(Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: u64,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

/// JSON-RPC error
#[derive(Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: i32,
    message: String,
    #[allow(dead_code)]
    data: Option<serde_json::Value>,
}

/// MCP tool definition from tools/list
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// MCP tools/list response
#[derive(Deserialize)]
struct ToolsListResult {
    tools: Vec<McpToolInfo>,
}

/// MCP tools/call response content
#[derive(Deserialize)]
struct McpContent {
    #[allow(dead_code)]
    r#type: String,
    text: Option<String>,
}

/// MCP tools/call response
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolsCallResult {
    content: Vec<McpContent>,
    #[serde(default)]
    is_error: bool,
}

impl HttpMcpClient {
    /// Create a new HTTP MCP client.
    ///
    /// `base_url` should be the gateway MCP endpoint URL (e.g., "http://localhost:8080/mcp")
    /// `token` is the MCP access token from the gateway welcome message.
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            client,
            base_url: base_url.into(),
            token: token.into(),
        })
    }

    /// Build the URL with token as path segment (e.g., /mcp/<token>).
    /// Token is percent-encoded to prevent path traversal or query/fragment injection.
    fn url_with_token(&self) -> String {
        if let Ok(mut url) = url::Url::parse(&self.base_url) {
            let ok = url
                .path_segments_mut()
                .map(|mut seg| {
                    seg.pop_if_empty().push(&self.token);
                })
                .is_ok();
            if ok {
                return url.to_string();
            }
        }
        // Fallback for malformed or non-hierarchical URLs.
        // Insert token before the earliest query or fragment delimiter.
        let base = self.base_url.trim_end_matches('/');
        let path_end = [base.find('?'), base.find('#')]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(base.len());
        let path_part = base[..path_end].trim_end_matches('/');
        let encoded =
            percent_encoding::utf8_percent_encode(&self.token, percent_encoding::NON_ALPHANUMERIC);
        format!("{}/{}{}", path_part, encoded, &base[path_end..])
    }

    /// Initialize the MCP connection (handshake).
    pub async fn initialize(&self) -> Result<()> {
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "initialize",
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "coven-agent-mux",
                    "version": "1.0.0"
                }
            })),
        };

        let resp: JsonRpcResponse = self
            .client
            .post(self.url_with_token())
            .json(&req)
            .send()
            .await
            .context("Failed to send initialize request")?
            .json()
            .await
            .context("Failed to parse initialize response")?;

        if let Some(err) = resp.error {
            anyhow::bail!("MCP initialize failed: {}", err.message);
        }

        tracing::debug!("MCP connection initialized");
        Ok(())
    }

    /// List available tools from the MCP server.
    pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>> {
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 2,
            method: "tools/list",
            params: None,
        };

        let resp: JsonRpcResponse = self
            .client
            .post(self.url_with_token())
            .json(&req)
            .send()
            .await
            .context("Failed to send tools/list request")?
            .json()
            .await
            .context("Failed to parse tools/list response")?;

        if let Some(err) = resp.error {
            anyhow::bail!("MCP tools/list failed: {}", err.message);
        }

        let result: ToolsListResult = serde_json::from_value(resp.result.unwrap_or_default())
            .context("Failed to parse tools list")?;

        Ok(result.tools)
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<(String, bool)> {
        static REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(100);

        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            method: "tools/call",
            params: Some(serde_json::json!({
                "name": name,
                "arguments": arguments
            })),
        };

        let resp: JsonRpcResponse = self
            .client
            .post(self.url_with_token())
            .json(&req)
            .send()
            .await
            .context("Failed to send tools/call request")?
            .json()
            .await
            .context("Failed to parse tools/call response")?;

        if let Some(err) = resp.error {
            return Ok((format!("MCP error: {}", err.message), true));
        }

        let result: ToolsCallResult = serde_json::from_value(resp.result.unwrap_or_default())
            .context("Failed to parse tool result")?;

        // Combine all text content
        let text = result
            .content
            .iter()
            .filter_map(|c| c.text.as_ref())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");

        Ok((text, result.is_error))
    }
}

/// A remote tool that proxies calls to the gateway MCP server.
pub struct RemoteMcpTool {
    client: Arc<HttpMcpClient>,
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

impl RemoteMcpTool {
    /// Create a new remote tool from MCP tool info.
    pub fn new(client: Arc<HttpMcpClient>, info: McpToolInfo) -> Self {
        Self {
            client,
            name: info.name,
            description: info.description,
            input_schema: info.input_schema,
        }
    }
}

#[async_trait]
impl Tool for RemoteMcpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> serde_json::Value {
        self.input_schema.clone()
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult> {
        match self.client.call_tool(&self.name, params).await {
            Ok((output, is_error)) => {
                if is_error {
                    Ok(ToolResult::error(output))
                } else {
                    Ok(ToolResult::text(output))
                }
            }
            Err(e) => Ok(ToolResult::error(format!("Failed to call tool: {}", e))),
        }
    }
}

/// Connect to a gateway MCP endpoint and register all available tools with the registry.
///
/// Returns the number of tools registered.
pub async fn connect_gateway_mcp(
    registry: &mux::tool::Registry,
    mcp_url: &str,
    token: &str,
    prefix: Option<&str>,
) -> Result<usize> {
    let client = HttpMcpClient::new(mcp_url, token)?;

    // Initialize connection
    client.initialize().await?;

    // List available tools
    let tools = client.list_tools().await?;
    let tool_count = tools.len();

    if tool_count == 0 {
        tracing::warn!("Gateway MCP returned no tools");
        return Ok(0);
    }

    tracing::info!(
        tool_count = tool_count,
        "Gateway MCP returned {} tools",
        tool_count
    );

    // Create shared client
    let client = Arc::new(client);

    // Register each tool
    for info in tools {
        let tool_name = match prefix {
            Some(p) => format!("{}_{}", p, info.name),
            None => info.name.clone(),
        };

        tracing::debug!(
            name = %info.name,
            prefixed_name = %tool_name,
            "Registering gateway pack tool"
        );

        let mut tool = RemoteMcpTool::new(Arc::clone(&client), info);
        if prefix.is_some() {
            // Override name with prefixed version
            tool.name = tool_name;
        }

        registry.register(tool).await;
    }

    Ok(tool_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_with_token() {
        let client = HttpMcpClient::new("http://localhost:8080/mcp", "test-token").unwrap();
        assert_eq!(
            client.url_with_token(),
            "http://localhost:8080/mcp/test-token"
        );
    }

    #[test]
    fn test_url_with_token_special_chars() {
        let client =
            HttpMcpClient::new("http://localhost:8080/mcp", "abc/def?key=val#frag").unwrap();
        let url = client.url_with_token();
        // Path-separator, query, and fragment delimiters must be percent-encoded
        assert!(!url.contains("?key=val"), "query delimiter must be encoded");
        assert!(!url.contains("#frag"), "fragment delimiter must be encoded");
        // The url crate encodes /, ?, # but leaves = (which is valid in path segments)
        assert!(url.contains("abc%2Fdef%3Fkey=val%23frag"));
    }

    #[test]
    fn test_url_with_token_fallback_query() {
        // Non-hierarchical URL triggers fallback path
        let client = HttpMcpClient::new("data:text/plain?x=1", "tok").unwrap();
        let url = client.url_with_token();
        // Token should be inserted before the query delimiter
        assert!(url.starts_with("data:text/plain/"));
        assert!(url.contains("tok"));
        assert!(url.ends_with("?x=1"));
    }

    #[test]
    fn test_url_with_token_fallback_fragment_before_query() {
        // Edge case: fragment appears before query (malformed but possible)
        let client = HttpMcpClient::new("data:path#frag?x=1", "tok").unwrap();
        let url = client.url_with_token();
        // Token should be inserted before the earliest delimiter (#)
        assert!(
            url.contains("path/") && url.contains("#frag?x=1"),
            "token should be before # not between # and ?: got {}",
            url
        );
    }

    #[test]
    fn test_url_with_token_fallback_fragment_only() {
        let client = HttpMcpClient::new("data:path#frag", "tok").unwrap();
        let url = client.url_with_token();
        assert!(url.contains("path/"));
        assert!(url.ends_with("#frag"));
    }
}
