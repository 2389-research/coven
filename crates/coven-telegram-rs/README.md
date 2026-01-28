# coven-telegram-rs

Telegram bridge for coven-gateway using Long Polling.

## Overview

`coven-telegram-rs` connects Telegram chats to coven agents via the coven-gateway gRPC API. It uses Telegram's Long Polling mechanism (via the `teloxide` crate) which works behind NAT and firewalls without requiring a public endpoint.

## Features

- Long Polling connection to Telegram Bot API
- gRPC client for coven-gateway ClientService
- Chat binding to agents via `/coven` commands
- Mention detection (`@bot_username`)
- Reply-to-bot detection for conversation threads
- Private chat, group, and thread context handling
- Configurable response modes (mention vs all)

## Installation

```bash
cargo build -p coven-telegram-rs --release
```

## Configuration

Copy `config.example.toml` to `~/.config/coven/telegram-bridge.toml`:

```bash
mkdir -p ~/.config/coven
cp config.example.toml ~/.config/coven/telegram-bridge.toml
```

Edit the config file and set your Telegram bot token and gateway URL.

### Getting a Bot Token

1. Message [@BotFather](https://t.me/botfather) on Telegram
2. Send `/newbot` and follow the prompts
3. Copy the token (format: `123456789:ABC-DEF...`)

### Environment Variables

The config supports environment variable expansion:

```toml
[telegram]
bot_token = "${TELEGRAM_BOT_TOKEN}"

[gateway]
url = "http://localhost:6666"
token = "${COVEN_TOKEN}"
```

## Usage

```bash
# Run with default config path
coven-telegram-bridge

# Run with custom config path
coven-telegram-bridge --config /path/to/config.toml

# Or use environment variable
COVEN_TELEGRAM_CONFIG=/path/to/config.toml coven-telegram-bridge
```

## Commands

Use these commands in any Telegram chat with the bot:

| Command | Description |
|---------|-------------|
| `/coven bind <agent-id>` | Bind chat to an agent |
| `/coven unbind` | Unbind chat from current agent |
| `/coven status` | Show current binding status |
| `/coven agents` | List available agents |
| `/coven help` | Show help message |

## Response Modes

### Mention Mode (default)

The bot only responds when:
- Mentioned with `@bot_username`
- In a private 1:1 chat
- Replying to the bot's message

### All Mode

The bot responds to every message in allowed chats.

## Architecture

```
Telegram API  <--Long Polling-->  coven-telegram-rs  <--gRPC-->  coven-gateway
                                        |
                                        v
                                  /coven commands
                                  Chat bindings (in-memory)
                                  Context handling
```

## Development

```bash
# Check compilation
cargo check -p coven-telegram-rs

# Run tests
cargo test -p coven-telegram-rs

# Run with debug logging
RUST_LOG=coven_telegram_rs=debug cargo run -p coven-telegram-rs
```

## License

MIT
