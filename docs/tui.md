# coven-tui

Terminal user interface for interacting with coven agents.

## Overview

`coven-tui` (binary: `coven-chat`) provides a rich terminal interface for:

- Real-time chat with agents
- Message streaming with syntax highlighting
- Theme customization
- Multi-agent switching

## Installation

```bash
# Build from source
make coven-chat

# Install globally
cargo install --path crates/coven-tui

# Run
coven-chat
```

## Usage

### Basic

```bash
# Start TUI
coven-chat

# Connect to specific gateway
coven-chat --gateway localhost:50051

# Start with specific agent
coven-chat --agent my-agent
```

### Command Line Options

| Option | Description | Default |
|--------|-------------|---------|
| `--gateway <ADDR>` | Gateway address | `localhost:50051` |
| `--agent <NAME>` | Initial agent | (first available) |
| `--theme <NAME>` | Color theme | `default` |
| `--config <PATH>` | Config file | `~/.config/coven/tui.toml` |

## Interface

```
┌─────────────────────────────────────────────────────────┐
│ coven-chat                          [my-agent] [online] │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  You: Can you help me with this code?                   │
│                                                         │
│  Agent: Of course! Let me take a look...                │
│                                                         │
│  ▌ Thinking...                                          │
│                                                         │
│                                                         │
├─────────────────────────────────────────────────────────┤
│ > Type your message...                              │ ↵ │
└─────────────────────────────────────────────────────────┘
```

### Panels

| Panel | Description |
|-------|-------------|
| Header | Agent name and status |
| Messages | Conversation history |
| Input | Message input area |
| Status | Connection status, typing indicator |

## Keyboard Shortcuts

### Navigation

| Key | Action |
|-----|--------|
| `↑` / `↓` | Scroll messages |
| `Page Up` / `Page Down` | Scroll page |
| `Home` / `End` | Jump to start/end |

### Input

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Shift+Enter` | New line |
| `Ctrl+C` | Cancel input |
| `Ctrl+L` | Clear screen |

### Application

| Key | Action |
|-----|--------|
| `Ctrl+Q` | Quit |
| `Ctrl+A` | Switch agent |
| `Ctrl+T` | Change theme |
| `Ctrl+N` | New conversation |
| `?` | Show help |

## Commands

Type `/` to enter command mode:

| Command | Description |
|---------|-------------|
| `/help` | Show help |
| `/quit` | Exit TUI |
| `/clear` | Clear messages |
| `/agents` | List agents |
| `/switch <name>` | Switch agent |
| `/theme <name>` | Change theme |
| `/thread` | Show thread info |

## Configuration

### Config File

Located at `~/.config/coven/tui.toml`:

```toml
# Gateway connection
gateway_url = "localhost:50051"

# Default agent
default_agent = "my-agent"

# Appearance
theme = "default"
show_timestamps = true
show_thinking = true

# Input
vim_mode = false
history_size = 1000

# SSH authentication
ssh_key = "~/.ssh/id_ed25519"
```

### Themes

Available themes:

| Theme | Description |
|-------|-------------|
| `default` | Dark theme with blue accents |
| `light` | Light background |
| `midnight` | Deep blue dark theme |
| `matrix` | Green on black |
| `ember` | Warm orange tones |
| `rose` | Pink accents |

Switch themes:

```bash
# Command line
coven-chat --theme matrix

# In TUI
/theme matrix

# Or Ctrl+T to cycle
```

## Features

### Markdown Rendering

Messages support markdown:

- **Bold** and *italic* text
- `inline code` and code blocks
- Lists and headers
- Links (displayed, not clickable)

### Syntax Highlighting

Code blocks are syntax highlighted:

```rust
fn main() {
    println!("Hello, coven!");
}
```

### Streaming

Responses stream in real-time:

- Thinking indicator while processing
- Text appears character by character
- Tool use/results shown inline

### Message History

- Scroll through past messages
- Search with `/search <query>`
- Export with `/export <file>`

## Architecture

```
coven-tui/
├── src/
│   ├── main.rs           # Entry point
│   ├── lib.rs            # Library exports
│   ├── app.rs            # Main application
│   ├── app_event.rs      # Event handling
│   ├── client_bridge.rs  # Gateway client wrapper
│   ├── error.rs          # Error types
│   ├── state/            # Application state
│   ├── theme/            # Theme definitions
│   ├── tui/              # Terminal handling
│   └── widgets/          # UI components
└── Cargo.toml
```

### Dependencies

```
coven-tui
└── coven-client    # Gateway communication
    ├── coven-grpc  # gRPC client
    └── coven-ssh   # Authentication
```

## Troubleshooting

### Display Issues

```
Error: terminal does not support required features
```

- Ensure terminal supports 256 colors
- Try `TERM=xterm-256color coven-chat`
- Use a modern terminal (iTerm2, Alacritty, etc.)

### Connection Lost

```
[disconnected] Reconnecting...
```

- Check gateway is running
- Verify network connectivity
- TUI will auto-reconnect

### Input Not Working

- Check terminal is in raw mode
- Try resizing terminal
- Exit and restart

## See Also

- [CLI Reference](cli.md) - Command-line alternative
- [Client Library](client.md) - Underlying client
- [Architecture](architecture.md) - System overview
