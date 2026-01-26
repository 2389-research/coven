# coven-agent

The agent binary that connects to coven-gateway and processes messages using Claude.

## Overview

`coven-agent` is a single-workspace agent that:

- Connects to coven-gateway via gRPC
- Receives messages from frontends
- Processes them using a configurable LLM backend
- Streams responses back in real-time
- Maintains conversation history

For multi-workspace scenarios, see [coven-swarm](swarm.md).

## Installation

```bash
# Build from source
make coven-agent

# Or install globally
cargo install --path crates/coven-agent
```

## Usage

### Quick Start

```bash
# Run with defaults
coven-agent run --name my-agent --working-dir ~/projects/myproject

# With explicit gateway
coven-agent run \
  --name my-agent \
  --working-dir ~/projects/myproject \
  --gateway localhost:50051

# Use CLI backend instead of direct API
coven-agent run \
  --name my-agent \
  --working-dir ~/projects/myproject \
  --backend cli
```

### Interactive Setup

```bash
# Create agent config interactively
coven-agent new
```

This launches a TUI wizard that guides you through configuration.

## Command Reference

### `coven-agent run`

Run the agent and connect to gateway.

```
USAGE:
    coven-agent run [OPTIONS]

OPTIONS:
    --name <NAME>           Agent identifier (required)
    --working-dir <PATH>    Working directory for the agent (required)
    --gateway <ADDR>        Gateway address [default: localhost:50051]
    --backend <TYPE>        Backend type: mux, cli [default: mux]
    --display <MODE>        Output mode: quiet, normal, verbose [default: normal]
    --config <PATH>         Path to config file
```

### `coven-agent new`

Interactive configuration wizard.

```
USAGE:
    coven-agent new [OPTIONS]

OPTIONS:
    --name <NAME>    Pre-fill agent name
    --output <PATH>  Config output path [default: ~/.config/coven/agents/<name>.toml]
```

## Configuration

### Config File

Agent configs are stored in `~/.config/coven/agents/<name>.toml`:

```toml
# Agent identity
name = "my-agent"
working_directory = "/home/user/projects/myproject"

# Gateway connection
gateway_url = "localhost:50051"

# Backend configuration
backend = "mux"  # or "cli"

# Model settings (mux backend)
[model]
name = "claude-sonnet-4-20250514"
max_tokens = 8192

# Display settings
[display]
mode = "normal"  # quiet, normal, verbose
```

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ANTHROPIC_API_KEY` | API key for mux backend | Required for mux |
| `ANTHROPIC_MODEL` | Model name | `claude-sonnet-4-20250514` |
| `ANTHROPIC_MAX_TOKENS` | Max response tokens | `8192` |
| `COVEN_GATEWAY` | Gateway address | `localhost:50051` |
| `COVEN_BACKEND` | Backend type | `mux` |
| `RUST_LOG` | Log level | `info` |

## Backends

### Mux Backend (Recommended)

Direct Anthropic API integration using the mux-rs library.

**Advantages:**
- Native async streaming
- Full tool support
- Better error handling
- Lower latency

**Requirements:**
- `ANTHROPIC_API_KEY` environment variable

```bash
coven-agent run --backend mux ...
```

### CLI Backend

Spawns the `claude` CLI as a subprocess.

**Advantages:**
- Uses existing Claude CLI installation
- No API key needed (uses CLI auth)

**Requirements:**
- `claude` CLI installed and authenticated

```bash
coven-agent run --backend cli ...
```

## Architecture

### Crate Structure

```
coven-agent/
├── src/
│   ├── main.rs       # CLI entry point
│   ├── lib.rs        # Library exports
│   ├── client.rs     # gRPC client and message handling
│   ├── metadata.rs   # Agent metadata (git info, OS, etc.)
│   ├── run.rs        # Run command implementation
│   ├── wizard.rs     # Interactive setup TUI
│   └── tui.rs        # Status display
└── Cargo.toml
```

### Dependencies

```
coven-agent
├── coven-core      # Runtime, backends, storage
├── coven-pack      # Tool execution
├── coven-grpc      # Gateway communication
└── coven-proto     # Protocol definitions
```

### Message Flow

```
Gateway                    Agent
   │                         │
   │◄─── RegisterAgent ──────│  1. Agent connects, registers
   │                         │
   │──── Welcome ───────────►│  2. Gateway confirms
   │                         │
   │──── SendMessage ───────►│  3. User message arrives
   │                         │
   │                         │  4. Agent processes with backend
   │                         │     Backend.send() → Stream<Event>
   │                         │
   │◄─── MessageResponse ────│  5. Streaming responses
   │◄─── MessageResponse ────│     (Thinking, Text, Tool*, Done)
   │◄─── MessageResponse ────│
   │                         │
   │◄─── Heartbeat ──────────│  6. Periodic heartbeats
   │                         │
```

## Metadata

Agents report metadata on registration:

```rust
pub struct AgentMetadata {
    pub working_directory: String,
    pub hostname: String,
    pub os: String,
    pub git: Option<GitInfo>,      // branch, commit, dirty
    pub workspaces: Vec<String>,   // workspace identifiers
    pub backend: String,           // mux, cli
}
```

This helps the gateway and frontends identify agents and their state.

## Logging

Configure logging with `RUST_LOG`:

```bash
# Info level (default)
RUST_LOG=info coven-agent run ...

# Debug agent internals
RUST_LOG=coven_agent=debug coven-agent run ...

# Debug backend communication
RUST_LOG=coven_core::backend=debug coven-agent run ...

# Full trace
RUST_LOG=trace coven-agent run ...
```

## Troubleshooting

### Connection Failed

```
Error: failed to connect to gateway
```

- Check gateway is running: `curl http://localhost:8080/health`
- Verify gateway address matches config
- Check network/firewall rules

### Authentication Failed

```
Error: authentication failed
```

- Verify SSH key exists: `ls ~/.ssh/id_ed25519`
- Check key is registered with gateway
- Ensure `ANTHROPIC_API_KEY` is set (for mux backend)

### Backend Errors

```
Error: backend send failed
```

- For mux: verify `ANTHROPIC_API_KEY` is valid
- For cli: verify `claude` CLI is installed and authenticated

## See Also

- [coven-swarm](swarm.md) - Multi-workspace orchestration
- [coven-core](core.md) - Runtime internals
- [Architecture](architecture.md) - System overview
