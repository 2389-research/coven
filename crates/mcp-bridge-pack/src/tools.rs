// ABOUTME: Dynamic tool registration from MCP server capabilities.
// ABOUTME: Converts MCP tools/resources/prompts to fold-pack tool definitions.

use crate::mcp_client::McpTool;
use fold_proto::ToolDefinition;

/// Convert MCP tools to fold ToolDefinitions.
pub fn mcp_tools_to_definitions(tools: &[McpTool]) -> Vec<ToolDefinition> {
    tools
        .iter()
        .map(|tool| ToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone().unwrap_or_default(),
            input_schema_json: tool
                .input_schema
                .as_ref()
                .map(|s| serde_json::to_string(s).unwrap_or_default())
                .unwrap_or_else(|| r#"{"type": "object"}"#.to_string()),
            required_capabilities: vec![],
            timeout_seconds: 60,
        })
        .collect()
}

/// Create tool definitions for MCP resource operations.
pub fn resource_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "mcp_list_resources".to_string(),
            description: "List available MCP resources".to_string(),
            input_schema_json: r#"{"type": "object"}"#.to_string(),
            required_capabilities: vec![],
            timeout_seconds: 30,
        },
        ToolDefinition {
            name: "mcp_read_resource".to_string(),
            description: "Read an MCP resource by URI".to_string(),
            input_schema_json: r#"{"type": "object", "properties": {"uri": {"type": "string", "description": "Resource URI"}}, "required": ["uri"]}"#.to_string(),
            required_capabilities: vec![],
            timeout_seconds: 60,
        },
    ]
}

/// Create tool definitions for MCP prompt operations.
pub fn prompt_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "mcp_list_prompts".to_string(),
            description: "List available MCP prompts".to_string(),
            input_schema_json: r#"{"type": "object"}"#.to_string(),
            required_capabilities: vec![],
            timeout_seconds: 30,
        },
        ToolDefinition {
            name: "mcp_get_prompt".to_string(),
            description: "Get an MCP prompt by name".to_string(),
            input_schema_json: r#"{"type": "object", "properties": {"name": {"type": "string", "description": "Prompt name"}, "arguments": {"type": "object", "description": "Prompt arguments"}}, "required": ["name"]}"#.to_string(),
            required_capabilities: vec![],
            timeout_seconds: 60,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_mcp_tools_to_definitions_empty() {
        let tools: Vec<McpTool> = vec![];
        let definitions = mcp_tools_to_definitions(&tools);
        assert!(definitions.is_empty());
    }

    #[test]
    fn test_mcp_tools_to_definitions_with_schema() {
        let tools = vec![McpTool {
            name: "test_tool".to_string(),
            description: Some("A test tool".to_string()),
            input_schema: Some(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                }
            })),
        }];

        let definitions = mcp_tools_to_definitions(&tools);
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].name, "test_tool");
        assert_eq!(definitions[0].description, "A test tool");
        assert!(definitions[0].input_schema_json.contains("query"));
        assert_eq!(definitions[0].timeout_seconds, 60);
    }

    #[test]
    fn test_mcp_tools_to_definitions_no_description() {
        let tools = vec![McpTool {
            name: "minimal".to_string(),
            description: None,
            input_schema: None,
        }];

        let definitions = mcp_tools_to_definitions(&tools);
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].name, "minimal");
        assert_eq!(definitions[0].description, "");
        assert_eq!(definitions[0].input_schema_json, r#"{"type": "object"}"#);
    }

    #[test]
    fn test_resource_tools() {
        let tools = resource_tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "mcp_list_resources");
        assert_eq!(tools[1].name, "mcp_read_resource");
        assert!(tools[1].input_schema_json.contains("uri"));
    }

    #[test]
    fn test_prompt_tools() {
        let tools = prompt_tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "mcp_list_prompts");
        assert_eq!(tools[1].name, "mcp_get_prompt");
        assert!(tools[1].input_schema_json.contains("name"));
        assert!(tools[1].input_schema_json.contains("arguments"));
    }
}
