// ABOUTME: Test pack providing echo tools for testing pack connectivity.
// ABOUTME: Registers echo and admin_echo tools with the gateway.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use coven_pack::{ManifestBuilder, PackClient, SchemaBuilder, ToolError, ToolHandler};
use coven_ssh::load_or_generate_key;
use serde::{Deserialize, Serialize};
use tracing::info;

const PACK_NAME: &str = "test-pack";

#[derive(Debug, Deserialize)]
struct EchoInput {
    message: String,
}

#[derive(Debug, Serialize)]
struct EchoOutput {
    echoed: String,
    tool: String,
}

struct TestHandler;

#[async_trait]
impl ToolHandler for TestHandler {
    async fn execute(&self, tool_name: &str, input_json: &str) -> Result<String, ToolError> {
        info!("Executing tool: {} with input: {}", tool_name, input_json);

        match tool_name {
            "echo" | "admin_echo" => {
                let input: EchoInput = serde_json::from_str(input_json)
                    .map_err(|e| ToolError::InvalidInput(e.to_string()))?;

                let output = EchoOutput {
                    echoed: input.message,
                    tool: tool_name.to_string(),
                };

                serde_json::to_string(&output)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
            }
            _ => Err(ToolError::UnknownTool(tool_name.to_string())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("test_pack=info".parse()?),
        )
        .init();

    let config = coven_pack::PackConfig::load(PACK_NAME).map_err(|e| anyhow!("{}", e))?;

    info!("Starting {}", PACK_NAME);
    info!("Gateway: {}", config.gateway_url);
    info!("SSH key: {}", config.ssh_key_path.display());

    // Load existing key or generate one
    let _private_key = load_or_generate_key(&config.ssh_key_path)?;

    let echo_schema = SchemaBuilder::object()
        .property(
            "message",
            SchemaBuilder::string().description("Message to echo"),
        )
        .required(&["message"])
        .build();

    let manifest = ManifestBuilder::new(PACK_NAME, "0.1.0")
        .tool("echo", "Echoes back the input message", &echo_schema, &[])
        .tool(
            "admin_echo",
            "Admin-only echo (requires admin capability)",
            &echo_schema,
            &["admin"],
        )
        .build();

    info!("Registering {} tools", manifest.tools.len());

    let client = PackClient::connect(&config.gateway_url, &config.ssh_key_path).await?;
    client.run(manifest, TestHandler).await?;

    Ok(())
}
