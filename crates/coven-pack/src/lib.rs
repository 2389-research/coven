// ABOUTME: Rust SDK for building tool packs that connect to coven-gateway.
// ABOUTME: Provides ManifestBuilder, ToolHandler trait, and PackClient for pack development.

//! # coven-pack
//!
//! A Rust SDK for building tool packs that provide tools to coven agents.
//!
//! Tool packs connect to coven-gateway and register a set of tools that agents
//! can invoke. When an agent uses a tool provided by a pack, the gateway routes
//! the execution request to the pack, which executes the tool and returns the result.
//!
//! ## Quick Start
//!
//! ```ignore
//! use coven_pack::{ManifestBuilder, PackClient, PackConfig, ToolHandler, ToolError};
//! use async_trait::async_trait;
//!
//! struct MyHandler;
//!
//! #[async_trait]
//! impl ToolHandler for MyHandler {
//!     async fn execute(&self, tool_name: &str, input_json: &str) -> Result<String, ToolError> {
//!         match tool_name {
//!             "greet" => Ok(r#"{"message": "Hello, world!"}"#.to_string()),
//!             _ => Err(ToolError::UnknownTool(tool_name.to_string())),
//!         }
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = PackConfig::load("my-pack")?;
//!
//!     let manifest = ManifestBuilder::new("my-pack", "1.0.0")
//!         .tool("greet", "Greets the user", r#"{"type": "object"}"#, &[])
//!         .build();
//!
//!     let client = PackClient::connect(
//!         &config.gateway_url,
//!         &config.ssh_key_path,
//!     ).await?;
//!
//!     client.run(manifest, MyHandler).await
//! }
//! ```

mod client;
mod config;
mod error;
mod handler;
mod manifest;

// Re-export primary types
pub use client::PackClient;
pub use config::PackConfig;
pub use error::{PackError, ToolError};
pub use handler::{FnHandler, ToolHandler};
pub use manifest::{ManifestBuilder, SchemaBuilder};

// Re-export proto types for convenience
pub use coven_proto::{ExecuteToolRequest, ExecuteToolResponse, PackManifest, ToolDefinition};
