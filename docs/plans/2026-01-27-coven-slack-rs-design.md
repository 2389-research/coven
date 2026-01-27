# Coven Slack Bridge (Rust) Design

Date: 2026-01-27

## Overview

**coven-slack-rs** - A Rust Slack bridge using Socket Mode (WebSocket) to connect Slack workspaces to coven agents via gRPC ClientService.

## Architecture

```
┌──────────────────┐     WebSocket      ┌─────────────────┐
│   Slack API      │◄──────────────────►│ coven-slack-rs  │
│  (Socket Mode)   │                    │                 │
└──────────────────┘                    │  ┌───────────┐  │
                                        │  │ Commands  │  │
                                        │  │ /coven    │  │
                                        │  └───────────┘  │
                                        │        │        │
                                        │        ▼        │
                                        │  ┌───────────┐  │     gRPC
                                        │  │  Bridge   │──┼────────────►  coven-gateway
                                        │  └───────────┘  │  ClientService
                                        │        │        │
                                        │        ▼        │
                                        │  ┌───────────┐  │
                                        │  │ Bindings  │  │
                                        │  │ (in-mem)  │  │
                                        │  └───────────┘  │
                                        └─────────────────┘
```

**Key decisions:**
- **Socket Mode** - WebSocket connection, no public endpoint needed, real-time events
- **Rust crate**: `slack-morphism` - most mature Rust Slack library with Socket Mode support
- **Same gRPC contract** as Matrix bridge - uses `ClientService` for send/stream
- **Same command pattern** - `/coven bind`, `/coven unbind`, `/coven status`, `/coven agents`

## Module Structure

```
coven-slack-rs/
├── Cargo.toml
├── src/
│   ├── main.rs          # Entry point, signal handling
│   ├── lib.rs           # Module exports
│   ├── config.rs        # TOML config, env var expansion
│   ├── error.rs         # BridgeError enum
│   ├── gateway.rs       # gRPC ClientService client
│   ├── slack.rs         # Socket Mode client wrapper
│   ├── bridge.rs        # Message routing, bindings
│   └── commands.rs      # /coven slash command handling
├── tests/
│   └── integration.rs
├── config.example.toml
└── README.md
```

## Configuration

```toml
# slack-bridge.toml

[slack]
# App-level token (xapp-...) for Socket Mode
app_token = "${SLACK_APP_TOKEN}"
# Bot token (xoxb-...) for posting messages
bot_token = "${SLACK_BOT_TOKEN}"

[gateway]
url = "http://localhost:6666"
token = "${COVEN_TOKEN}"

[bridge]
# Channels the bot listens to (empty = all channels it's in)
allowed_channels = []

# Response trigger mode:
#   "mention" - only respond when @mentioned or in DMs
#   "all"     - respond to every message in allowed channels
response_mode = "mention"

# Typing indicator while agent responds
typing_indicator = true

# Post responses in threads (keeps channels cleaner)
thread_replies = true
```

## Response Logic

The bot responds based on context:

| Context | Responds? |
|---------|-----------|
| Thread reply | Always |
| DM | Always |
| @mention in channel | Always |
| Channel message (response_mode="all") | Yes |
| Channel message (response_mode="mention") | No |

Threads always get responses to maintain conversation flow without requiring @mentions.

## Message Flow

1. User sends message in Slack channel (mentions bot, DM, or thread)
2. Slack pushes event via Socket Mode WebSocket
3. Bridge checks: allowed channel? has binding? is `/coven` command?
4. If command → execute, respond in thread
5. If message → `gateway.send_message()` with `frontend: "slack"`, `channel_id: <channel>`
6. Stream response events, accumulate text
7. Post response to Slack (in thread if threaded message or `thread_replies=true`)

## Slack-Specific Features

- **App mentions** - respond when `@bot` mentioned
- **Threads** - preserve thread context, always reply in same thread
- **Blocks** - use Slack Block Kit for rich formatting (code blocks, markdown)
- **Typing indicator** - show typing while agent processes (if enabled)

## Error Types

```rust
pub enum BridgeError {
    Config(String),
    Slack(slack_morphism::errors::SlackClientError),
    Gateway(tonic::Status),
    Connection(tonic::transport::Error),
    Io(std::io::Error),
}
```

## Reconnection Behavior

- Socket Mode auto-reconnects (slack-morphism handles this)
- Gateway connection: retry with exponential backoff on disconnect

## Commands

| Command | Description |
|---------|-------------|
| `/coven bind <agent-id>` | Bind channel to agent |
| `/coven unbind` | Unbind channel from agent |
| `/coven status` | Show current binding |
| `/coven agents` | List available agents |
| `/coven help` | Show help message |

## Scenario Test Cases

1. `/coven agents` lists online agents
2. `/coven bind agent-123` binds channel
3. `/coven status` shows binding
4. Send message → response streamed back to Slack
5. `/coven unbind` removes binding
6. Message in unbound channel → no response (or warning)
7. `response_mode = "all"` responds without @mention
8. `response_mode = "mention"` ignores non-mentions
9. Thread reply always gets response regardless of mode
10. DM always gets response

## Dependencies

```toml
[dependencies]
# Internal crates
coven-proto.workspace = true
coven-grpc.workspace = true

# Async runtime
tokio = { workspace = true, features = ["full", "signal"] }
futures.workspace = true

# gRPC
tonic.workspace = true
prost.workspace = true

# Slack
slack-morphism = { version = "2", features = ["hyper", "socket-mode"] }

# Config and serialization
serde = { workspace = true, features = ["derive"] }
toml.workspace = true
dirs.workspace = true
shellexpand = "3"

# Logging and errors
tracing.workspace = true
tracing-subscriber = { workspace = true, features = ["env-filter"] }
anyhow.workspace = true
thiserror.workspace = true

# CLI
clap = { workspace = true, features = ["derive", "env"] }

# Utilities
uuid = { workspace = true, features = ["v4"] }
```

## Implementation Notes

- Mirror coven-matrix-rs structure for consistency
- Reuse gateway client code pattern exactly
- Commands module nearly identical (different response formatting)
- Slack-specific: Block Kit formatting, thread handling, Socket Mode events
