#!/bin/bash
# ABOUTME: End-to-end scenario test for agent identity and auto-registration
# ABOUTME: Tests real gateway + agent with SSH auth

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GATEWAY_DIR="$(dirname "$SCRIPT_DIR")"
AGENT_DIR="/Users/harper/Public/src/2389/fold-project/fold-agent"
TEST_DB="/tmp/fold-gateway-test-$$.db"
CONFIG_FILE="/tmp/fold-gateway-test-$$.yaml"
GATEWAY_PID=""
AGENT_PID=""

# Use port 50051 since agent config points there
GRPC_PORT=50051
HTTP_PORT=8051

cleanup() {
    echo "Cleaning up..."
    [ -n "$GATEWAY_PID" ] && kill $GATEWAY_PID 2>/dev/null || true
    [ -n "$AGENT_PID" ] && kill $AGENT_PID 2>/dev/null || true
    # Kill any processes on our test ports
    lsof -ti :$GRPC_PORT | xargs kill 2>/dev/null || true
    rm -f "$TEST_DB" "$CONFIG_FILE" /tmp/agent-output-*.txt /tmp/principal-*.txt /tmp/gateway-*.log
}
trap cleanup EXIT

# Kill any existing processes on test ports
echo "Ensuring test ports are free..."
lsof -ti :$GRPC_PORT | xargs kill 2>/dev/null || true
lsof -ti :$HTTP_PORT | xargs kill 2>/dev/null || true
sleep 1

# Build binaries
echo "=== Building gateway ==="
cd "$GATEWAY_DIR"
go build -o bin/fold-gateway ./cmd/fold-gateway

echo "=== Building agent ==="
cd "$AGENT_DIR"
cargo build --release -p fold-agent

echo ""
echo "=========================================="
echo "SCENARIO 1: Default mode (disabled) rejects unknown keys"
echo "=========================================="

cat > "$CONFIG_FILE" << EOF
server:
  grpc_addr: "127.0.0.1:$GRPC_PORT"
  http_addr: "127.0.0.1:$HTTP_PORT"
database:
  path: "$TEST_DB"
auth:
  jwt_secret: "test-secret-for-scenario-testing-32chars!"
  # agent_auto_registration not set = defaults to "disabled"
EOF

echo "Starting gateway with default config (auto-registration disabled)..."
cd "$GATEWAY_DIR"
FOLD_CONFIG="$CONFIG_FILE" ./bin/fold-gateway serve > /tmp/gateway-1.log 2>&1 &
GATEWAY_PID=$!
sleep 2

echo "Attempting to connect agent with SSH key (unknown to this fresh DB)..."
cd "$AGENT_DIR"
./target/release/fold-agent --headless > /tmp/agent-output-1.txt 2>&1 &
AGENT_PID=$!
sleep 3
kill $AGENT_PID 2>/dev/null || true
AGENT_PID=""

if grep -q "unknown public key" /tmp/agent-output-1.txt; then
    echo "✅ PASS: Agent correctly rejected with 'unknown public key'"
else
    echo "❌ FAIL: Expected 'unknown public key' rejection"
    echo "--- Agent output ---"
    cat /tmp/agent-output-1.txt
    echo "--- Gateway log ---"
    tail -20 /tmp/gateway-1.log
    exit 1
fi

kill $GATEWAY_PID 2>/dev/null || true
GATEWAY_PID=""
rm -f "$TEST_DB"

echo ""
echo "=========================================="
echo "SCENARIO 2: 'approved' mode auto-registers and connects"
echo "=========================================="

cat > "$CONFIG_FILE" << EOF
server:
  grpc_addr: "127.0.0.1:$GRPC_PORT"
  http_addr: "127.0.0.1:$HTTP_PORT"
database:
  path: "$TEST_DB"
auth:
  jwt_secret: "test-secret-for-scenario-testing-32chars!"
  agent_auto_registration: "approved"
EOF

echo "Starting gateway with auto-registration=approved..."
cd "$GATEWAY_DIR"
FOLD_CONFIG="$CONFIG_FILE" ./bin/fold-gateway serve > /tmp/gateway-2.log 2>&1 &
GATEWAY_PID=$!
sleep 2

echo "Connecting agent (should auto-register and connect)..."
cd "$AGENT_DIR"
./target/release/fold-agent --headless > /tmp/agent-output-2.txt 2>&1 &
AGENT_PID=$!
sleep 5
kill $AGENT_PID 2>/dev/null || true
AGENT_PID=""

if grep -q "Registered as" /tmp/agent-output-2.txt || grep -q "Instance ID:" /tmp/agent-output-2.txt; then
    echo "✅ PASS: Agent auto-registered and connected successfully"
else
    echo "❌ FAIL: Expected successful connection with Instance ID"
    echo "--- Agent output ---"
    cat /tmp/agent-output-2.txt
    echo "--- Gateway log ---"
    tail -30 /tmp/gateway-2.log
    exit 1
fi

# Check the gateway created a principal with correct display name format
echo "Checking principal was created with correct display name..."
sqlite3 "$TEST_DB" "SELECT display_name FROM principals WHERE type='agent';" | tee /tmp/principal-name.txt
if grep -q "^agent-" /tmp/principal-name.txt; then
    echo "✅ PASS: Principal created with 'agent-xxx' display name format"
else
    echo "❌ FAIL: Expected display name starting with 'agent-'"
    cat /tmp/principal-name.txt
    exit 1
fi

# Check instance ID length
INSTANCE_ID=$(grep "Instance ID:" /tmp/agent-output-2.txt | sed 's/.*Instance ID: //' | tr -d '[:space:]')
if [ -n "$INSTANCE_ID" ] && [ ${#INSTANCE_ID} -eq 12 ]; then
    echo "✅ PASS: Instance ID is 12 characters: $INSTANCE_ID"
else
    echo "❌ FAIL: Instance ID length issue: '$INSTANCE_ID' (length: ${#INSTANCE_ID})"
    exit 1
fi

kill $GATEWAY_PID 2>/dev/null || true
GATEWAY_PID=""
rm -f "$TEST_DB"

echo ""
echo "=========================================="
echo "SCENARIO 3: 'pending' mode auto-registers but rejects"
echo "=========================================="

cat > "$CONFIG_FILE" << EOF
server:
  grpc_addr: "127.0.0.1:$GRPC_PORT"
  http_addr: "127.0.0.1:$HTTP_PORT"
database:
  path: "$TEST_DB"
auth:
  jwt_secret: "test-secret-for-scenario-testing-32chars!"
  agent_auto_registration: "pending"
EOF

echo "Starting gateway with auto-registration=pending..."
cd "$GATEWAY_DIR"
FOLD_CONFIG="$CONFIG_FILE" ./bin/fold-gateway serve > /tmp/gateway-3.log 2>&1 &
GATEWAY_PID=$!
sleep 2

echo "Connecting agent (should be registered but rejected as pending)..."
cd "$AGENT_DIR"
./target/release/fold-agent --headless > /tmp/agent-output-3.txt 2>&1 &
AGENT_PID=$!
sleep 3
kill $AGENT_PID 2>/dev/null || true
AGENT_PID=""

if grep -q "pending" /tmp/agent-output-3.txt; then
    echo "✅ PASS: Agent rejected with pending status message"
else
    echo "⚠️  Checking if principal was created with pending status..."
fi

# Verify principal was created with pending status
echo "Checking principal status in database..."
sqlite3 "$TEST_DB" "SELECT status FROM principals WHERE type='agent';" | tee /tmp/principal-status.txt
if grep -q "pending" /tmp/principal-status.txt; then
    echo "✅ PASS: Principal created with 'pending' status"
else
    echo "❌ FAIL: Expected pending status in database"
    echo "--- Database contents ---"
    sqlite3 "$TEST_DB" "SELECT * FROM principals;"
    echo "--- Gateway log ---"
    tail -30 /tmp/gateway-3.log
    exit 1
fi

kill $GATEWAY_PID 2>/dev/null || true
GATEWAY_PID=""

echo ""
echo "=========================================="
echo "ALL SCENARIOS PASSED ✅"
echo "=========================================="
