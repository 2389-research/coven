# Implementation Plan: coven-slack-rs (Team 2)

## Architectural Approach

Team 2's approach emphasizes **type-safe channel abstraction** with a focus on Slack's threading model. Key differentiators:

1. **Channel abstraction layer** - Abstract channel/DM/thread as a unified `SlackContext` type
2. **Response mode as enum** - Type-safe response mode rather than string matching
3. **Event-driven architecture** - Use channels (mpsc) for clean separation between Slack events and gateway communication
4. **Strict thread semantics** - All responses go to threads; create new threads for channel messages

## Module Design

```
coven-slack-rs/
├── Cargo.toml
├── src/
│   ├── main.rs           # CLI + signal handling + orchestration
│   ├── lib.rs            # Module exports
│   ├── error.rs          # BridgeError with slack-morphism errors
│   ├── config.rs         # TOML config with env expansion
│   ├── gateway.rs        # GatewayClient (copied pattern from matrix-rs)
│   ├── slack.rs          # SlackClient wrapping slack-morphism
│   ├── context.rs        # SlackContext (channel/thread/dm abstraction)
│   ├── commands.rs       # /coven command parser and executor
│   └── bridge.rs         # Core routing logic
├── tests/
│   └── integration.rs    # Config and command parsing tests
├── config.example.toml
└── README.md
```

## Tasks

### Task 1: Cargo.toml Setup
- Create crate at `crates/coven-slack-rs/`
- Binary: `coven-slack-bridge`
- Dependencies: slack-morphism, coven-proto, coven-grpc, tokio, tonic, etc.

### Task 2: error.rs
- Define `BridgeError` enum with variants for Slack, Gateway, Config, IO
- Define `Result<T>` type alias

### Task 3: config.rs
- `SlackConfig`: app_token, bot_token
- `GatewayConfig`: url, token
- `BridgeConfig`: allowed_channels, response_mode (enum), typing_indicator, thread_replies
- `ResponseMode` enum: `Mention`, `All`
- Config::load with shellexpand for env vars
- Validation: require tokens, validate URL format

### Task 4: context.rs
- `SlackContext` enum: `Channel`, `Thread`, `DirectMessage`
- Helper methods: `should_respond(&self, response_mode: ResponseMode, is_mention: bool) -> bool`
- Thread ID extraction and construction

### Task 5: gateway.rs
- Copy pattern from coven-matrix-rs
- `GatewayClient` with auth interceptor
- `list_agents`, `send_message`, `stream_events`

### Task 6: slack.rs
- `SlackClient` wrapping slack-morphism
- Socket Mode connection setup
- Message posting (with thread support)
- Typing indicator
- Event type extraction (app_mention, message, etc.)

### Task 7: commands.rs
- `Command` enum: Bind, Unbind, Status, Agents, Help, Unknown
- `Command::parse` for `/coven` slash command text
- `CommandContext` for execution state
- `execute_command` async function

### Task 8: bridge.rs
- `ChannelBinding` struct with channel_id and conversation_key
- `Bridge` struct owning SlackClient, GatewayClient, bindings map
- Event handler registration
- Message routing logic with response mode checking
- Stream response aggregation

### Task 9: main.rs + lib.rs
- CLI with clap (config path, optional flags)
- Signal handling (SIGINT, SIGTERM)
- Tokio runtime setup
- Tracing initialization

### Task 10: Tests
- Command parsing tests
- Config loading tests
- Response mode logic tests
- Channel allowlist tests

### Task 11: Documentation
- config.example.toml with all options documented
- README.md with setup instructions

## Response Logic Matrix

| Context | @mention | response_mode=mention | response_mode=all |
|---------|----------|----------------------|-------------------|
| Thread  | -        | YES                  | YES               |
| DM      | -        | YES                  | YES               |
| Channel | YES      | YES                  | YES               |
| Channel | NO       | NO                   | YES               |

## Key Implementation Decisions

1. **Response threads**: When responding to a channel message (not already in thread), create a new thread
2. **Binding scope**: Bindings are per-channel, not per-thread (threads inherit channel binding)
3. **Socket Mode events**: Handle both `app_mention` and `message` event types
4. **Error recovery**: Log errors but don't crash; reconnection handled by slack-morphism

## Verification

After implementation:
- `cargo build` passes
- `cargo test` passes
- `cargo clippy` passes (no warnings)
- Config example is valid TOML
