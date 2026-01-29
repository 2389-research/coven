# Coven TUI Redesign

This document captures the design for a simplified, robust rewrite of coven-tui.

## Motivation

The current TUI (~2,500 lines, 24 files) has accumulated complexity. The coven-gateway project demonstrates that a simple architecture can be both effective and robust. This redesign follows those patterns while keeping rich terminal UI features.

## Goals

- **Simpler**: 6 core files vs 24
- **Robust**: Channel-based async, clean state management
- **Feature-complete**: Agent picker, scrollable chat, streaming, metadata display

## Non-Goals

- Themes (single color palette)
- Starfield animation
- Top status bar (bottom only)
- CLI subcommands beyond `send` and `setup`

## CLI Interface

```
coven-chat                      # Launch TUI with picker
coven-chat --agent <name>       # Launch TUI, skip to chat with agent
coven-chat send -p <message>    # Non-interactive, print response to stdout
coven-chat setup                # First-time configuration wizard
```

## Module Structure

```
coven-tui/src/
├── main.rs           # Entry point, CLI args, config loading
├── app.rs            # App struct, state, handle_key(), handle_response()
├── ui/
│   ├── mod.rs        # render() function - dispatches to widgets
│   ├── chat.rs       # Render message list + streaming response
│   ├── input.rs      # Render input area (wraps tui-textarea)
│   ├── picker.rs     # Render agent picker overlay
│   └── status.rs     # Render bottom status bar
├── client.rs         # Thin wrapper around coven-client, returns channel
└── types.rs          # Message, Agent, StreamingMessage, Mode enum
```

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ Input Task  │────▶│   Main      │◀────│ gRPC Client │
│ (key events)│     │   Loop      │     │ (responses) │
└─────────────┘     └─────────────┘     └─────────────┘
                          │
                          ▼
                    ┌───────────┐
                    │  Render   │
                    └───────────┘
```

- Input runs in separate tokio task (crossterm events are blocking)
- Responses stream through a channel from coven-client
- Main loop uses `tokio::select!` on input, responses, and tick (for animations)
- No event enum - state mutations happen directly in handlers

## Types

```rust
pub enum Mode {
    Picker,      // Selecting an agent
    Chat,        // Normal chat view
    Sending,     // Message in flight (disable input)
}

pub struct Agent {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub model: Option<String>,
    pub capabilities: Vec<String>,
    pub connected: bool,
}

pub struct Message {
    pub role: Role,           // User | Assistant
    pub content: String,
    pub thinking: Option<String>,
    pub tool_uses: Vec<ToolUse>,
    pub timestamp: DateTime<Utc>,
    pub tokens: Option<MessageTokens>,
}

pub struct MessageTokens {
    pub input: u32,
    pub output: u32,
}

pub struct ToolUse {
    pub name: String,
    pub status: ToolStatus,   // Running | Complete | Error
}

pub struct StreamingMessage {
    pub content: String,
    pub thinking: Option<String>,
    pub tool_uses: Vec<ToolUse>,
}

pub struct SessionMetadata {
    pub thread_id: String,
    pub model: String,
    pub working_dir: Option<String>,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub total_cost: f64,
}
```

## App State

```rust
pub struct App {
    pub mode: Mode,
    pub agents: Vec<Agent>,
    pub selected_agent: Option<String>,

    // Chat state
    pub messages: Vec<Message>,
    pub streaming: Option<StreamingMessage>,
    pub scroll_offset: usize,

    // Input state
    pub input: TextArea<'static>,
    pub input_history: Vec<String>,
    pub history_index: Option<usize>,

    // Picker state
    pub picker_filter: String,
    pub picker_index: usize,

    // Session metadata
    pub session: SessionMetadata,

    // Connection
    pub connected: bool,
    pub error: Option<String>,

    // Quit handling
    pub last_ctrl_c: Option<Instant>,
}
```

## Main Loop

```rust
async fn run_app(
    terminal: &mut Terminal<impl Backend>,
    config: Config,
    initial_agent: Option<String>,
) -> Result<()> {
    let (key_tx, mut key_rx) = mpsc::channel(32);
    let (response_tx, mut response_rx) = mpsc::channel(32);

    // Spawn input handler
    let input_handle = tokio::spawn(input_task(key_tx));

    // Create client & app
    let client = Client::new(&config)?;
    let mut app = App::new(initial_agent);

    // Fetch agents on startup
    app.agents = client.list_agents().await?;

    let mut tick = tokio::time::interval(Duration::from_millis(100));

    loop {
        terminal.draw(|f| ui::render(f, &app))?;

        tokio::select! {
            Some(key) = key_rx.recv() => {
                if let Some(action) = app.handle_key(key) {
                    match action {
                        Action::Quit => break,
                        Action::SendMessage(msg) => {
                            // Spawn send task...
                        }
                    }
                }
            }
            Some(response) = response_rx.recv() => {
                app.handle_response(response);
            }
            _ = tick.tick() => {
                // Throbber animation
            }
        }
    }

    input_handle.abort();
    app.save(&config_dir)?;
    Ok(())
}
```

## Client Wrapper

```rust
pub enum Response {
    Text(String),
    Thinking(String),
    ToolStart(String),
    ToolComplete(String),
    ToolError(String, String),
    Metadata(MetadataUpdate),
    Done,
    Error(String),
}

pub enum MetadataUpdate {
    WorkingDir(String),
    TokenUsage { input: u32, output: u32 },
    Cost(f64),
}
```

The client is a thin wrapper around coven-client that:
- Converts `coven_client::StreamEvent` to our `Response` enum
- Streams responses through a channel
- Handles errors through the same channel

## UI Layout

```
┌──────────────────────────────────────────────────────────────┐
│ You 14:32                                                    │
│   What files handle routing?                                 │
│                                                              │
│ claude-main 14:32 (423↓)                                     │
│   [thinking...]                                              │
│   ✓ Read                                                     │
│   ✓ Grep                                                     │
│   The routing is handled in src/router.rs...                 │
│                                                              │
│ claude-main (streaming)                                      │
│   [thinking...]                                              │
│   ⠋ Read                                                     │
│   Looking at the code...                                     │
├──────────────────────────────────────────────────────────────┤
│ > Write your message here...                                 │
│   (multiline supported with Shift+Enter)                     │
│                                                              │
├──────────────────────────────────────────────────────────────┤
│ claude-main (claude-3-opus) ● │ ~/proj │ 1.2k↑ 3.4k↓ │ $0.02 │
└──────────────────────────────────────────────────────────────┘
```

Agent picker is rendered as a centered overlay when `mode == Mode::Picker`.

## Key Bindings

| Key | Mode | Action |
|-----|------|--------|
| `Ctrl+Q` | Any | Quit immediately |
| `Ctrl+C` (×2) | Any | Quit (within 500ms) |
| `Ctrl+Space` | Any | Open picker |
| `Esc` | Picker | Close picker |
| `Enter` | Picker | Select agent |
| `↑/↓` | Picker | Navigate list |
| `Type` | Picker | Filter agents |
| `Enter` | Chat | Send message |
| `Shift+Enter` | Chat | Newline in input |
| `Ctrl+↑/↓` | Chat | Scroll chat |
| `PageUp/Down` | Chat | Scroll chat (fast) |
| `↑/↓` | Chat (empty input) | History navigation |

## Metadata Display

| Data | Location | Updates |
|------|----------|---------|
| Model | Status bar | On agent select |
| Connection status | Status bar | On connect/disconnect |
| Working directory | Status bar | During stream |
| Token totals | Status bar | After each message |
| Cost | Status bar | After each message |
| Thread ID | Status bar | On session start |
| Timestamp | Per-message | On message complete |
| Message tokens | Per-message | On message complete |
| Agent capabilities | Picker | On agent list refresh |

## Persistence

Saved to `~/.config/coven-chat/state.json`:

```rust
struct PersistedState {
    last_agent: Option<String>,
    input_history: Vec<String>,  // Max 100 entries
}
```

Configuration saved to `~/.config/coven-chat/config.toml`:

```toml
[gateway]
url = "localhost:7777"
```

## CLI Commands

### `coven-chat send -p <message>`

Non-interactive mode for scripting:
- Streams response text to stdout
- Ignores thinking/tool use output
- Exits 0 on success, 1 on error

### `coven-chat setup`

Interactive first-time setup:
1. Prompt for gateway URL (default: localhost:7777)
2. Test connection
3. Save config file
4. Print success message

## Dependencies

- `ratatui` - Terminal UI
- `crossterm` - Terminal control
- `tui-textarea` - Multiline input
- `tokio` - Async runtime
- `coven-client` - Gateway communication
- `coven-ssh` - SSH key auth
- `clap` - CLI parsing
- `serde` / `serde_json` - Persistence
- `chrono` - Timestamps
- `anyhow` - Error handling

## Migration Path

1. Create new implementation alongside existing code
2. Test against real gateway
3. Replace existing coven-tui with new implementation
4. Remove old code

## Open Questions

None - design is complete.
