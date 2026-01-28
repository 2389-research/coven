# Implementation Plan: coven-slack-rs (Team 3)

Date: 2026-01-27

## Overview

This plan implements a Slack bridge for coven-gateway using Socket Mode. The architecture
mirrors coven-matrix-rs but adapts to Slack's event-driven Socket Mode API and threading model.

## Key Design Decisions

1. **Event-Driven Architecture**: Use slack-morphism's Socket Mode support for real-time events
2. **Channel Bindings**: In-memory HashMap with channel_id -> agent_id mapping
3. **Thread-First Responses**: All responses go in threads to keep channels clean
4. **Response Mode**: Configurable "mention" vs "all" for channel message handling
5. **Separate App/Bot Tokens**: App token for Socket Mode connection, Bot token for posting

## Implementation Tasks

### Task 1: Create Crate Structure

Create `crates/coven-slack-rs/` with:
- Cargo.toml (workspace member)
- src/ directory structure

```
coven-slack-rs/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── error.rs
│   ├── config.rs
│   ├── gateway.rs
│   ├── slack.rs
│   ├── bridge.rs
│   └── commands.rs
├── tests/
│   └── integration.rs
├── config.example.toml
└── README.md
```

### Task 2: Implement error.rs

Define BridgeError enum with variants:
- Config(String)
- Slack(slack_morphism error)
- Gateway(tonic::Status)
- Connection(tonic::transport::Error)
- Io(std::io::Error)

### Task 3: Implement config.rs

Config structure:
```rust
pub struct Config {
    pub slack: SlackConfig,
    pub gateway: GatewayConfig,
    pub bridge: BridgeConfig,
}

pub struct SlackConfig {
    pub app_token: String,    // xapp-... for Socket Mode
    pub bot_token: String,    // xoxb-... for API calls
}

pub struct BridgeConfig {
    pub allowed_channels: Vec<String>,
    pub response_mode: ResponseMode, // "mention" or "all"
    pub typing_indicator: bool,
    pub thread_replies: bool,
}
```

### Task 4: Implement gateway.rs

Copy gateway client pattern from coven-matrix-rs:
- AuthInterceptor for Bearer token
- GatewayClient with connect/list_agents/send_message/stream_events

### Task 5: Implement slack.rs

Slack client wrapper:
- SlackClient struct wrapping slack-morphism
- Socket Mode connection handling
- Message posting with thread support
- Typing indicator support (if available in Slack API)

### Task 6: Implement commands.rs

Command enum and parsing:
- `/coven bind <agent-id>`
- `/coven unbind`
- `/coven status`
- `/coven agents`
- `/coven help`

Execute commands with context (gateway, bindings, channel_id)

### Task 7: Implement bridge.rs

Core bridge logic:
- ChannelBinding struct (channel_id -> conversation_key)
- Bridge struct with slack client, gateway client, bindings
- Event handler for message events
- Response logic: thread detection, DM detection, @mention detection
- Message streaming and response accumulation

### Task 8: Implement main.rs and lib.rs

Entry point with:
- CLI argument parsing (config path)
- Tracing setup
- Signal handling (ctrl+c, SIGTERM)
- Bridge initialization and run loop

### Task 9: Write Tests

Integration tests:
- Command parsing
- Config loading and validation
- Channel allowed checks
- Response mode logic

### Task 10: Create Example Config and README

config.example.toml with documented options
README.md with setup instructions

## Response Logic Matrix

| Context | response_mode="mention" | response_mode="all" |
|---------|------------------------|---------------------|
| Thread reply | YES | YES |
| DM | YES | YES |
| @mention in channel | YES | YES |
| Channel message | NO | YES |

## Dependencies

From workspace:
- coven-proto
- coven-grpc
- tokio, futures
- tonic, prost
- serde, toml
- tracing, tracing-subscriber
- anyhow, thiserror
- clap
- uuid
- dirs, shellexpand

New:
- slack-morphism = "2" (with socket-mode feature)

## Verification

1. `cargo build -p coven-slack-rs`
2. `cargo test -p coven-slack-rs`
3. Manual test with real Slack workspace (if tokens available)
