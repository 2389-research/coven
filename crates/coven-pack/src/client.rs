// ABOUTME: PackClient for connecting to coven-gateway and serving tools.
// ABOUTME: Handles registration, authentication, and tool execution request streaming.

use crate::error::{PackError, ToolError};
use crate::handler::ToolHandler;
use coven_grpc::{create_channel, ChannelConfig};
use coven_proto::pack_service_client::PackServiceClient;
use coven_proto::{ExecuteToolResponse, PackManifest};
use coven_ssh::{load_key, PrivateKey, SshAuthCredentials};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::Channel;
use tracing::{debug, error, info, warn};

/// Credentials are refreshed when older than this many seconds.
/// Gateway rejects signatures older than 5 minutes (300s), so refresh at 4 minutes.
const CREDENTIAL_REFRESH_TTL_SECS: i64 = 240;

/// Client for connecting a tool pack to coven-gateway.
///
/// The pack client handles:
/// - SSH-based authentication with the gateway
/// - Registering the pack's manifest (tools)
/// - Receiving tool execution requests
/// - Sending tool execution results
///
/// # Example
///
/// ```ignore
/// use coven_pack::{PackClient, ManifestBuilder, ToolHandler, ToolError};
/// use async_trait::async_trait;
/// use std::path::PathBuf;
///
/// struct MyHandler;
///
/// #[async_trait]
/// impl ToolHandler for MyHandler {
///     async fn execute(&self, tool_name: &str, input_json: &str) -> Result<String, ToolError> {
///         Ok(r#"{"result": "ok"}"#.to_string())
///     }
/// }
///
/// let client = PackClient::connect(
///     "http://localhost:50051",
///     &PathBuf::from("~/.ssh/id_ed25519"),
/// ).await?;
///
/// let manifest = ManifestBuilder::new("my-pack", "1.0.0")
///     .tool("test", "Test tool", "{}", &[])
///     .build();
///
/// client.run(manifest, MyHandler).await?;
/// ```
pub struct PackClient {
    channel: Channel,
    private_key: PrivateKey,
    credentials: Arc<RwLock<SshAuthCredentials>>,
}

impl PackClient {
    /// Connect to a coven-gateway server.
    ///
    /// # Arguments
    ///
    /// * `url` - The gateway URL (e.g., "http://localhost:50051")
    /// * `ssh_key_path` - Path to the SSH private key for authentication
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The SSH key cannot be loaded
    /// - The key type is not supported (only ed25519 is supported)
    /// - Connection to the gateway fails
    pub async fn connect(url: &str, ssh_key_path: &Path) -> Result<Self, PackError> {
        // Load SSH key
        let private_key =
            load_key(ssh_key_path).map_err(|e| PackError::KeyLoadFailed(e.to_string()))?;

        // Create auth credentials
        let credentials = SshAuthCredentials::new(&private_key)?;

        // Create gRPC channel
        let config = ChannelConfig::new(url);
        let channel = create_channel(&config).await?;

        info!(
            url = url,
            key_path = %ssh_key_path.display(),
            "Pack client connected"
        );

        Ok(Self {
            channel,
            private_key,
            credentials: Arc::new(RwLock::new(credentials)),
        })
    }

    /// Refresh credentials if they are stale.
    ///
    /// This should be called before sending any request to the gateway.
    /// Credentials are refreshed if older than CREDENTIAL_REFRESH_TTL_SECS.
    async fn refresh_credentials_if_stale(&self) -> Result<(), PackError> {
        let creds = self.credentials.read().await;
        if creds.is_stale(CREDENTIAL_REFRESH_TTL_SECS) {
            drop(creds); // Release read lock before acquiring write lock
            let mut creds = self.credentials.write().await;
            // Double-check after acquiring write lock (another task may have refreshed)
            if creds.is_stale(CREDENTIAL_REFRESH_TTL_SECS) {
                let age = creds.age_secs();
                *creds = SshAuthCredentials::new(&self.private_key)?;
                debug!(old_age_secs = age, "Refreshed stale SSH credentials");
            }
        }
        Ok(())
    }

    /// Run the pack, handling tool execution requests.
    ///
    /// This method:
    /// 1. Registers the pack's manifest with the gateway
    /// 2. Receives tool execution requests via streaming
    /// 3. Calls the handler for each request
    /// 4. Sends results back to the gateway
    ///
    /// This method runs until the connection is closed or an error occurs.
    ///
    /// # Arguments
    ///
    /// * `manifest` - The pack manifest describing available tools
    /// * `handler` - The handler that implements tool execution logic
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Registration fails
    /// - The stream is closed unexpectedly
    /// - A fatal error occurs during tool execution
    pub async fn run<H: ToolHandler>(
        &self,
        manifest: PackManifest,
        handler: H,
    ) -> Result<(), PackError> {
        let handler = Arc::new(handler);
        let pack_id = manifest.pack_id.clone();

        info!(
            pack_id = %pack_id,
            version = %manifest.version,
            tools = manifest.tools.len(),
            "Registering pack with gateway"
        );

        // Create the gRPC client
        let mut client = PackServiceClient::new(self.channel.clone());

        // Refresh credentials if stale before registration
        self.refresh_credentials_if_stale().await?;

        // Create a request with auth credentials
        let mut request = tonic::Request::new(manifest);
        {
            let creds = self.credentials.read().await;
            creds.apply_to_request(&mut request)?;
        }

        // Register and get the execution request stream
        let response = client
            .register(request)
            .await
            .map_err(|e| PackError::RegistrationRejected(e.to_string()))?;

        let mut stream = response.into_inner();

        info!(pack_id = %pack_id, "Pack registered, waiting for tool requests");

        // Notify handler of successful registration
        // The gateway doesn't send a PackWelcome in the current protocol,
        // so we use the pack_id from the manifest
        handler.on_registered(&pack_id, &[]).await;

        // Process tool execution requests
        loop {
            match stream.message().await {
                Ok(Some(request)) => {
                    let request_id = request.request_id.clone();
                    let tool_name = request.tool_name.clone();

                    info!(
                        pack_id = %pack_id,
                        request_id = %request_id,
                        tool = %tool_name,
                        "-> Tool execute"
                    );

                    // Execute the tool
                    let started = std::time::Instant::now();
                    let result = handler.execute(&tool_name, &request.input_json).await;
                    let elapsed = started.elapsed();

                    // Build the response
                    let response = match result {
                        Ok(ref output) => {
                            info!(
                                pack_id = %pack_id,
                                request_id = %request_id,
                                tool = %tool_name,
                                duration_ms = elapsed.as_millis() as u64,
                                output_bytes = output.len(),
                                "<- Tool result: success"
                            );
                            ExecuteToolResponse {
                                request_id,
                                result: Some(
                                    coven_proto::execute_tool_response::Result::OutputJson(output.clone()),
                                ),
                            }
                        }
                        Err(e) => {
                            warn!(
                                pack_id = %pack_id,
                                request_id = %request_id,
                                tool = %tool_name,
                                duration_ms = elapsed.as_millis() as u64,
                                error = %e,
                                "<- Tool result: error"
                            );
                            ExecuteToolResponse {
                                request_id,
                                result: Some(coven_proto::execute_tool_response::Result::Error(
                                    format_tool_error(&e),
                                )),
                            }
                        }
                    };

                    // Send the result back
                    self.send_result(&pack_id, response).await?;
                }
                Ok(None) => {
                    info!(pack_id = %pack_id, "Stream closed by gateway");
                    handler.on_closing(Some("stream closed")).await;
                    break;
                }
                Err(e) => {
                    error!(pack_id = %pack_id, error = %e, "Stream error");
                    handler.on_closing(Some(&e.to_string())).await;
                    return Err(PackError::StreamError(e.to_string()));
                }
            }
        }

        Ok(())
    }

    /// Send a tool execution result back to the gateway.
    async fn send_result(
        &self,
        pack_id: &str,
        response: ExecuteToolResponse,
    ) -> Result<(), PackError> {
        let mut client = PackServiceClient::new(self.channel.clone());

        // Refresh credentials if stale before sending result
        self.refresh_credentials_if_stale().await?;

        let mut request = tonic::Request::new(response);
        {
            let creds = self.credentials.read().await;
            creds.apply_to_request(&mut request)?;
        }

        client.tool_result(request).await.map_err(|e| {
            error!(pack_id = %pack_id, error = %e, "Failed to send tool result");
            PackError::StreamError(e.to_string())
        })?;

        Ok(())
    }
}

/// Format a ToolError for transmission to the gateway.
fn format_tool_error(error: &ToolError) -> String {
    match error {
        ToolError::UnknownTool(name) => format!("unknown tool: {}", name),
        ToolError::InvalidInput(msg) => format!("invalid input: {}", msg),
        ToolError::ExecutionFailed(msg) => format!("execution failed: {}", msg),
        ToolError::Timeout => "execution timed out".to_string(),
        ToolError::MissingCapability(cap) => format!("missing capability: {}", cap),
        ToolError::Internal(msg) => format!("internal error: {}", msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tool_error() {
        assert_eq!(
            format_tool_error(&ToolError::UnknownTool("search".to_string())),
            "unknown tool: search"
        );
        assert_eq!(
            format_tool_error(&ToolError::InvalidInput("missing field".to_string())),
            "invalid input: missing field"
        );
        assert_eq!(
            format_tool_error(&ToolError::ExecutionFailed(
                "connection refused".to_string()
            )),
            "execution failed: connection refused"
        );
        assert_eq!(
            format_tool_error(&ToolError::Timeout),
            "execution timed out"
        );
        assert_eq!(
            format_tool_error(&ToolError::MissingCapability("web".to_string())),
            "missing capability: web"
        );
        assert_eq!(
            format_tool_error(&ToolError::Internal("panic".to_string())),
            "internal error: panic"
        );
    }
}
