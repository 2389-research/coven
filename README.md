# coven

A Rust-based agent platform for orchestrating AI agents with tool capabilities.

## Overview

coven provides infrastructure for running and coordinating AI agents that can use tools (called "packs") to accomplish tasks. The platform supports:

- **Agent Runtime**: Execute agents with LLM backends and tool access
- **Swarm Orchestration**: Coordinate multiple agents working together
- **Pack System**: Modular tool bundles that agents can use
- **Terminal UI**: Interactive interface for managing agents
- **CLI**: Command-line interface for all operations

## Crates

| Crate | Description |
|-------|-------------|
| `coven-proto` | Protocol buffer definitions for gRPC communication |
| `coven-ssh` | SSH key management and authentication |
| `coven-grpc` | gRPC client for coven services |
| `coven-client` | High-level client library |
| `coven-pack` | Pack trait definitions and utilities |
| `coven-core` | Core agent runtime and execution |
| `coven-agent` | Agent binary |
| `coven-swarm` | Swarm orchestration |
| `coven-swarm-backend` | Storage backends for swarm state |
| `coven-swarm-core` | Core swarm types and traits |
| `coven-tui` | Terminal user interface |
| `coven-cli` | Unified command-line interface |

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
