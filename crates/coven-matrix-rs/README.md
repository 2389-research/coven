# coven-matrix-rs

Rust Matrix bridge for coven-gateway. Routes messages between Matrix rooms and coven agents via gRPC.

## Installation

```bash
cargo install --path .
```

## Configuration

Create `~/.config/coven/matrix-bridge.toml`:

```toml
[matrix]
homeserver = "https://matrix.org"
username = "@your-bot:matrix.org"
password = "${MATRIX_PASSWORD}"

[gateway]
url = "http://localhost:6666"
token = "${COVEN_TOKEN}"

[bridge]
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

In any Matrix room where the bot is present:

- `/coven bind <agent-id>` - Bind room to an agent
- `/coven unbind` - Unbind room from agent
- `/coven status` - Show current binding
- `/coven agents` - List available agents
- `/coven help` - Show help

## Environment Variables

- `MATRIX_PASSWORD` - Matrix account password
- `COVEN_TOKEN` - Gateway authentication token
- `COVEN_MATRIX_CONFIG` - Config file path
- `RUST_LOG` - Logging level (e.g., `coven_matrix_rs=debug`)
