# coven-slack-rs

Slack bridge for coven-gateway using Socket Mode WebSocket connection.

## Overview

coven-slack-rs connects Slack workspaces to coven agents through coven-gateway. It uses Slack's Socket Mode for real-time message delivery without requiring a public HTTP endpoint.

## Features

- **Socket Mode** - WebSocket connection, no public endpoint needed
- **Slash commands** - `/coven bind`, `/coven unbind`, `/coven status`, `/coven agents`
- **Flexible response mode** - Respond to all messages or only @mentions
- **Thread support** - Keeps channels clean by responding in threads
- **Channel filtering** - Restrict to specific channels

## Setup

### 1. Create a Slack App

1. Go to [api.slack.com/apps](https://api.slack.com/apps) and create a new app
2. Enable Socket Mode in "Socket Mode" settings
3. Create an App-Level Token with `connections:write` scope
4. Add the following bot token scopes in "OAuth & Permissions":
   - `chat:write` - Post messages
   - `app_mentions:read` - Receive @mention events
   - `channels:history` - Read channel messages
   - `im:history` - Read DM messages
   - `groups:history` - Read private channel messages
5. Install the app to your workspace
6. Copy the Bot User OAuth Token (xoxb-...)

### 2. Configure the Bridge

Copy `config.example.toml` to `~/.config/coven/slack-bridge.toml`:

```bash
mkdir -p ~/.config/coven
cp config.example.toml ~/.config/coven/slack-bridge.toml
```

Edit the config file and set your tokens:

```toml
[slack]
app_token = "${SLACK_APP_TOKEN}"  # xapp-...
bot_token = "${SLACK_BOT_TOKEN}"  # xoxb-...

[gateway]
url = "http://localhost:6666"
token = "${COVEN_TOKEN}"

[bridge]
response_mode = "mention"  # or "all"
thread_replies = true
```

### 3. Run the Bridge

```bash
# Set environment variables
export SLACK_APP_TOKEN="xapp-..."
export SLACK_BOT_TOKEN="xoxb-..."
export COVEN_TOKEN="your-gateway-token"

# Run the bridge
cargo run --bin coven-slack-bridge

# Or with a custom config path
cargo run --bin coven-slack-bridge -- --config /path/to/config.toml
```

## Usage

### Commands

In any Slack channel where the bot is present:

| Command | Description |
|---------|-------------|
| `/coven bind <agent-id>` | Bind channel to an agent |
| `/coven unbind` | Unbind channel from current agent |
| `/coven status` | Show current binding status |
| `/coven agents` | List available agents |
| `/coven help` | Show help message |

### Response Behavior

The bot responds based on context and configuration:

| Context | response_mode=mention | response_mode=all |
|---------|----------------------|-------------------|
| Thread reply | Always responds | Always responds |
| DM | Always responds | Always responds |
| @mention in channel | Responds | Responds |
| Channel message | Ignores | Responds |

## Configuration Options

| Option | Description | Default |
|--------|-------------|---------|
| `slack.app_token` | App-level token for Socket Mode | Required |
| `slack.bot_token` | Bot token for API calls | Required |
| `gateway.url` | coven-gateway gRPC URL | Required |
| `gateway.token` | Authentication token | Optional |
| `bridge.allowed_channels` | Restrict to these channels | [] (all) |
| `bridge.response_mode` | "mention" or "all" | "mention" |
| `bridge.typing_indicator` | Show typing indicator | true |
| `bridge.thread_replies` | Reply in threads | true |

## Environment Variables

- `COVEN_SLACK_CONFIG` - Path to config file
- `SLACK_APP_TOKEN` - Slack app-level token
- `SLACK_BOT_TOKEN` - Slack bot token
- `COVEN_TOKEN` - Gateway authentication token

## Building

```bash
# Build the binary
cargo build --release -p coven-slack-rs

# Run tests
cargo test -p coven-slack-rs

# The binary will be at target/release/coven-slack-bridge
```

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
