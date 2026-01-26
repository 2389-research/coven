# coven

A Rust-based platform for orchestrating AI agents with tool capabilities.

## What is coven?

coven is an agent orchestration platform that connects AI agents (powered by Claude) to a central gateway, enabling:

- **Multi-agent coordination** - Run multiple specialized agents across workspaces
- **Tool packs** - Extend agent capabilities with modular tool bundles
- **Real-time streaming** - Bidirectional gRPC for instant response streaming
- **Multiple frontends** - Terminal UI, HTTP API, Matrix chat integration

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              Frontends                                   │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐    │
│  │  coven-tui  │  │  HTTP API   │  │   Matrix    │  │  coven-cli  │    │
│  │  (Rust TUI) │  │  (clients)  │  │   Bridge    │  │  (unified)  │    │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘    │
└─────────┼────────────────┼────────────────┼────────────────┼────────────┘
          │                │                │                │
          ▼                ▼                ▼                ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         coven-gateway (Go)                               │
│  ┌──────────────────┐  ┌──────────────────┐  ┌──────────────────┐      │
│  │   HTTP Server    │  │   gRPC Server    │  │   Pack Service   │      │
│  │   (SSE events)   │  │  (agent streams) │  │  (tool registry) │      │
│  └────────┬─────────┘  └────────┬─────────┘  └────────┬─────────┘      │
│           │                     │                     │                 │
│           └─────────────────────┼─────────────────────┘                 │
│                                 ▼                                       │
│                         ┌──────────────┐                                │
│                         │   SQLite     │                                │
│                         │  (threads,   │                                │
│                         │  messages)   │                                │
│                         └──────────────┘                                │
└─────────────────────────────────────────────────────────────────────────┘
          ▲                       ▲                       ▲
          │                       │                       │
┌─────────┼───────────────────────┼───────────────────────┼───────────────┐
│         │         Agents        │                       │               │
│  ┌──────┴──────┐  ┌─────────────┴───────┐  ┌───────────┴─────────┐    │
│  │ coven-agent │  │    coven-swarm      │  │    Tool Packs       │    │
│  │  (single)   │  │ (multi-workspace)   │  │ (mcp, productivity) │    │
│  └─────────────┘  └─────────────────────┘  └─────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────┘
```

## Components

| Component | Language | Description |
|-----------|----------|-------------|
| **coven** | Rust | This monorepo - agents, CLI, TUI, packs |
| **coven-gateway** | Go | Central server - routing, storage, pack registry |
| **coven-proto** | Protobuf | Shared protocol definitions |

## Quick Start

### Prerequisites

- Rust 1.75+
- Go 1.21+ (for gateway)
- protoc (for proto generation)
- An Anthropic API key

### Build

```bash
# Clone and build
git clone https://github.com/2389-research/coven.git
cd coven
make build

# Or build specific components
make coven          # CLI
make coven-agent    # Agent binary
make coven-swarm    # Swarm orchestrator
```

### Run

```bash
# Start the gateway (separate repo)
cd ../coven-gateway
make build && ./bin/coven-gateway serve

# Run a single agent
./target/debug/coven-agent run --name my-agent --working-dir ~/projects/myproject

# Or use the swarm for multiple workspaces
./target/debug/coven-swarm supervisor
```

## Crates

### Core

| Crate | Description |
|-------|-------------|
| [`coven-proto`](docs/proto.md) | Protocol buffer definitions for gRPC |
| [`coven-core`](docs/core.md) | Agent runtime, backends, and storage |
| [`coven-grpc`](docs/grpc.md) | gRPC client utilities |

### Agents

| Crate | Description |
|-------|-------------|
| [`coven-agent`](docs/agent.md) | Single-workspace agent binary |
| [`coven-swarm`](docs/swarm.md) | Multi-workspace supervisor |
| [`coven-swarm-core`](docs/swarm.md) | Shared swarm types |
| [`coven-swarm-backend`](docs/swarm.md) | Pluggable Claude backends |

### Clients & UI

| Crate | Description |
|-------|-------------|
| [`coven-cli`](docs/cli.md) | Unified command-line interface |
| [`coven-tui`](docs/tui.md) | Terminal user interface |
| [`coven-client`](docs/client.md) | High-level gateway client |
| [`coven-ssh`](docs/client.md) | SSH key authentication |

### Packs

| Crate | Description |
|-------|-------------|
| [`coven-pack`](docs/packs.md) | Pack SDK for building tools |
| [`mcp-bridge-pack`](docs/packs.md) | Bridge to MCP servers |
| [`productivity-pack`](docs/packs.md) | Todo and notes tools |
| [`test-pack`](docs/packs.md) | Echo tools for testing |

## Documentation

- [Architecture](docs/architecture.md) - System design and data flow
- [Agent Guide](docs/agent.md) - Running and configuring agents
- [Swarm Guide](docs/swarm.md) - Multi-workspace orchestration
- [Pack Development](docs/packs.md) - Building tool packs
- [CLI Reference](docs/cli.md) - Command-line usage
- [Client Library](docs/client.md) - Using coven-client

## Configuration

Configuration files live in `~/.config/coven/`:

```
~/.config/coven/
├── config.toml           # Main agent config
├── agents/
│   └── <name>.toml       # Per-agent configs
└── swarm/
    └── config.toml       # Swarm config
```

Data is stored in `~/.local/share/coven/`:

```
~/.local/share/coven/
├── threads.db            # SQLite conversation store
└── sessions/             # Session state
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ANTHROPIC_API_KEY` | API key for Claude | Required |
| `ANTHROPIC_MODEL` | Model to use | `claude-sonnet-4-20250514` |
| `COVEN_GATEWAY` | Gateway gRPC address | `localhost:50051` |
| `COVEN_BACKEND` | Backend type (`mux`/`cli`) | `mux` |
| `RUST_LOG` | Log level | `info` |

## Development

```bash
make              # check + test + clippy
make build        # debug build
make release      # release build
make test         # run tests
make clippy       # lint
make fmt          # format code

# Run directly
make run ARGS="..."
make run-agent ARGS="..."
make run-swarm ARGS="..."
```

See [CLAUDE.md](CLAUDE.md) for detailed development guidelines.

## License

MIT
