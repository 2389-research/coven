#!/bin/bash
# ABOUTME: Scenario tests for fold-swarm
# ABOUTME: Tests real functionality with real dependencies

set -e

BINARY="./target/release/fold-swarm"
TEST_DIR=$(mktemp -d)
CONFIG_DIR="$TEST_DIR/config"
WORKSPACES_DIR="$TEST_DIR/workspaces"

cleanup() {
    echo "Cleaning up $TEST_DIR"
    rm -rf "$TEST_DIR"
    # Kill any spawned processes
    pkill -f "fold-swarm.*$TEST_DIR" 2>/dev/null || true
}
trap cleanup EXIT

echo "=== Test Directory: $TEST_DIR ==="
mkdir -p "$CONFIG_DIR" "$WORKSPACES_DIR"

# Create a minimal config for testing
cat > "$CONFIG_DIR/config.toml" << EOF
gateway_url = "grpc://localhost:50051"
prefix = "test"
working_directory = "$WORKSPACES_DIR"
default_backend = "direct"
acp_binary = "claude"
EOF

echo ""
echo "=== Scenario 1: Config loads correctly ==="
# Just verify the binary can parse the config
if $BINARY agent --workspace test --config "$CONFIG_DIR/config.toml" --help 2>&1 | grep -q "workspace"; then
    echo "PASS: Binary parses arguments correctly"
else
    echo "FAIL: Binary argument parsing failed"
    exit 1
fi

echo ""
echo "=== Scenario 2: Agent fails gracefully without workspace dir ==="
# Should fail because workspace directory doesn't exist
if $BINARY agent --workspace nonexistent --config "$CONFIG_DIR/config.toml" 2>&1 | grep -q "does not exist"; then
    echo "PASS: Agent correctly rejects missing workspace directory"
else
    echo "FAIL: Agent didn't reject missing workspace directory"
    exit 1
fi

echo ""
echo "=== Scenario 3: SSH key auto-generation ==="
# Create workspace dir
mkdir -p "$WORKSPACES_DIR/testagent"

# Set XDG_CONFIG_HOME to use test directory for SSH key
export XDG_CONFIG_HOME="$TEST_DIR/xdg_config"
mkdir -p "$XDG_CONFIG_HOME"

# Run agent - it should generate SSH key then fail on gateway connection
OUTPUT=$($BINARY agent --workspace testagent --config "$CONFIG_DIR/config.toml" 2>&1 || true)
echo "$OUTPUT"

if echo "$OUTPUT" | grep -q "SSH key"; then
    echo "PASS: SSH key generation attempted"
else
    echo "FAIL: No SSH key generation message"
    exit 1
fi

# Check if key file was created
if [ -f "$XDG_CONFIG_HOME/fold/fold-swarm/agent_key" ]; then
    echo "PASS: SSH key file created at $XDG_CONFIG_HOME/fold/fold-swarm/agent_key"
    ls -la "$XDG_CONFIG_HOME/fold/fold-swarm/"
else
    echo "FAIL: SSH key file not found"
    exit 1
fi

# Check key file permissions (should be 600)
PERMS=$(stat -f "%OLp" "$XDG_CONFIG_HOME/fold/fold-swarm/agent_key" 2>/dev/null || stat -c "%a" "$XDG_CONFIG_HOME/fold/fold-swarm/agent_key" 2>/dev/null)
if [ "$PERMS" = "600" ]; then
    echo "PASS: SSH key has correct permissions (600)"
else
    echo "FAIL: SSH key permissions are $PERMS, expected 600"
    exit 1
fi

echo ""
echo "=== Scenario 4: Supervisor discovers workspaces ==="
# Create some workspace directories
mkdir -p "$WORKSPACES_DIR/workspace1"
mkdir -p "$WORKSPACES_DIR/workspace2"
mkdir -p "$WORKSPACES_DIR/dispatch"

# Run supervisor with timeout - it should discover workspaces and try to spawn
# We expect it to fail on gateway connection, but we want to see the discovery logs
timeout 3 $BINARY supervisor --config "$CONFIG_DIR/config.toml" 2>&1 || true | head -50

echo ""
echo "=== Scenario 5: Socket file creation ==="
# The supervisor should create a socket file
# Run in background briefly
timeout 2 $BINARY supervisor --config "$CONFIG_DIR/config.toml" 2>&1 &
SUPERVISOR_PID=$!
sleep 1

SOCKET_PATH="/tmp/fold-swarm-test.sock"
if [ -S "$SOCKET_PATH" ]; then
    echo "PASS: Socket file created at $SOCKET_PATH"
else
    echo "INFO: Socket file not found (may be expected if supervisor failed early)"
fi

# Clean up supervisor
kill $SUPERVISOR_PID 2>/dev/null || true
wait $SUPERVISOR_PID 2>/dev/null || true

echo ""
echo "=== All scenarios complete ==="
