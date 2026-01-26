# coven CLI

Unified command-line interface for the coven platform.

## Installation

```bash
# Build from source
make coven

# Install globally
cargo install --path crates/coven-cli

# Verify installation
coven --version
```

## Commands

### Overview

```
coven <COMMAND>

Commands:
  chat      Interactive chat session
  agent     Agent management
  swarm     Swarm operations
  pack      Pack management
  config    Configuration
  help      Show help
```

### `coven chat`

Start an interactive chat session.

```bash
# Default agent
coven chat

# Specific agent
coven chat --agent my-agent

# Specific thread
coven chat --thread thread-123
```

**Options:**

| Option | Description |
|--------|-------------|
| `--agent <NAME>` | Agent to chat with |
| `--thread <ID>` | Thread ID (creates new if not specified) |
| `--gateway <ADDR>` | Gateway address |

### `coven agent`

Manage agents.

```bash
# List agents
coven agent list

# Show agent status
coven agent status my-agent

# Run agent
coven agent run --name my-agent --working-dir /path/to/project

# Create new agent config
coven agent new
```

**Subcommands:**

| Subcommand | Description |
|------------|-------------|
| `list` | List all connected agents |
| `status <NAME>` | Show agent details |
| `run` | Run an agent |
| `new` | Interactive agent setup |

### `coven swarm`

Swarm operations.

```bash
# Initialize swarm config
coven swarm init

# Start supervisor
coven swarm supervisor

# Show swarm status
coven swarm status

# Run single workspace agent
coven swarm agent --workspace myproject
```

**Subcommands:**

| Subcommand | Description |
|------------|-------------|
| `init` | Initialize swarm configuration |
| `supervisor` | Run supervisor daemon |
| `status` | Show swarm status |
| `agent` | Run workspace agent |

### `coven pack`

Pack management.

```bash
# List registered packs
coven pack list

# Show pack tools
coven pack show productivity

# Run a pack
coven pack run productivity
```

**Subcommands:**

| Subcommand | Description |
|------------|-------------|
| `list` | List registered packs |
| `show <NAME>` | Show pack details and tools |
| `run <NAME>` | Run a pack |

### `coven config`

Configuration management.

```bash
# Show current config
coven config show

# Edit config
coven config edit

# Set value
coven config set gateway_url localhost:50051

# Get value
coven config get gateway_url
```

## Configuration

### Config File

Located at `~/.config/coven/cli.toml`:

```toml
# Default gateway
gateway_url = "localhost:50051"

# Default agent for chat
default_agent = "my-agent"

# SSH key for authentication
ssh_key = "~/.ssh/id_ed25519"

# Output format
output_format = "text"  # text, json

# Color output
color = true
```

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `COVEN_GATEWAY` | Gateway address | `localhost:50051` |
| `COVEN_CONFIG` | Config file path | `~/.config/coven/cli.toml` |
| `COVEN_SSH_KEY` | SSH key path | `~/.ssh/id_ed25519` |
| `NO_COLOR` | Disable color output | unset |

## Output Formats

### Text (Default)

Human-readable output:

```
$ coven agent list
NAME          STATUS    WORKSPACE
my-agent      online    /home/user/projects/myproject
other-agent   offline   /home/user/projects/other
```

### JSON

Machine-readable output:

```bash
$ coven agent list --format json
[
  {
    "name": "my-agent",
    "status": "online",
    "workspace": "/home/user/projects/myproject"
  }
]
```

## Interactive Mode

### Chat Session

```
$ coven chat --agent my-agent
Connected to my-agent

You: Hello!

Agent: Hello! How can I help you today?

You: /help

Commands:
  /quit     Exit chat
  /clear    Clear conversation
  /thread   Show thread ID
  /agents   List available agents
  /switch   Switch to different agent

You: /quit
Goodbye!
```

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+C` | Cancel current input |
| `Ctrl+D` | Exit chat |
| `Up/Down` | Navigate history |
| `Tab` | Autocomplete commands |

## Examples

### Basic Workflow

```bash
# 1. Start gateway (separate terminal)
cd ../coven-gateway && make run

# 2. Start an agent
coven agent run --name dev --working-dir ~/projects/myapp

# 3. Chat with agent
coven chat --agent dev
```

### Swarm Workflow

```bash
# 1. Initialize swarm
coven swarm init

# 2. Edit config
coven config edit  # Set working_directory

# 3. Start supervisor
coven swarm supervisor

# 4. Check status
coven swarm status
```

### Pack Development

```bash
# 1. Run test pack
coven pack run test

# 2. Verify registration
coven pack list

# 3. Test with agent
coven chat
You: Use the echo tool with "hello"
```

## Troubleshooting

### Connection Failed

```
Error: failed to connect to gateway at localhost:50051
```

- Verify gateway is running
- Check gateway address in config
- Test connectivity: `grpcurl -plaintext localhost:50051 list`

### Agent Not Found

```
Error: agent "my-agent" not found
```

- List available agents: `coven agent list`
- Check agent is running
- Verify agent name matches

### Authentication Failed

```
Error: authentication failed
```

- Check SSH key exists
- Verify key is authorized on gateway
- Try explicit key: `--ssh-key /path/to/key`

## See Also

- [Agent Guide](agent.md) - Agent configuration
- [Swarm Guide](swarm.md) - Multi-workspace setup
- [Architecture](architecture.md) - System overview
