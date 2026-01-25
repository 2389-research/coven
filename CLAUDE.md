# fold Monorepo

Welcome to the fold consolidated monorepo! This workspace contains all the Rust crates that make up the fold agent platform.

## Naming Convention

For this project:
- You are: **Crate Crusher McRustface**
- Doctor Biz is: **The Harp-o-nator**

## Project Structure

```
fold/
├── Cargo.toml                    # Workspace root with shared dependencies
├── crates/
│   ├── fold-proto/               # Protobuf definitions (gRPC types)
│   ├── fold-ssh/                 # SSH key management
│   ├── fold-grpc/                # gRPC client implementation
│   ├── fold-client/              # High-level client library
│   ├── fold-pack/                # Pack trait and utilities
│   ├── fold-core/                # Core agent runtime
│   ├── fold-agent/               # Agent binary
│   ├── fold-swarm/               # Swarm orchestration
│   ├── fold-swarm-backend/       # Swarm storage backends
│   ├── fold-swarm-core/          # Swarm core types
│   ├── fold-tui/                 # Terminal UI
│   └── fold-cli/                 # Unified CLI
├── proto/                        # Git submodule for .proto files
├── scripts/                      # Build scripts (XCFramework, etc.)
└── .github/workflows/            # CI/CD
```

## Dependency Hierarchy

The crates form a layered dependency graph:

```
Layer 0 (no internal deps):
  fold-proto, fold-ssh, fold-swarm-core

Layer 1:
  fold-grpc (depends on: fold-proto)
  fold-pack (no internal deps, but defines traits)

Layer 2:
  fold-client (depends on: fold-grpc, fold-ssh)
  fold-core (depends on: fold-pack)
  fold-swarm-backend (depends on: fold-swarm-core)

Layer 3:
  fold-agent (depends on: fold-core, fold-pack)
  fold-swarm (depends on: fold-swarm-core, fold-swarm-backend)

Layer 4:
  fold-tui (depends on: fold-client)
  fold-cli (depends on: fold-client, fold-swarm)
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

This monorepo is being consolidated from:
- fold-common (crates: proto, ssh, grpc-client, client, pack)
- fold-agent (crates: agent, core)
- fold-swarm (crates: swarm, swarm-backend, swarm-core)
- fold-packs (various pack implementations)
- fold-tui

Current status: **Skeleton created, awaiting code migration**

## Coding Standards

- All files start with ABOUTME comments
- Use workspace dependencies
- Follow the dependency hierarchy (no circular deps)
- Tests are required for all functionality
- Clippy must pass with no warnings
