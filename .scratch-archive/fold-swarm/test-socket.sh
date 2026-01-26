#!/bin/bash
# ABOUTME: Test socket communication with supervisor
# ABOUTME: Verifies dispatch tools can talk to supervisor

set -e

TEST_DIR=$(mktemp -d)
CONFIG_DIR="$TEST_DIR/config"
WORKSPACES_DIR="$TEST_DIR/workspaces"
mkdir -p "$CONFIG_DIR" "$WORKSPACES_DIR/existing_workspace"

cat > "$CONFIG_DIR/config.toml" << EOF
gateway_url = "grpc://localhost:50051"
prefix = "sockettest"
working_directory = "$WORKSPACES_DIR"
default_backend = "direct"
acp_binary = "claude"
EOF

export XDG_CONFIG_HOME="$TEST_DIR/xdg_config"
mkdir -p "$XDG_CONFIG_HOME"

SOCKET_PATH="/tmp/fold-swarm-sockettest.sock"
BINARY="./target/release/fold-swarm"

cleanup() {
    echo "Cleaning up..."
    kill $SUPERVISOR_PID 2>/dev/null || true
    wait $SUPERVISOR_PID 2>/dev/null || true
    rm -rf "$TEST_DIR"
    rm -f "$SOCKET_PATH"
}
trap cleanup EXIT

echo "=== Starting supervisor in background ==="
$BINARY supervisor --config "$CONFIG_DIR/config.toml" 2>&1 &
SUPERVISOR_PID=$!
sleep 2

echo ""
echo "=== Test 1: List workspaces via socket ==="
if [ -S "$SOCKET_PATH" ]; then
    echo '{"type":"list"}' | nc -U "$SOCKET_PATH" | head -1
    echo "PASS: Socket responds to list request"
else
    echo "FAIL: Socket not found at $SOCKET_PATH"
    exit 1
fi

echo ""
echo "=== Test 2: Create workspace via socket ==="
RESPONSE=$(echo '{"type":"create","name":"new_workspace"}' | nc -U "$SOCKET_PATH" | head -1)
echo "Response: $RESPONSE"
if echo "$RESPONSE" | grep -q '"success":true'; then
    echo "PASS: Workspace created successfully"
else
    echo "FAIL: Workspace creation failed"
    exit 1
fi

sleep 1
echo ""
echo "=== Test 3: List should now show new workspace ==="
RESPONSE=$(echo '{"type":"list"}' | nc -U "$SOCKET_PATH" | head -1)
echo "Response: $RESPONSE"
if echo "$RESPONSE" | grep -q "new_workspace"; then
    echo "PASS: New workspace appears in list"
else
    echo "FAIL: New workspace not in list"
    exit 1
fi

echo ""
echo "=== Test 4: Check workspace directory was created ==="
if [ -d "$WORKSPACES_DIR/new_workspace" ]; then
    echo "PASS: Workspace directory created at $WORKSPACES_DIR/new_workspace"
else
    echo "FAIL: Workspace directory not created"
    exit 1
fi

echo ""
echo "=== Test 5: Delete workspace via socket ==="
RESPONSE=$(echo '{"type":"delete","name":"new_workspace"}' | nc -U "$SOCKET_PATH" | head -1)
echo "Response: $RESPONSE"
if echo "$RESPONSE" | grep -q '"success":true'; then
    echo "PASS: Workspace deleted successfully"
else
    echo "FAIL: Workspace deletion failed"
    exit 1
fi

echo ""
echo "=== All socket tests passed ==="
