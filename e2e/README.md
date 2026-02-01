# Coven E2E Test Suite

End-to-end tests for the coven ecosystem. Runs gateway, agents, and test scenarios in Docker containers.

## Quick Start

```bash
# Set your API key
export ANTHROPIC_API_KEY=sk-ant-...

# Run all tests
./run-tests.sh

# Run with rebuild
./run-tests.sh --rebuild

# Keep services running after tests (for debugging)
./run-tests.sh --keep-running
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Docker Network                           │
│                                                             │
│  ┌──────────────┐     ┌────────────────────┐               │
│  │   Gateway    │◄────│  agent-standalone  │               │
│  │   :8080      │     │   (mux backend)    │               │
│  │   :50051     │     └────────────────────┘               │
│  │              │                                           │
│  │              │     ┌────────────────────┐               │
│  │              │◄────│  swarm-supervisor  │               │
│  │              │     │   (ws1, ws2)       │               │
│  └──────────────┘     └────────────────────┘               │
│         ▲                                                   │
│         │                                                   │
│  ┌──────┴───────┐                                          │
│  │ test-runner  │  ← Executes scenarios                    │
│  └──────────────┘                                          │
└─────────────────────────────────────────────────────────────┘
```

## Test Scenarios

### Infrastructure
- `gateway-health` - Gateway responds to health checks
- `agent-list` - Gateway lists registered agents

### Per-Agent Tests
- `simple-message-{agent}` - Agent responds to basic messages
- `pack-tool-log-entry-{agent}` - Agent can create log entries
- `pack-tool-log-search-{agent}` - Agent can search logs

### Parallel Tests
- `parallel-messages` - Multiple agents handle concurrent requests

## Directory Structure

```
e2e/
├── docker-compose.yml      # Service definitions
├── Dockerfile.agent        # Standalone agent image
├── Dockerfile.swarm        # Swarm supervisor image
├── Dockerfile.test-runner  # Test runner image
├── run-tests.sh           # Main entry point
├── test_runner.py         # Python test implementation
├── config/
│   ├── gateway.yaml       # Gateway config
│   ├── agent.toml         # Standalone agent config
│   └── swarm.toml         # Swarm config
├── scenarios/
│   └── pack-tools.jsonl   # Scenario definitions
├── workspaces/            # Agent workspaces
└── results/               # Test results output
```

## Scenarios Format

Scenarios are defined in JSONL format:

```json
{
  "name": "pack-tool-log-entry-standalone",
  "description": "Standalone agent can use log_entry pack tool",
  "given": ["Agent running with mux backend"],
  "when": ["Send message asking to use log_entry tool"],
  "then": ["Tool returns success with entry ID"],
  "validates": ["pack-tools", "log-entry"]
}
```

## Results

Test results are written to `results/e2e-results.json`:

```json
{
  "passed": 10,
  "failed": 0,
  "results": [
    {
      "name": "gateway-health",
      "passed": true,
      "duration_ms": 45,
      "error": null
    }
  ]
}
```

## Debugging

```bash
# Keep services running
./run-tests.sh --keep-running

# View logs
docker compose logs -f gateway
docker compose logs -f agent-standalone
docker compose logs -f swarm-supervisor

# Shell into container
docker compose exec gateway sh
docker compose exec test-runner bash

# Manual test
curl http://localhost:8080/api/agents
curl -X POST http://localhost:8080/api/send \
  -H "Content-Type: application/json" \
  -d '{"agent_id":"e2e-standalone","content":"hello","sender":"debug"}'

# Stop everything
docker compose down -v
```

## CI Integration

```yaml
# GitHub Actions example
jobs:
  e2e:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Run E2E tests
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: |
          cd coven/e2e
          ./run-tests.sh
```
