# Coven Slack Bridge Implementation Plan - Team 1

## Overview

Implementation of `coven-slack-rs`, a Rust Slack bridge using Socket Mode for real-time Slack events and gRPC ClientService for coven-gateway communication.

## Architectural Decisions

### 1. Event Processing Model
- Use `slack-morphism`'s async event handlers with tokio channels
- Single event loop pattern with message passing for clean separation
- Bindings stored in `DashMap` for lock-free concurrent access

### 2. Response Mode Handling
- Enum-based response mode with clear matching logic
- Context struct (`MessageContext`) to encapsulate all decision factors
- Thread detection via `thread_ts` field presence

### 3. Error Handling Strategy
- Comprehensive `BridgeError` enum with `thiserror` derive
- Graceful degradation: log errors, notify user in channel when possible
- No panics in event handlers

### 4. Module Responsibilities

| Module | Responsibility |
|--------|---------------|
| `config.rs` | TOML parsing, env expansion, validation |
| `error.rs` | Error types and conversions |
| `slack.rs` | Socket Mode connection, event handling |
| `gateway.rs` | gRPC ClientService client wrapper |
| `commands.rs` | /coven command parsing and execution |
| `bridge.rs` | Message routing, bindings, orchestration |
| `main.rs` | Entry point, signal handling |

## Implementation Tasks

### Task 1: Crate Setup
**Files:** `Cargo.toml`, `src/lib.rs`, `src/main.rs`

- Create workspace member at `crates/coven-slack-rs/`
- Define dependencies from design doc
- Set up module structure
- Create main.rs with CLI and signal handling

**Acceptance:** `cargo build` succeeds

### Task 2: Error Types
**File:** `src/error.rs`

```rust
pub enum BridgeError {
    Config(String),
    Slack(slack_morphism::errors::SlackClientError),
    Gateway(tonic::Status),
    Connection(tonic::transport::Error),
    Io(std::io::Error),
}
```

**Acceptance:** Compiles with proper `From` implementations

### Task 3: Configuration
**File:** `src/config.rs`

- SlackConfig: app_token, bot_token
- GatewayConfig: url, token
- BridgeConfig: allowed_channels, response_mode, typing_indicator, thread_replies
- Environment variable expansion with shellexpand
- Validation for required fields

**Acceptance:** Config loads from TOML with env var expansion

### Task 4: Gateway Client
**File:** `src/gateway.rs`

- Reuse pattern from coven-matrix-rs
- AuthInterceptor for Bearer token
- Methods: connect, list_agents, send_message, stream_events

**Acceptance:** Can connect to gateway and list agents

### Task 5: Slack Client
**File:** `src/slack.rs`

- SlackClient wrapper around slack-morphism
- Socket Mode connection
- Methods for posting messages (with thread support)
- Typing indicator support
- Block Kit formatting helpers

**Acceptance:** Can connect to Slack and post messages

### Task 6: Commands
**File:** `src/commands.rs`

- Command enum: Bind, Unbind, Status, Agents, Help
- Parse from message text
- Execute with context
- Format responses for Slack

**Acceptance:** All commands parse and execute correctly

### Task 7: Bridge Logic
**File:** `src/bridge.rs`

- Bridge struct orchestrating all components
- In-memory bindings (channel_id -> agent_id)
- Message routing logic:
  - Check if command
  - Check response mode / context
  - Forward to gateway
  - Stream response back to Slack

**Acceptance:** Full message flow works

### Task 8: Example Config
**File:** `config.example.toml`

- All configuration options documented
- Environment variable placeholders

### Task 9: Integration Tests
**File:** `tests/integration.rs`

- Command parsing tests
- Config loading tests
- Response mode logic tests

**Acceptance:** `cargo test` passes

## Response Mode Decision Tree

```
Is message a /coven command?
├── Yes → Execute command, respond
└── No → Check context
         ├── Is in thread? → Always respond
         ├── Is DM? → Always respond
         ├── Is @mention? → Always respond
         └── Channel message:
             ├── response_mode = "all" → Respond
             └── response_mode = "mention" → Ignore
```

## Dependencies (from design doc)

```toml
[dependencies]
coven-proto.workspace = true
coven-grpc.workspace = true
tokio = { workspace = true, features = ["full", "signal"] }
futures.workspace = true
tonic.workspace = true
prost.workspace = true
slack-morphism = { version = "2", features = ["hyper", "socket-mode"] }
serde = { workspace = true, features = ["derive"] }
toml.workspace = true
dirs.workspace = true
shellexpand = "3"
tracing.workspace = true
tracing-subscriber = { workspace = true, features = ["env-filter"] }
anyhow.workspace = true
thiserror.workspace = true
clap = { workspace = true, features = ["derive", "env"] }
uuid = { workspace = true, features = ["v4"] }
```

## Testing Strategy

1. **Unit tests** for command parsing, config validation
2. **Integration tests** with mock/stub components where needed
3. No mock mode - test against real services when available

## Success Criteria

- [ ] `cargo build` succeeds
- [ ] `cargo test` passes
- [ ] All /coven commands work
- [ ] Messages route correctly based on response_mode
- [ ] Thread replies preserved
- [ ] Typing indicator works
- [ ] Graceful shutdown on SIGTERM/SIGINT
