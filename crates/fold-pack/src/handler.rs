// ABOUTME: ToolHandler trait for implementing tool execution logic.
// ABOUTME: Packs implement this trait to define how their tools execute.

use crate::error::ToolError;
use async_trait::async_trait;

/// Trait for handling tool execution requests.
///
/// Implement this trait to define how your pack's tools execute. The pack client
/// will call your implementation for each tool execution request received from
/// the gateway.
///
/// # Example
///
/// ```ignore
/// use fold_pack::{ToolHandler, ToolError};
/// use async_trait::async_trait;
/// use serde_json::{json, Value};
///
/// struct MyHandler;
///
/// #[async_trait]
/// impl ToolHandler for MyHandler {
///     async fn execute(&self, tool_name: &str, input_json: &str) -> Result<String, ToolError> {
///         // Parse the input
///         let input: Value = serde_json::from_str(input_json)
///             .map_err(|e| ToolError::InvalidInput(e.to_string()))?;
///
///         match tool_name {
///             "greet" => {
///                 let name = input["name"].as_str().unwrap_or("World");
///                 Ok(json!({"greeting": format!("Hello, {}!", name)}).to_string())
///             }
///             "add" => {
///                 let a = input["a"].as_i64()
///                     .ok_or_else(|| ToolError::InvalidInput("missing 'a'".into()))?;
///                 let b = input["b"].as_i64()
///                     .ok_or_else(|| ToolError::InvalidInput("missing 'b'".into()))?;
///                 Ok(json!({"sum": a + b}).to_string())
///             }
///             _ => Err(ToolError::UnknownTool(tool_name.to_string())),
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// Execute a tool with the given input.
    ///
    /// # Arguments
    ///
    /// * `tool_name` - The name of the tool to execute, as registered in the manifest
    /// * `input_json` - The tool input as a JSON string
    ///
    /// # Returns
    ///
    /// * `Ok(String)` - The tool output as a JSON string
    /// * `Err(ToolError)` - An error that occurred during execution
    ///
    /// # Errors
    ///
    /// Return appropriate `ToolError` variants for different failure modes:
    /// - `ToolError::UnknownTool` - The tool name is not recognized
    /// - `ToolError::InvalidInput` - The input JSON is malformed or missing required fields
    /// - `ToolError::ExecutionFailed` - The tool logic failed
    /// - `ToolError::Timeout` - The tool took too long to execute
    async fn execute(&self, tool_name: &str, input_json: &str) -> Result<String, ToolError>;

    /// Called when the pack successfully registers with the gateway.
    ///
    /// Override this method to perform any setup after registration completes.
    /// The default implementation does nothing.
    ///
    /// # Arguments
    ///
    /// * `pack_id` - The confirmed pack ID
    /// * `rejected_tools` - Tools that were rejected due to name collisions
    async fn on_registered(&self, _pack_id: &str, _rejected_tools: &[String]) {
        // Default: do nothing
    }

    /// Called when the connection is closing.
    ///
    /// Override this method to perform cleanup when the pack is disconnecting.
    /// The default implementation does nothing.
    ///
    /// # Arguments
    ///
    /// * `reason` - Optional reason for the disconnection
    async fn on_closing(&self, _reason: Option<&str>) {
        // Default: do nothing
    }
}

/// A simple function-based tool handler.
///
/// Useful for packs with simple, stateless tools that can be implemented
/// as a single function.
///
/// # Example
///
/// ```ignore
/// use fold_pack::{FnHandler, ToolError};
///
/// let handler = FnHandler::new(|tool_name, input_json| async move {
///     match tool_name {
///         "echo" => Ok(input_json.to_string()),
///         _ => Err(ToolError::UnknownTool(tool_name.to_string())),
///     }
/// });
/// ```
pub struct FnHandler<F> {
    func: F,
}

impl<F, Fut> FnHandler<F>
where
    F: Fn(&str, &str) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<String, ToolError>> + Send,
{
    /// Create a function-based handler.
    pub fn new(func: F) -> Self {
        Self { func }
    }
}

#[async_trait]
impl<F, Fut> ToolHandler for FnHandler<F>
where
    F: Fn(&str, &str) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<String, ToolError>> + Send,
{
    async fn execute(&self, tool_name: &str, input_json: &str) -> Result<String, ToolError> {
        (self.func)(tool_name, input_json).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHandler;

    #[async_trait]
    impl ToolHandler for TestHandler {
        async fn execute(&self, tool_name: &str, input_json: &str) -> Result<String, ToolError> {
            match tool_name {
                "echo" => Ok(input_json.to_string()),
                "fail" => Err(ToolError::ExecutionFailed(
                    "intentional failure".to_string(),
                )),
                _ => Err(ToolError::UnknownTool(tool_name.to_string())),
            }
        }
    }

    #[tokio::test]
    async fn test_tool_handler_execute() {
        let handler = TestHandler;

        // Test successful execution
        let result = handler.execute("echo", r#"{"message": "hello"}"#).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), r#"{"message": "hello"}"#);

        // Test execution failure
        let result = handler.execute("fail", "{}").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolError::ExecutionFailed(_)));

        // Test unknown tool
        let result = handler.execute("unknown", "{}").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ToolError::UnknownTool(_)));
    }

    #[tokio::test]
    async fn test_default_callbacks() {
        let handler = TestHandler;

        // Default callbacks should complete without error
        handler.on_registered("test-pack", &[]).await;
        handler
            .on_registered("test-pack", &["collision".to_string()])
            .await;
        handler.on_closing(None).await;
        handler.on_closing(Some("shutdown")).await;
    }

    #[tokio::test]
    async fn test_fn_handler() {
        // Need to convert to owned strings before the async block
        let handler = FnHandler::new(|tool_name: &str, input: &str| {
            let tool_name = tool_name.to_string();
            let input = input.to_string();
            async move {
                if tool_name == "reverse" {
                    Ok(input.chars().rev().collect())
                } else {
                    Err(ToolError::UnknownTool(tool_name))
                }
            }
        });

        let result = handler.execute("reverse", "hello").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "olleh");

        let result = handler.execute("unknown", "test").await;
        assert!(result.is_err());
    }
}
