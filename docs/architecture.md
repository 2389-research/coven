# Architecture

This document describes the high-level architecture of the coven platform.

## System Overview

coven is a distributed system for orchestrating AI agents. The architecture consists of three main layers:

1. **Gateway** (Go) - Central coordination server
2. **Agents** (Rust) - AI-powered workers that execute tasks
3. **Frontends** - User interfaces (TUI, HTTP API, Matrix)

## Component Diagram

```
                         ┌─────────────────┐
                         │    Frontends    │
                         │  TUI / HTTP /   │
                         │     Matrix      │
                         └────────┬────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────┐
│                      coven-gateway                           │
│                                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ HTTP Server  │  │ gRPC Server  │  │ Pack Service │      │
│  │   :8080      │  │   :50051     │  │  (registry)  │      │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘      │
│         │                 │                 │               │
│         ▼                 ▼                 ▼               │
│  ┌──────────────────────────────────────────────────┐      │
│  │                   Agent Manager                   │      │
│  │  - Connection tracking                            │      │
│  │  - Request/response correlation                   │      │
│  │  - Agent routing                                  │      │
│  └──────────────────────────────────────────────────┘      │
│                          │                                  │
│                          ▼                                  │
│  ┌──────────────────────────────────────────────────┐      │
│  │                     Store                         │      │
│  │  - Threads (frontend ID → agent session)         │      │
│  │  - Messages (conversation history)               │      │
│  │  - Bindings (channel → agent mapping)            │      │
│  └──────────────────────────────────────────────────┘      │
└─────────────────────────────────────────────────────────────┘
         ▲                    ▲                    ▲
         │                    │                    │
    ┌────┴────┐          ┌────┴────┐          ┌────┴────┐
    │  Agent  │          │  Agent  │          │  Pack   │
    │ (grpc)  │          │ (grpc)  │          │ (grpc)  │
    └─────────┘          └─────────┘          └─────────┘
```

## Data Flow

### Message Flow (User → Agent → Response)

```
1. User sends message via frontend
   └─► HTTP POST /api/send { thread_id, message }

2. Gateway looks up agent binding
   └─► Store.ResolveBinding(frontend, channel_id)

3. Gateway creates request, sends to agent
   └─► Manager.SendMessage(agent_id, request)
   └─► Connection.Send(SendMessage proto)

4. Agent receives message
   └─► gRPC stream receives ServerMessage
   └─► IncomingMessage created, passed to Fold.handle()

5. Agent processes with LLM backend
   └─► Backend.send() returns Stream<BackendEvent>
   └─► Events: Thinking → Text → ToolUse → ToolResult → Done

6. Agent streams responses to gateway
   └─► Each BackendEvent → MessageResponse proto
   └─► gRPC stream sends AgentMessage

7. Gateway correlates and forwards
   └─► Connection matches request_id
   └─► Response channel receives events

8. Frontend receives SSE stream
   └─► HTTP response streams events
   └─► event: text, data: {"content": "..."}
```

### Agent Registration

```
1. Agent connects to gateway gRPC
   └─► CovenControl.AgentStream (bidirectional)

2. Agent sends RegisterAgent message
   └─► Contains: agent_id, metadata, capabilities

3. Gateway sends Welcome response
   └─► Contains: session_id, configuration

4. Agent begins heartbeat loop
   └─► Sends Heartbeat every 30s
   └─► Gateway tracks last_seen

5. Connection maintained until shutdown
   └─► Gateway can send Shutdown message
   └─► Agent gracefully disconnects
```

## Crate Dependencies

```
Layer 0 (no internal deps):
┌─────────────┐  ┌─────────────┐  ┌─────────────────┐
│ coven-proto │  │  coven-ssh  │  │ coven-swarm-core│
└─────────────┘  └─────────────┘  └─────────────────┘

Layer 1:
┌─────────────┐  ┌─────────────┐
│ coven-grpc  │  │ coven-pack  │
│  (proto)    │  │             │
└─────────────┘  └─────────────┘

Layer 2:
┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐
│coven-client │  │ coven-core  │  │ coven-swarm-backend │
│ (grpc,ssh)  │  │   (pack)    │  │   (swarm-core)      │
└─────────────┘  └─────────────┘  └─────────────────────┘

Layer 3:
┌─────────────┐  ┌───────────────────────────────────────┐
│ coven-agent │  │              coven-swarm              │
│ (core,pack) │  │ (swarm-core, swarm-backend, grpc)    │
└─────────────┘  └───────────────────────────────────────┘

Layer 4:
┌─────────────┐  ┌─────────────────────────────────────┐
│  coven-tui  │  │             coven-cli               │
│  (client)   │  │      (client, swarm, agent)         │
└─────────────┘  └─────────────────────────────────────┘
```

## Backend Abstraction

Agents use pluggable backends for LLM communication:

```rust
#[async_trait]
pub trait Backend: Send + Sync {
    async fn send(
        &self,
        session_id: &str,
        message: &str,
        is_new_session: bool,
    ) -> Result<BoxStream<'static, BackendEvent>>;
}
```

### Available Backends

| Backend | Description |
|---------|-------------|
| `MuxBackend` | Direct Anthropic API (recommended) |
| `DirectCliBackend` | Spawns `claude` CLI subprocess |
| `AcpBackend` | Agent Communication Protocol |

## Protocol

The gRPC protocol uses bidirectional streaming:

```protobuf
service CovenControl {
  rpc AgentStream(stream AgentMessage) returns (stream ServerMessage);
}

// Agent → Server
message AgentMessage {
  oneof payload {
    RegisterAgent register = 1;
    MessageResponse response = 2;
    Heartbeat heartbeat = 3;
  }
}

// Server → Agent
message ServerMessage {
  oneof payload {
    Welcome welcome = 1;
    SendMessage send_message = 2;
    Shutdown shutdown = 3;
  }
}
```

### Response Events

Message responses stream as a sequence of events:

```
Thinking    → Agent is processing
Text        → Content chunk (streaming)
ToolUse     → Agent wants to use a tool
ToolResult  → Result of tool execution
Done        → Message complete
Error       → Processing failed
```

## Storage

### Gateway (SQLite)

- **threads** - Maps frontend thread IDs to agent sessions
- **messages** - Conversation history with role and content
- **bindings** - Channel-to-agent routing rules

### Agent (SQLite)

- **threads.db** - Local conversation cache
- **sessions/** - Active session state

## Security

### Authentication

1. **SSH Keys** - Agents authenticate with ED25519 keys
2. **Fingerprinting** - Gateway validates key fingerprints
3. **gRPC Metadata** - Auth tokens in request metadata

### Authorization

- Agents can only access their own sessions
- Pack tools have explicit permission scopes
- Admin endpoints require separate authentication

## Deployment

### Single Agent

```bash
coven-agent run --name my-agent --working-dir /path/to/project
```

### Swarm (Multiple Workspaces)

```bash
# Start supervisor (spawns agents per workspace)
coven-swarm supervisor

# Agents connect automatically
# One agent per subdirectory of working_directory
```

### Gateway

```bash
coven-gateway serve --config config.yaml
```

## Configuration

See individual component docs for configuration details:

- [Agent Configuration](agent.md#configuration)
- [Swarm Configuration](swarm.md#configuration)
- [CLI Configuration](cli.md#configuration)
