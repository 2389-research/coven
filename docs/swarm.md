# coven-swarm

Supervisor daemon that orchestrates multiple workspace agents.

## Overview

`coven-swarm` manages a fleet of agents, one per workspace directory. It:

- Discovers workspaces in a configured directory
- Spawns and monitors agent processes
- Provides a unified status view
- Handles agent lifecycle (start, stop, restart)

## Components

| Crate | Description |
|-------|-------------|
| `coven-swarm` | Main binary with supervisor and agent modes |
| `coven-swarm-core` | Shared configuration and types |
| `coven-swarm-backend` | Pluggable Claude backend abstraction |

## Installation

```bash
# Build from source
make coven-swarm

# Or install globally
cargo install --path crates/coven-swarm
```

## Usage

### Initialize Configuration

```bash
coven-swarm init
```

Creates config at `~/.config/coven/swarm/config.toml`.

### Run Supervisor

```bash
coven-swarm supervisor
```

Starts the supervisor daemon that spawns workspace agents.

### Run Single Agent (Advanced)

```bash
# Usually spawned by supervisor, but can run directly
coven-swarm agent --workspace myproject
```

## Configuration

### Config File

Located at `~/.config/coven/swarm/config.toml`:

```toml
# Gateway connection
gateway_url = "localhost:50051"

# Agent naming
prefix = "home"  # Agents named: home_project1, home_project2, etc.

# Workspace discovery
working_directory = "/home/user/projects"

# Backend selection
default_backend = "acp"  # acp, mux, direct

# Supervisor settings
[supervisor]
socket_path = "/tmp/coven-swarm.sock"  # Unix socket for IPC
```

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `COVEN_GATEWAY` | Gateway address | `localhost:50051` |
| `COVEN_SWARM_CONFIG` | Config file path | `~/.config/coven/swarm/config.toml` |
| `RUST_LOG` | Log level | `info` |

## Architecture

### Supervisor Mode

```
┌────────────────────────────────────────────────────────────┐
│                    coven-swarm supervisor                   │
│                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐        │
│  │   Discover  │  │   Spawn     │  │   Monitor   │        │
│  │  workspaces │──│   agents    │──│   health    │        │
│  └─────────────┘  └─────────────┘  └─────────────┘        │
│         │                                                   │
│         ▼                                                   │
│  ┌─────────────────────────────────────────────────────┐  │
│  │                    Unix Socket                       │  │
│  │              /tmp/coven-swarm.sock                   │  │
│  └─────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────┘
         │                │                │
         ▼                ▼                ▼
    ┌─────────┐      ┌─────────┐      ┌─────────┐
    │ Agent 1 │      │ Agent 2 │      │ Agent 3 │
    │project_a│      │project_b│      │project_c│
    └─────────┘      └─────────┘      └─────────┘
         │                │                │
         └────────────────┼────────────────┘
                          ▼
                   ┌─────────────┐
                   │   Gateway   │
                   └─────────────┘
```

### Workspace Discovery

The supervisor scans `working_directory` for subdirectories:

```
/home/user/projects/
├── project_a/     → spawns agent "home_project_a"
├── project_b/     → spawns agent "home_project_b"
└── project_c/     → spawns agent "home_project_c"
```

### Agent Naming

Agents are named `{prefix}_{workspace}`:

- Prefix from config (e.g., "home", "work")
- Workspace is the directory name
- Example: `home_myproject`

## Backends

### ACP Backend (Default)

Agent Communication Protocol - uses Claude Code CLI.

```toml
default_backend = "acp"
```

### Mux Backend

Direct Anthropic API integration.

```toml
default_backend = "mux"
```

Requires `ANTHROPIC_API_KEY`.

### Direct Backend

Direct CLI subprocess.

```toml
default_backend = "direct"
```

## Supervisor IPC

The supervisor exposes a Unix socket for control:

```bash
# Socket location
/tmp/coven-swarm.sock

# Or from config
socket_path = "/var/run/coven/swarm.sock"
```

### Commands

| Command | Description |
|---------|-------------|
| `status` | List all agents and their state |
| `restart <name>` | Restart specific agent |
| `stop <name>` | Stop specific agent |
| `start <name>` | Start stopped agent |

## Dispatch Mode

Agents can run in dispatch mode for tool-only execution:

```bash
coven-swarm agent --workspace myproject --dispatch-mode
```

In dispatch mode, agents:
- Only execute tool calls
- Don't process conversational messages
- Useful for background task execution

## Logging

```bash
# Supervisor logs
RUST_LOG=coven_swarm=debug coven-swarm supervisor

# Agent-specific logs
RUST_LOG=coven_swarm::agent=debug coven-swarm agent --workspace foo
```

## Deployment

### Systemd Service

```ini
# /etc/systemd/system/coven-swarm.service
[Unit]
Description=Coven Swarm Supervisor
After=network.target

[Service]
Type=simple
User=coven
Environment=RUST_LOG=info
Environment=ANTHROPIC_API_KEY=sk-...
ExecStart=/usr/local/bin/coven-swarm supervisor
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

### Docker

```dockerfile
FROM rust:1.75-slim as builder
WORKDIR /app
COPY . .
RUN cargo build --release -p coven-swarm

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/coven-swarm /usr/local/bin/
CMD ["coven-swarm", "supervisor"]
```

```yaml
# docker-compose.yml
services:
  swarm:
    build: .
    environment:
      - ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}
      - COVEN_GATEWAY=gateway:50051
    volumes:
      - ./workspaces:/workspaces
      - ./config:/root/.config/coven
    depends_on:
      - gateway
```

## Troubleshooting

### Agents Not Starting

```
Error: failed to spawn agent for workspace X
```

- Check workspace directory exists
- Verify permissions on working_directory
- Check supervisor logs for details

### Socket Connection Failed

```
Error: failed to connect to supervisor socket
```

- Verify supervisor is running
- Check socket path permissions
- Ensure socket_path directory exists

### Gateway Connection Issues

```
Error: agent failed to connect to gateway
```

- Verify gateway is running
- Check gateway_url in config
- Test with: `grpcurl -plaintext localhost:50051 list`

## See Also

- [coven-agent](agent.md) - Single-workspace agent
- [Architecture](architecture.md) - System overview
- [CLI Reference](cli.md) - Unified CLI commands
