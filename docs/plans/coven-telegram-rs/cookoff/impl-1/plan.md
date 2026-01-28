# Coven Telegram Bridge - Team 1 Implementation Plan

Date: 2026-01-27

## Overview

Implementing `coven-telegram-rs` - a Telegram bridge for coven-gateway using Long Polling with the `teloxide` crate.

## Architecture

Following the exact same patterns as `coven-slack-rs`:
- Config loading with env var expansion
- TelegramContext enum for Private/Group/Thread contexts
- Commands module for /coven bind|unbind|status|agents|help
- Gateway client with gRPC ClientService
- Bridge orchestrator connecting all pieces
- Teloxide-based Telegram client

## Implementation Order

### Phase 1: Foundation
1. `Cargo.toml` - dependencies aligned with workspace
2. `error.rs` - BridgeError enum with thiserror
3. `config.rs` - TOML config with env expansion

### Phase 2: Core Logic
4. `context.rs` - TelegramContext enum
5. `commands.rs` - Command parsing and execution
6. `gateway.rs` - gRPC client wrapper

### Phase 3: Telegram Integration
7. `telegram.rs` - Teloxide bot wrapper
8. `bridge.rs` - Message routing and bindings

### Phase 4: Entry Points
9. `lib.rs` - Module exports
10. `main.rs` - Binary entry point with dispatcher

### Phase 5: Testing & Docs
11. `tests/integration.rs` - Comprehensive tests
12. `config.example.toml` - Example configuration
13. `README.md` - Documentation

## Key Decisions

### Long Polling vs Webhook
- Using Long Polling (teloxide default)
- Works behind NAT/firewalls
- No public endpoint needed
- Simpler deployment

### Context Types
```rust
pub enum TelegramContext {
    Private { chat_id: i64 },
    Group { chat_id: i64 },
    Thread { chat_id: i64, thread_id: i32 },
}
```

### Bot Mention Detection
- Telegram doesn't use `<@BOT>` syntax
- Check for `@bot_username` in text
- Also check reply-to-bot in threads

### Commands
- `/coven bind <agent-id>` - Bind chat to agent
- `/coven unbind` - Remove binding
- `/coven status` - Show current binding
- `/coven agents` - List available agents
- `/coven help` - Show help

## Dependencies

```toml
teloxide = { version = "0.13", features = ["macros"] }
tokio = { workspace = true }
futures = { workspace = true }
tonic = { workspace = true }
coven-proto = { workspace = true }
coven-grpc = { workspace = true }
serde = { workspace = true }
toml = { workspace = true }
dirs = { workspace = true }
shellexpand = "3"
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
thiserror = { workspace = true }
uuid = { workspace = true }
clap = { workspace = true }
```

## Test Strategy

1. **Unit tests** in each module for parsing and logic
2. **Integration tests** for config loading and context behavior
3. **Module-level tests** for command parsing

## Success Criteria

- [ ] `cargo check -p coven-telegram-rs` passes
- [ ] `cargo test -p coven-telegram-rs` passes
- [ ] All modules follow slack-rs patterns
- [ ] Comprehensive test coverage
- [ ] Clean, idiomatic Rust code
