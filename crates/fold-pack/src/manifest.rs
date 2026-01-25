// ABOUTME: Fluent builder for PackManifest construction.
// ABOUTME: Provides ergonomic API for defining pack tools with JSON schemas.

use fold_proto::{PackManifest, ToolDefinition};

/// Builder for constructing a `PackManifest`.
///
/// Provides a fluent API for defining the tools provided by a pack.
///
/// # Example
///
/// ```
/// use fold_pack::ManifestBuilder;
///
/// let manifest = ManifestBuilder::new("my-pack", "1.0.0")
///     .tool("search", "Search for documents", r#"{"type": "object", "properties": {"query": {"type": "string"}}}"#, &["web"])
///     .tool_with_timeout("fetch", "Fetch a URL", r#"{"type": "object"}"#, &["web"], 60)
///     .build();
///
/// assert_eq!(manifest.pack_id, "my-pack");
/// assert_eq!(manifest.version, "1.0.0");
/// assert_eq!(manifest.tools.len(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct ManifestBuilder {
    pack_id: String,
    version: String,
    tools: Vec<ToolDefinition>,
}

impl ManifestBuilder {
    /// Create a builder for a pack manifest.
    ///
    /// # Arguments
    ///
    /// * `pack_id` - Unique identifier for the pack (e.g., "elevenlabs", "github")
    /// * `version` - Semantic version of the pack (e.g., "1.0.0")
    pub fn new(pack_id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            pack_id: pack_id.into(),
            version: version.into(),
            tools: Vec::new(),
        }
    }

    /// Add a tool to the manifest.
    ///
    /// # Arguments
    ///
    /// * `name` - Tool name (should be unique within the pack)
    /// * `description` - Human-readable description of what the tool does
    /// * `input_schema_json` - JSON Schema defining the tool's input parameters
    /// * `required_capabilities` - Capabilities required to use this tool
    ///
    /// # Returns
    ///
    /// Self for method chaining.
    ///
    /// # Example
    ///
    /// ```
    /// use fold_pack::ManifestBuilder;
    ///
    /// let manifest = ManifestBuilder::new("my-pack", "1.0.0")
    ///     .tool(
    ///         "search",
    ///         "Search the web for information",
    ///         r#"{
    ///             "type": "object",
    ///             "properties": {
    ///                 "query": {"type": "string", "description": "Search query"},
    ///                 "limit": {"type": "integer", "default": 10}
    ///             },
    ///             "required": ["query"]
    ///         }"#,
    ///         &["web"],
    ///     )
    ///     .build();
    /// ```
    pub fn tool(
        self,
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema_json: impl Into<String>,
        required_capabilities: &[&str],
    ) -> Self {
        self.tool_with_timeout(
            name,
            description,
            input_schema_json,
            required_capabilities,
            30,
        )
    }

    /// Add a tool to the manifest with a custom timeout.
    ///
    /// # Arguments
    ///
    /// * `name` - Tool name (should be unique within the pack)
    /// * `description` - Human-readable description of what the tool does
    /// * `input_schema_json` - JSON Schema defining the tool's input parameters
    /// * `required_capabilities` - Capabilities required to use this tool
    /// * `timeout_seconds` - Maximum execution time in seconds
    ///
    /// # Returns
    ///
    /// Self for method chaining.
    pub fn tool_with_timeout(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema_json: impl Into<String>,
        required_capabilities: &[&str],
        timeout_seconds: i32,
    ) -> Self {
        self.tools.push(ToolDefinition {
            name: name.into(),
            description: description.into(),
            input_schema_json: input_schema_json.into(),
            required_capabilities: required_capabilities
                .iter()
                .map(|s| s.to_string())
                .collect(),
            timeout_seconds,
        });
        self
    }

    /// Add a pre-built ToolDefinition to the manifest.
    ///
    /// Useful when you have a ToolDefinition from another source.
    pub fn add_tool(mut self, tool: ToolDefinition) -> Self {
        self.tools.push(tool);
        self
    }

    /// Build the final PackManifest.
    pub fn build(self) -> PackManifest {
        PackManifest {
            pack_id: self.pack_id,
            version: self.version,
            tools: self.tools,
        }
    }

    /// Get the current pack ID.
    pub fn pack_id(&self) -> &str {
        &self.pack_id
    }

    /// Get the current version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Get the current tools.
    pub fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }
}

/// Helper for building JSON schemas for tool inputs.
///
/// Provides convenience methods for common schema patterns.
///
/// # Example
///
/// ```
/// use fold_pack::SchemaBuilder;
///
/// let schema = SchemaBuilder::object()
///     .property("query", SchemaBuilder::string().description("Search query"))
///     .property("limit", SchemaBuilder::integer().default_value(10))
///     .required(&["query"])
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct SchemaBuilder {
    value: serde_json::Value,
}

impl SchemaBuilder {
    /// Create an object schema.
    pub fn object() -> Self {
        Self {
            value: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    /// Create a string schema.
    pub fn string() -> Self {
        Self {
            value: serde_json::json!({"type": "string"}),
        }
    }

    /// Create an integer schema.
    pub fn integer() -> Self {
        Self {
            value: serde_json::json!({"type": "integer"}),
        }
    }

    /// Create a number schema.
    pub fn number() -> Self {
        Self {
            value: serde_json::json!({"type": "number"}),
        }
    }

    /// Create a boolean schema.
    pub fn boolean() -> Self {
        Self {
            value: serde_json::json!({"type": "boolean"}),
        }
    }

    /// Create an array schema with the given item type.
    pub fn array(items: SchemaBuilder) -> Self {
        Self {
            value: serde_json::json!({
                "type": "array",
                "items": items.value
            }),
        }
    }

    /// Add a description to this schema.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.value["description"] = serde_json::Value::String(desc.into());
        self
    }

    /// Add a default value to this schema.
    pub fn default_value(mut self, default: impl Into<serde_json::Value>) -> Self {
        self.value["default"] = default.into();
        self
    }

    /// Add a property to an object schema.
    pub fn property(mut self, name: impl Into<String>, schema: SchemaBuilder) -> Self {
        if let Some(props) = self.value.get_mut("properties") {
            props[name.into()] = schema.value;
        }
        self
    }

    /// Mark properties as required.
    pub fn required(mut self, required: &[&str]) -> Self {
        let required_vec: Vec<serde_json::Value> = required
            .iter()
            .map(|s| serde_json::Value::String(s.to_string()))
            .collect();
        self.value["required"] = serde_json::Value::Array(required_vec);
        self
    }

    /// Add an enum constraint.
    pub fn enum_values(mut self, values: &[&str]) -> Self {
        let enum_vec: Vec<serde_json::Value> = values
            .iter()
            .map(|s| serde_json::Value::String(s.to_string()))
            .collect();
        self.value["enum"] = serde_json::Value::Array(enum_vec);
        self
    }

    /// Build the schema as a JSON string.
    pub fn build(self) -> String {
        self.value.to_string()
    }

    /// Get the schema as a serde_json Value.
    pub fn into_value(self) -> serde_json::Value {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_builder_basic() {
        let manifest = ManifestBuilder::new("test-pack", "1.0.0")
            .tool("hello", "Says hello", r#"{"type": "object"}"#, &[])
            .build();

        assert_eq!(manifest.pack_id, "test-pack");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools[0].name, "hello");
        assert_eq!(manifest.tools[0].timeout_seconds, 30);
    }

    #[test]
    fn test_manifest_builder_multiple_tools() {
        let manifest = ManifestBuilder::new("multi-pack", "2.0.0")
            .tool("search", "Search", "{}", &["web"])
            .tool("fetch", "Fetch", "{}", &["web", "network"])
            .tool_with_timeout("long_task", "Long task", "{}", &[], 120)
            .build();

        assert_eq!(manifest.tools.len(), 3);
        assert_eq!(manifest.tools[0].required_capabilities, vec!["web"]);
        assert_eq!(
            manifest.tools[1].required_capabilities,
            vec!["web", "network"]
        );
        assert_eq!(manifest.tools[2].timeout_seconds, 120);
    }

    #[test]
    fn test_manifest_builder_add_tool() {
        let tool = ToolDefinition {
            name: "external".to_string(),
            description: "External tool".to_string(),
            input_schema_json: "{}".to_string(),
            required_capabilities: vec![],
            timeout_seconds: 45,
        };

        let manifest = ManifestBuilder::new("pack", "1.0.0").add_tool(tool).build();

        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools[0].name, "external");
        assert_eq!(manifest.tools[0].timeout_seconds, 45);
    }

    #[test]
    fn test_manifest_builder_accessors() {
        let builder = ManifestBuilder::new("test", "1.0.0")
            .tool("a", "A", "{}", &[])
            .tool("b", "B", "{}", &[]);

        assert_eq!(builder.pack_id(), "test");
        assert_eq!(builder.version(), "1.0.0");
        assert_eq!(builder.tools().len(), 2);
    }

    #[test]
    fn test_schema_builder_object() {
        let schema = SchemaBuilder::object()
            .property("name", SchemaBuilder::string().description("User name"))
            .property("age", SchemaBuilder::integer())
            .required(&["name"])
            .build();

        let parsed: serde_json::Value = serde_json::from_str(&schema).unwrap();
        assert_eq!(parsed["type"], "object");
        assert_eq!(parsed["properties"]["name"]["type"], "string");
        assert_eq!(parsed["properties"]["name"]["description"], "User name");
        assert_eq!(parsed["required"], serde_json::json!(["name"]));
    }

    #[test]
    fn test_schema_builder_array() {
        let schema = SchemaBuilder::array(SchemaBuilder::string()).build();

        let parsed: serde_json::Value = serde_json::from_str(&schema).unwrap();
        assert_eq!(parsed["type"], "array");
        assert_eq!(parsed["items"]["type"], "string");
    }

    #[test]
    fn test_schema_builder_with_default() {
        let schema = SchemaBuilder::integer().default_value(10).build();

        let parsed: serde_json::Value = serde_json::from_str(&schema).unwrap();
        assert_eq!(parsed["default"], 10);
    }

    #[test]
    fn test_schema_builder_enum() {
        let schema = SchemaBuilder::string()
            .enum_values(&["small", "medium", "large"])
            .build();

        let parsed: serde_json::Value = serde_json::from_str(&schema).unwrap();
        assert_eq!(
            parsed["enum"],
            serde_json::json!(["small", "medium", "large"])
        );
    }

    #[test]
    fn test_schema_builder_into_value() {
        let value = SchemaBuilder::boolean().into_value();
        assert_eq!(value["type"], "boolean");
    }

    #[test]
    fn test_all_primitive_types() {
        let string_schema = SchemaBuilder::string().build();
        let int_schema = SchemaBuilder::integer().build();
        let num_schema = SchemaBuilder::number().build();
        let bool_schema = SchemaBuilder::boolean().build();

        assert!(string_schema.contains("\"type\":\"string\""));
        assert!(int_schema.contains("\"type\":\"integer\""));
        assert!(num_schema.contains("\"type\":\"number\""));
        assert!(bool_schema.contains("\"type\":\"boolean\""));
    }
}
