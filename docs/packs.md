# Tool Packs

Packs are modular tool bundles that extend agent capabilities.

## Overview

Packs connect to coven-gateway and register tools that agents can use. When an agent needs to use a tool, the gateway routes the request to the appropriate pack.

```
Agent ──► Gateway ──► Pack
              │
              └──► Tool execution
              │
              └──► Result back to agent
```

## Available Packs

| Pack | Description |
|------|-------------|
| `coven-pack` | SDK for building custom packs |
| `mcp-bridge-pack` | Bridge to MCP (Model Context Protocol) servers |
| `productivity-pack` | Todo lists and notes |
| `test-pack` | Echo tools for testing |

## Using Packs

### Running a Pack

```bash
# Build and run
cargo run -p productivity-pack

# With custom gateway
GATEWAY_ADDR=mygateway:50051 cargo run -p productivity-pack
```

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `GATEWAY_ADDR` | Gateway gRPC address | `localhost:50051` |
| `PACK_SSH_KEY` | SSH key for auth | `~/.ssh/id_ed25519` |
| `RUST_LOG` | Log level | `info` |

## Building Custom Packs

### Quick Start

```rust
use coven_pack::{PackClient, ToolHandler, ManifestBuilder};
use async_trait::async_trait;

#[derive(Clone)]
struct MyPack;

#[async_trait]
impl ToolHandler for MyPack {
    async fn handle(
        &self,
        tool_name: &str,
        input: serde_json::Value,
    ) -> Result<String, String> {
        match tool_name {
            "my_tool" => {
                let arg = input["arg"].as_str().unwrap_or("default");
                Ok(format!("Result: {}", arg))
            }
            _ => Err(format!("Unknown tool: {}", tool_name)),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let manifest = ManifestBuilder::new("my-pack")
        .tool("my_tool", "Does something useful")
        .param("arg", "string", "Input argument", true)
        .build();

    let client = PackClient::connect("localhost:50051").await?;
    client.run(manifest, MyPack).await
}
```

### Manifest Builder

```rust
let manifest = ManifestBuilder::new("pack-name")
    // Simple tool
    .tool("tool_name", "Tool description")
    .param("param1", "string", "Parameter description", true)  // required
    .param("param2", "number", "Optional param", false)        // optional

    // Another tool
    .tool("other_tool", "Another tool")
    .param("input", "object", "JSON input", true)

    .build();
```

### Tool Handler Trait

```rust
#[async_trait]
pub trait ToolHandler: Clone + Send + Sync + 'static {
    async fn handle(
        &self,
        tool_name: &str,
        input: serde_json::Value,
    ) -> Result<String, String>;
}
```

### Pack Client

```rust
let client = PackClient::connect(gateway_addr).await?;

// Register and run (blocking)
client.run(manifest, handler).await?;

// Or register only
client.register(manifest).await?;

// Then handle requests manually
while let Some(request) = client.next_request().await {
    let result = handler.handle(&request.tool_name, request.input).await;
    client.send_result(request.id, result).await?;
}
```

## MCP Bridge Pack

Bridges any MCP (Model Context Protocol) server to coven.

### Configuration

```bash
# Stdio transport (spawn process)
MCP_SERVER_COMMAND="npx @modelcontextprotocol/server-filesystem /path" \
  cargo run -p mcp-bridge-pack

# HTTP transport
MCP_SERVER_URL="http://localhost:3000/mcp" \
  cargo run -p mcp-bridge-pack
```

### How It Works

1. Connects to MCP server (stdio or HTTP)
2. Discovers available tools via `tools/list`
3. Registers discovered tools with gateway
4. Proxies tool calls to MCP server

### Supported MCP Features

- Tool discovery and execution
- Stdio and HTTP transports
- JSON-RPC 2.0 protocol

## Productivity Pack

Provides todo list and notes management.

### Tools

| Tool | Description |
|------|-------------|
| `todo_add` | Add a todo item |
| `todo_list` | List todos |
| `todo_complete` | Mark todo as complete |
| `todo_delete` | Delete a todo |
| `note_create` | Create a note |
| `note_read` | Read a note |
| `note_list` | List all notes |
| `note_delete` | Delete a note |

### Storage

SQLite database at `~/.local/share/coven-packs/productivity.db`.

```bash
# Custom path
PRODUCTIVITY_DB_PATH=/path/to/db.sqlite cargo run -p productivity-pack
```

## Test Pack

Simple echo tools for testing pack connectivity.

### Tools

| Tool | Description |
|------|-------------|
| `echo` | Echoes input back |
| `admin_echo` | Echoes with admin prefix |

### Usage

```bash
cargo run -p test-pack
```

Useful for:
- Testing gateway pack registration
- Verifying agent tool execution
- Debugging pack communication

## Pack Protocol

### Registration

```protobuf
message PackManifest {
  string name = 1;
  repeated ToolDefinition tools = 2;
}

message ToolDefinition {
  string name = 1;
  string description = 2;
  repeated ParameterDefinition parameters = 3;
}
```

### Execution

```protobuf
message ExecuteToolRequest {
  string request_id = 1;
  string tool_name = 2;
  string input_json = 3;
}

message ExecuteToolResponse {
  string request_id = 1;
  oneof result {
    string output = 2;
    string error = 3;
  }
}
```

## Deployment

### Systemd

```ini
# /etc/systemd/system/coven-productivity-pack.service
[Unit]
Description=Coven Productivity Pack
After=coven-gateway.service

[Service]
Type=simple
Environment=GATEWAY_ADDR=localhost:50051
ExecStart=/usr/local/bin/productivity-pack
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

### Docker

```dockerfile
FROM rust:1.75-slim as builder
WORKDIR /app
COPY . .
RUN cargo build --release -p productivity-pack

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/productivity-pack /usr/local/bin/
CMD ["productivity-pack"]
```

## See Also

- [Architecture](architecture.md) - System overview
- [coven-agent](agent.md) - How agents use tools
- [MCP Specification](https://modelcontextprotocol.io/) - MCP protocol docs
