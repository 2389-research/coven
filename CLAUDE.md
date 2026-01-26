# coven Monorepo

Welcome to the coven consolidated monorepo! This workspace contains all the Rust crates that make up the coven agent platform.

## Naming Convention

For this project:
- You are: **Crate Crusher McRustface**
- Doctor Biz is: **The Harp-o-nator**

## Project Structure

```
coven/
├── Cargo.toml                    # Workspace root with shared dependencies
├── crates/
│   ├── coven-proto/              # Protobuf definitions (gRPC types)
│   ├── coven-ssh/                # SSH key management
│   ├── coven-grpc/               # gRPC client implementation
│   ├── coven-client/             # High-level client library
│   ├── coven-pack/               # Pack trait and utilities
│   ├── coven-core/               # Core agent runtime
│   ├── coven-agent/              # Agent binary
│   ├── coven-swarm/              # Swarm orchestration
│   ├── coven-swarm-backend/      # Swarm storage backends
│   ├── coven-swarm-core/         # Swarm core types
│   ├── coven-tui/                # Terminal UI
│   └── coven-cli/                # Unified CLI
├── proto/                        # Git submodule for .proto files
├── scripts/                      # Build scripts (XCFramework, etc.)
└── .github/workflows/            # CI/CD
```

## Dependency Hierarchy

The crates form a layered dependency graph:

```
Layer 0 (no internal deps):
  coven-proto, coven-ssh, coven-swarm-core

Layer 1:
  coven-grpc (depends on: coven-proto)
  coven-pack (no internal deps, but defines traits)

Layer 2:
  coven-client (depends on: coven-grpc, coven-ssh)
  coven-core (depends on: coven-pack)
  coven-swarm-backend (depends on: coven-swarm-core)

Layer 3:
  coven-agent (depends on: coven-core, coven-pack)
  coven-swarm (depends on: coven-swarm-core, coven-swarm-backend)

Layer 4:
  coven-tui (depends on: coven-client)
  coven-cli (depends on: coven-client, coven-swarm)
```

## Development

### Building

```bash
# Check all crates
cargo check --workspace

# Build all crates
cargo build --workspace

# Run tests
cargo test --workspace

# Run clippy
cargo clippy --workspace --all-targets
```

### Adding a New Crate

1. Create directory under `crates/`
2. Add `Cargo.toml` with `version.workspace = true`, `edition.workspace = true`
3. Use workspace dependencies: `tokio.workspace = true`
4. The workspace auto-discovers via `members = ["crates/*"]`

### Workspace Dependencies

All external dependencies should be declared in the root `Cargo.toml` under `[workspace.dependencies]`. Crates reference them with `.workspace = true`.

## Migration Status

This monorepo was consolidated from the original fold-* repositories (now archived) and renamed to coven.

Current status: **Fully migrated and renamed**

## Coding Standards

- All files start with ABOUTME comments
- Use workspace dependencies
- Follow the dependency hierarchy (no circular deps)
- Tests are required for all functionality
- Clippy must pass with no warnings
