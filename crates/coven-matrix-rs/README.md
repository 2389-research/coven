# coven-matrix-rs

Rust Matrix bridge for coven-gateway. Routes messages between Matrix rooms and coven agents via gRPC.

## Installation

```bash
cargo install --path .
```

## Quick Start

Run the interactive setup wizard:

```bash
coven-matrix-bridge --setup
```

This walks you through configuring the bridge and saves the config to `~/.config/coven/matrix-bridge.toml`.

## Manual Configuration

Create `~/.config/coven/matrix-bridge.toml`:

```toml
[matrix]
homeserver = "https://matrix.org"
username = "@your-bot:matrix.org"
password = "${MATRIX_PASSWORD}"

[gateway]
host = "localhost"
port = 6666
tls = false
token = "${COVEN_TOKEN}"

[bridge]
# Restrict to specific rooms (empty = allow all)
allowed_rooms = []
# Restrict to specific users (empty = allow all)
allowed_senders = []
typing_indicator = true
```

## Usage

```bash
# Run with default config location
coven-matrix-bridge

# Run with custom config
coven-matrix-bridge --config /path/to/config.toml

# Set config via environment
COVEN_MATRIX_CONFIG=/path/to/config.toml coven-matrix-bridge
```

## Commands

**In a DM with the bot:**

- `!coven bind <bind-id>` - Create a room bound to an agent (e.g., `!coven bind ef2bbe1b-f0f`)
- `!coven agents` - List available agents
- `!coven rooms` - List your bound rooms
- `!coven help` - Show help

**In a bound room:**

- `!coven status` - Show current binding
- `!coven unbind` - Unbind room from agent
- `!coven help` - Show help

### Quick Start Flow

1. DM the bot: `!coven bind <agent-bind-id>`
2. Accept the room invite
3. Start chatting with the agent!

## Environment Variables

- `MATRIX_PASSWORD` - Matrix account password
- `COVEN_TOKEN` - Gateway authentication token
- `COVEN_MATRIX_CONFIG` - Config file path
- `RUST_LOG` - Logging level (e.g., `coven_matrix_rs=debug`)
