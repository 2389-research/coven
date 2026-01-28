# Coven Telegram Bridge (Rust) Design

Date: 2026-01-27

## Overview

**coven-telegram-rs** - A Rust Telegram bridge using Long Polling to connect Telegram chats to coven agents via gRPC ClientService.

## Architecture

```
┌──────────────────┐     Long Polling     ┌──────────────────┐
│   Telegram API   │◄────────────────────►│ coven-telegram-rs│
│   (Bot API)      │                      │                  │
└──────────────────┘                      │  ┌───────────┐   │
                                          │  │ Commands  │   │
                                          │  │ /coven    │   │
                                          │  └───────────┘   │
                                          │        │         │
                                          │        ▼         │
                                          │  ┌───────────┐   │     gRPC
                                          │  │  Bridge   │───┼────────────►  coven-gateway
                                          │  └───────────┘   │  ClientService
                                          │        │         │
                                          │        ▼         │
                                          │  ┌───────────┐   │
                                          │  │ Bindings  │   │
                                          │  │ (in-mem)  │   │
                                          │  └───────────┘   │
                                          └──────────────────┘
```

**Key decisions:**
- **Long Polling** - No public endpoint needed, works behind NAT/firewalls
- **Rust crate**: `teloxide` - most mature async Telegram library
- **Same gRPC contract** as Slack/Matrix bridges - uses `ClientService`
- **Same command pattern** - `/coven bind`, `/coven unbind`, `/coven status`, `/coven agents`

## Module Structure

```
coven-telegram-rs/
├── Cargo.toml
├── src/
│   ├── main.rs          # Entry point, dispatcher setup
│   ├── lib.rs           # Module exports
│   ├── config.rs        # TOML config, env var expansion
│   ├── error.rs         # BridgeError enum
│   ├── gateway.rs       # gRPC ClientService client
│   ├── telegram.rs      # Telegram bot wrapper
│   ├── bridge.rs        # Message routing, bindings
│   ├── context.rs       # TelegramContext enum (Group/Private/Thread)
│   └── commands.rs      # /coven command handling
├── tests/
│   └── integration.rs
├── config.example.toml
└── README.md
```

## Cargo.toml

```toml
[package]
name = "coven-telegram-rs"
version = "0.1.0"
edition = "2021"
description = "Telegram bridge for coven-gateway using Long Polling"
license = "MIT"

[[bin]]
name = "coven-telegram-bridge"
path = "src/main.rs"

[dependencies]
# Telegram
teloxide = { version = "0.13", features = ["macros"] }

# Async runtime
tokio = { version = "1", features = ["full"] }
futures = "0.3"

# gRPC
tonic = "0.12"
coven-proto = { path = "../coven-proto" }

# Config
serde = { version = "1", features = ["derive"] }
toml = "0.8"
shellexpand = "3"
dirs = "5"

# Utilities
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
thiserror = "2"
uuid = { version = "1", features = ["v4"] }
clap = { version = "4", features = ["derive"] }

[dev-dependencies]
tempfile = "3"
```

## Configuration

```toml
# telegram-bridge.toml

[telegram]
# Bot token from @BotFather
bot_token = "${TELEGRAM_BOT_TOKEN}"

[gateway]
url = "http://localhost:6666"
token = "${COVEN_TOKEN}"

[bridge]
# Chat IDs the bot listens to (empty = all chats it's in)
allowed_chats = []

# Response trigger mode:
#   "mention" - only respond when @mentioned or in private chats
#   "all"     - respond to every message in allowed chats
response_mode = "mention"

# Reply in threads (Telegram "reply to" feature)
thread_replies = true
```

## TelegramContext

Similar to SlackContext, handles the three message contexts:

```rust
pub enum TelegramContext {
    /// Private 1:1 chat with bot
    Private { chat_id: i64 },
    /// Group/supergroup message (not a reply)
    Group { chat_id: i64 },
    /// Reply thread in group
    Thread { chat_id: i64, thread_id: i32 },
}

impl TelegramContext {
    pub fn should_respond(&self, response_mode: ResponseMode, is_mention: bool) -> bool {
        match self {
            TelegramContext::Private { .. } => true,  // Always respond in private
            TelegramContext::Thread { .. } => true,   // Always respond in threads
            TelegramContext::Group { .. } => {
                is_mention || response_mode == ResponseMode::All
            }
        }
    }
}
```

## Commands

Same pattern as Slack:

| Command | Description |
|---------|-------------|
| `/coven bind <agent-id>` | Bind chat to an agent |
| `/coven unbind` | Unbind chat |
| `/coven status` | Show current binding |
| `/coven agents` | List available agents |
| `/coven help` | Show help |

Telegram native commands (registered with BotFather):
- `/bind` → alias for `/coven bind`
- `/unbind` → alias for `/coven unbind`
- `/status` → alias for `/coven status`

## Message Flow

1. `teloxide` dispatcher receives update via long polling
2. Check if message is a command → handle via `commands.rs`
3. Check context (private/group/thread) and response_mode
4. Look up binding for chat_id
5. Strip @bot_mention from text
6. Send to gateway via `ClientService.SendMessage`
7. Stream response events, accumulate text
8. Reply to original message (creates thread if configured)

## Error Types

```rust
pub enum BridgeError {
    Config(String),
    Telegram(String),
    TeloxideRequest(#[from] teloxide::RequestError),
    Gateway(#[from] tonic::Status),
    Connection(#[from] tonic::transport::Error),
    Io(#[from] std::io::Error),
}
```

## Testing Strategy

- Unit tests for config parsing, context logic, command parsing
- Integration tests for message flow (mocked Telegram/Gateway)
- Scenario tests per test-kitchen pattern

## Known Limitations (MVP)

- Bindings are in-memory only (lost on restart)
- No gateway reconnection logic
- No typing indicator (Telegram supports "typing..." action but not implemented)
