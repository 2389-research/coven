# fold

A Rust-based agent platform for orchestrating AI agents with tool capabilities.

## Overview

fold provides infrastructure for running and coordinating AI agents that can use tools (called "packs") to accomplish tasks. The platform supports:

- **Agent Runtime**: Execute agents with LLM backends and tool access
- **Swarm Orchestration**: Coordinate multiple agents working together
- **Pack System**: Modular tool bundles that agents can use
- **Terminal UI**: Interactive interface for managing agents
- **CLI**: Command-line interface for all operations

## Crates

| Crate | Description |
|-------|-------------|
| `fold-proto` | Protocol buffer definitions for gRPC communication |
| `fold-ssh` | SSH key management and authentication |
| `fold-grpc` | gRPC client for fold services |
| `fold-client` | High-level client library |
| `fold-pack` | Pack trait definitions and utilities |
| `fold-core` | Core agent runtime and execution |
| `fold-agent` | Agent binary |
| `fold-swarm` | Swarm orchestration |
| `fold-swarm-backend` | Storage backends for swarm state |
| `fold-swarm-core` | Core swarm types and traits |
| `fold-tui` | Terminal user interface |
| `fold-cli` | Unified command-line interface |

## Building

```bash
# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Build release
cargo build --workspace --release
```

## License

MIT
