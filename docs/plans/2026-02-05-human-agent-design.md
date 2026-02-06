# Human Agent Design

**Date**: 2026-02-05
**Status**: Approved
**Owner**: Claude (Brainstorming skill)

## Overview

`coven human` is a new TUI command that allows humans to act as agents in the coven gateway. It registers as a regular agent with `capabilities: ["human"]` and provides a simple message-in/message-out interface for receiving and responding to messages in real-time.

## Use Cases

- **Workflow testing**: Test agent workflows by having a human act as an agent
- **Human-in-the-loop**: Insert human judgment into agent coordination flows
- **Debugging**: Observe message flow and agent interactions directly
- **Hybrid teams**: Mix AI agents and human agents in the same system

## Architecture

### Component Structure

```
coven-human/
├── Cargo.toml
├── src/
│   ├── lib.rs          # Public API: run_human() function
│   ├── app.rs          # Main application state machine
│   ├── ui.rs           # Ratatui rendering logic
│   └── messages.rs     # Message types for bubbletea-style events
```

### Dependencies

- **coven-client**: Gateway connection, authentication, message streaming
- **coven-grpc**: gRPC protocol handling
- **coven-ssh**: SSH key generation and management
- **ratatui**: TUI framework
- **crossterm**: Terminal control
- **tokio**: Async runtime

### CLI Integration

Add to `coven-cli`:

```rust
Commands::Human {
    /// Gateway server URL
    #[arg(short, long)]
    gateway: Option<String>,

    /// Agent name (defaults to hostname)
    #[arg(short, long)]
    name: Option<String>,

    /// Agent ID (auto-generated if not provided)
    #[arg(long)]
    id: Option<String>,
}
```

## Registration & Connection

On startup:

1. Load or generate SSH key via `coven-ssh`
2. Connect to gateway gRPC endpoint
3. Register as agent with:
   - `agent_id`: Auto-generated UUID or user-provided
   - `name`: User-provided or hostname
   - `capabilities`: `["human"]`
4. Enter main message loop

## State Machine

```
┌──────────────┐
│ Connecting   │
└──────┬───────┘
       │
       ▼
┌──────────────┐
│  Connected   │◄─────────────┐
│  (waiting)   │              │
└──────┬───────┘              │
       │                      │
       │ Message arrives      │
       ▼                      │
┌──────────────┐              │
│  Viewing     │              │
│  (readonly)  │              │
└──────┬───────┘              │
       │                      │
       │ Press 'r' to reply   │
       ▼                      │
┌──────────────┐              │
│  Composing   │              │
│  (editing)   │              │
└──────┬───────┘              │
       │                      │
       │ Press Enter to send  │
       └──────────────────────┘
```

## Message Handling

- Messages arrive via gRPC bidirectional stream
- All messages displayed immediately in viewport
- Multiple messages accumulate in scrollable viewport
- Human scrolls to see all messages
- Press `r` to start composing reply
- Type response in text area
- Press Enter to send complete message (no streaming)

## UI Layout

### Viewing Mode

```
┌─────────────────────────────────────────────────────────────┐
│ coven human | Connected: agent-abc123 | Server: gateway-01  │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  [2026-02-05 10:23:45] Message from thread-xyz:             │
│  Hey, can you check if the deployment succeeded?            │
│                                                              │
│  [2026-02-05 10:24:12] Message from thread-abc:             │
│  What's the status of the database migration?               │
│                                                              │
│  [2026-02-05 10:25:03] Message from thread-xyz:             │
│  Also need you to verify the logs look clean.               │
│                                                              │
├─────────────────────────────────────────────────────────────┤
│ [viewing] Press 'r' to reply, 'q' to quit                   │
└─────────────────────────────────────────────────────────────┘
```

### Composing Mode

```
┌─────────────────────────────────────────────────────────────┐
│ coven human | Composing reply                                │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  [2026-02-05 10:23:45] Message from thread-xyz:             │
│  Hey, can you check if the deployment succeeded?            │
│                                                              │
├─────────────────────────────────────────────────────────────┤
│ Reply (Enter to send, Esc to cancel):                       │
│ Yes, deployment succeeded! All services are up.█            │
│                                                              │
├─────────────────────────────────────────────────────────────┤
│ [composing] Enter to send, Esc to cancel                    │
└─────────────────────────────────────────────────────────────┘
```

## UI Components

- **Header bar**: Connection status, agent ID, server info
- **Message viewport**: Scrollable area with timestamps and messages
- **Compose area**: Multi-line text input (appears on 'r')
- **Status bar**: Current mode and keybindings

## Key Bindings

| Key | Action |
|-----|--------|
| `r` | Start reply (enter compose mode) |
| `Esc` | Cancel compose, return to viewing |
| `Enter` | Send message (when composing) |
| `Ctrl-C` or `q` | Quit application |
| `Up/Down` or `j/k` | Scroll viewport |

## Error Handling

### Connection Errors
- **Startup failure**: Display error, allow retry or exit
- **Connection lost**: Show "Disconnected" in header, attempt reconnection
- **Reconnection success**: Resume stream, show "Reconnected" notification

### Message Handling
- **Empty message**: Ignore, don't send
- **Long messages**: Allow multi-line input, no artificial limit
- **Rapid influx**: All messages accumulate, human scrolls
- **No messages**: Show "Waiting for messages..." placeholder

### Terminal Handling
- **Resize**: Recalculate layout, redraw
- **Too small**: Warn if below minimum (80x24)
- **Ctrl-C**: Clean shutdown, close gRPC, restore terminal

### Send Failures
- **Message send fails**: Show error in status bar, keep draft for retry
- **Network timeout**: Display timeout error, allow retry

## Testing Strategy

### Unit Tests
- Message formatting and display logic
- State transitions (connecting → connected → composing → viewing)
- Key binding handling

### Integration Tests
1. Start local gateway: `coven serve`
2. Launch `coven human`, verify registration
3. Send test message via `coven-client`, verify display
4. Type response in TUI, verify it reaches gateway

### Manual Tests
- Test with real gateway and multiple agents
- Verify human agent appears in `coven admin agents list`
- Bind human agent to Matrix/Slack channel, verify bidirectional flow
- Test rapid message influx (10+ messages)

## Implementation Checklist

### New Files
- [ ] `coven/crates/coven-human/Cargo.toml`
- [ ] `coven/crates/coven-human/src/lib.rs` (~50 lines)
- [ ] `coven/crates/coven-human/src/app.rs` (~150 lines)
- [ ] `coven/crates/coven-human/src/ui.rs` (~100 lines)
- [ ] `coven/crates/coven-human/src/messages.rs` (~30 lines)

### Modified Files
- [ ] `coven/Cargo.toml` - Add `coven-human` to workspace
- [ ] `coven/crates/coven-cli/Cargo.toml` - Add `coven-human` dependency
- [ ] `coven/crates/coven-cli/src/main.rs` - Add `Human` command and handler

### Estimated Effort
- **New code**: ~400 lines
- **Integration code**: ~50 lines
- **Total**: ~450 lines

## Success Criteria

1. Human can connect to gateway as an agent
2. Human receives messages in real-time
3. Human can compose and send responses
4. Human agent appears in agent lists
5. Human agent can be bound to channels
6. Clean error handling and graceful shutdown
7. All tests pass

## Future Enhancements (Out of Scope)

- Message threading/grouping
- Rich text formatting
- File attachments
- Message history persistence
- Multiple concurrent threads
