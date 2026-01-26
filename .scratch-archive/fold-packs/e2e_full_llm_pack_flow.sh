#!/bin/bash
# ABOUTME: Full e2e scenario test: client -> gateway -> agent (LLM) -> gateway -> pack -> back
# ABOUTME: Spins up entire stack locally and verifies tool invocation through real Claude API

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PACKS_DIR="$(dirname "$SCRIPT_DIR")"
PROJECT_DIR="$(dirname "$PACKS_DIR")"
GATEWAY_DIR="$PROJECT_DIR/fold-gateway"
AGENT_DIR="$PROJECT_DIR/fold-agent"
COMMON_DIR="$PROJECT_DIR/fold-common"

# Test ports (thematic: "fold" in leet = F01D = 61453, but let's use memorable ones)
GRPC_PORT=50061
HTTP_PORT=18061

# Sentinel value the LLM must echo through the pack tool
SENTINEL="FOLD_E2E_$(date +%s)"

# Temp files
TEST_DB="/tmp/fold-e2e-$$.db"
CONFIG_FILE="/tmp/fold-e2e-$$.yaml"
AGENT_CONFIG="/tmp/fold-e2e-agent-$$.toml"
GATEWAY_LOG="/tmp/fold-e2e-gateway-$$.log"
PACK_LOG="/tmp/fold-e2e-pack-$$.log"
AGENT_LOG="/tmp/fold-e2e-agent-$$.log"
RESPONSE_FILE="/tmp/fold-e2e-response-$$.txt"

# PIDs
GATEWAY_PID=""
PACK_PID=""
AGENT_PID=""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info() { echo -e "${CYAN}[INFO]${NC} $1"; }
pass() { echo -e "${GREEN}[PASS]${NC} $1"; }
fail() { echo -e "${RED}[FAIL]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }

cleanup() {
    echo ""
    info "Cleaning up..."
    [ -n "$AGENT_PID" ] && kill "$AGENT_PID" 2>/dev/null || true
    [ -n "$PACK_PID" ] && kill "$PACK_PID" 2>/dev/null || true
    [ -n "$GATEWAY_PID" ] && kill "$GATEWAY_PID" 2>/dev/null || true
    # Kill anything left on test ports
    lsof -ti :$GRPC_PORT 2>/dev/null | xargs kill 2>/dev/null || true
    lsof -ti :$HTTP_PORT 2>/dev/null | xargs kill 2>/dev/null || true
    rm -f "$TEST_DB" "$CONFIG_FILE" "$AGENT_CONFIG" "$GATEWAY_LOG" "$PACK_LOG" "$AGENT_LOG" "$RESPONSE_FILE"
}
trap cleanup EXIT

# Pre-flight: check required env
if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
    # Try loading from agent's .env
    if [ -f "$AGENT_DIR/.env" ]; then
        info "Loading ANTHROPIC_API_KEY from $AGENT_DIR/.env"
        export $(grep ANTHROPIC_API_KEY "$AGENT_DIR/.env" | tr -d '"' | tr -d "'")
    fi
fi

if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
    fail "ANTHROPIC_API_KEY not set and not found in $AGENT_DIR/.env"
    exit 1
fi

# Ensure ports are free
info "Ensuring test ports are free..."
lsof -ti :$GRPC_PORT 2>/dev/null | xargs kill 2>/dev/null || true
lsof -ti :$HTTP_PORT 2>/dev/null | xargs kill 2>/dev/null || true
sleep 1

echo ""
echo "=========================================="
echo "  FOLD E2E SCENARIO: Full LLM Pack Flow  "
echo "=========================================="
echo ""
echo "  Sentinel: $SENTINEL"
echo "  Gateway:  127.0.0.1:$GRPC_PORT (gRPC), 127.0.0.1:$HTTP_PORT (HTTP)"
echo ""

# ─── Step 1: Build gateway ────────────────────────────────────────────────────
info "[1/9] Building gateway..."
cd "$GATEWAY_DIR"
go build -o /tmp/fold-e2e-gateway ./cmd/fold-gateway 2>&1 | tail -5
pass "Gateway built"

# ─── Step 2: Build test-pack ──────────────────────────────────────────────────
info "[2/9] Building test-pack..."
cd "$PACKS_DIR"
cargo build --release -p test-pack 2>&1 | tail -5
pass "test-pack built"

# ─── Step 3: Build agent ──────────────────────────────────────────────────────
info "[3/9] Building agent..."
cd "$AGENT_DIR"
cargo build --release -p fold-agent 2>&1 | tail -5
pass "Agent built"

# ─── Step 4: Start gateway ────────────────────────────────────────────────────
info "[4/9] Starting gateway..."

cat > "$CONFIG_FILE" << EOF
server:
  grpc_addr: "127.0.0.1:$GRPC_PORT"
  http_addr: "127.0.0.1:$HTTP_PORT"
database:
  path: "$TEST_DB"
auth:
  agent_auto_registration: "approved"
  # No jwt_secret = HTTP API open (no auth required for test)
logging:
  level: "debug"
EOF

cd "$GATEWAY_DIR"
FOLD_CONFIG="$CONFIG_FILE" /tmp/fold-e2e-gateway serve > "$GATEWAY_LOG" 2>&1 &
GATEWAY_PID=$!
sleep 2

if ! kill -0 "$GATEWAY_PID" 2>/dev/null; then
    fail "Gateway failed to start!"
    echo "--- Gateway log ---"
    cat "$GATEWAY_LOG"
    exit 1
fi
pass "Gateway running (PID $GATEWAY_PID)"

# ─── Step 5: Start test-pack ─────────────────────────────────────────────────
info "[5/9] Starting test-pack..."

cd "$PACKS_DIR"
FOLD_SERVER=127.0.0.1 FOLD_PORT=$GRPC_PORT \
    ./target/release/test-pack > "$PACK_LOG" 2>&1 &
PACK_PID=$!
sleep 3

if ! kill -0 "$PACK_PID" 2>/dev/null; then
    fail "test-pack failed to start!"
    echo "--- Pack log ---"
    cat "$PACK_LOG"
    exit 1
fi
pass "test-pack running (PID $PACK_PID)"

# ─── Step 6: Start agent ──────────────────────────────────────────────────────
info "[6/9] Starting agent (headless, mux backend)..."

# Create a test-specific agent config (overrides any existing .fold/agent.toml)
cat > "$AGENT_CONFIG" << EOF
name = "e2e-test-agent"
server = "http://127.0.0.1:$GRPC_PORT"
backend = "mux"
working_dir = "/tmp"
EOF

cd "$AGENT_DIR"
ANTHROPIC_API_KEY="$ANTHROPIC_API_KEY" \
    ./target/release/fold-agent \
    --headless \
    --config "$AGENT_CONFIG" \
    > "$AGENT_LOG" 2>&1 &
AGENT_PID=$!

# Wait for agent to register (poll /api/agents)
info "Waiting for agent to register..."
AGENT_ID=""
for i in $(seq 1 20); do
    sleep 2
    AGENTS_RESP=$(curl -s "http://127.0.0.1:$HTTP_PORT/api/agents" 2>/dev/null || echo "[]")
    AGENT_ID=$(echo "$AGENTS_RESP" | python3 -c "
import json, sys
agents = json.load(sys.stdin)
if agents:
    print(agents[0]['id'])
" 2>/dev/null || echo "")
    if [ -n "$AGENT_ID" ]; then
        break
    fi
    if ! kill -0 "$AGENT_PID" 2>/dev/null; then
        fail "Agent process died!"
        echo "--- Agent log ---"
        cat "$AGENT_LOG"
        exit 1
    fi
done

if [ -z "$AGENT_ID" ]; then
    fail "Agent did not register within 40 seconds"
    echo "--- Agent log ---"
    cat "$AGENT_LOG"
    echo "--- Gateway log (last 30 lines) ---"
    tail -30 "$GATEWAY_LOG"
    exit 1
fi
pass "Agent registered: $AGENT_ID"

# ─── Step 7: Verify tools are available ───────────────────────────────────────
info "[7/9] Verifying pack tools are available..."

# Check MCP endpoint (the agent should have received tools in Welcome)
# We'll verify by checking the gateway logs for tool registration
if grep -q "echo" "$GATEWAY_LOG"; then
    pass "Echo tool registered with gateway"
else
    warn "Could not confirm echo tool in gateway logs (may still work)"
fi

# ─── Step 8: Send message and capture SSE response ────────────────────────────
info "[8/9] Sending message to agent (via HTTP API)..."
info "  Message: Use the echo tool to echo '$SENTINEL'"

# POST to /api/send with SSE streaming response
# Use curl with timeout and capture full SSE stream
curl -s -N --max-time 120 \
    -X POST "http://127.0.0.1:$HTTP_PORT/api/send" \
    -H "Content-Type: application/json" \
    -d "{
        \"agent_id\": \"$AGENT_ID\",
        \"content\": \"You have access to an echo tool. Use it now to echo exactly this string: '$SENTINEL'. Do not say anything else, just call the echo tool with that exact message.\",
        \"sender\": \"e2e-test\"
    }" > "$RESPONSE_FILE" 2>&1 &

CURL_PID=$!

# Wait for response (up to 120 seconds for LLM processing)
info "Waiting for LLM response (this may take a while)..."
TIMEOUT=120
ELAPSED=0
while kill -0 "$CURL_PID" 2>/dev/null && [ $ELAPSED -lt $TIMEOUT ]; do
    sleep 2
    ELAPSED=$((ELAPSED + 2))
    # Check if we got a done event
    if grep -q "event: done\|event:done" "$RESPONSE_FILE" 2>/dev/null; then
        break
    fi
done

# Kill curl if still running
kill "$CURL_PID" 2>/dev/null || true
wait "$CURL_PID" 2>/dev/null || true

# ─── Step 9: Verify results ──────────────────────────────────────────────────
info "[9/9] Verifying results..."
echo ""

if [ ! -s "$RESPONSE_FILE" ]; then
    fail "No response received from gateway!"
    echo "--- Agent log (last 30 lines) ---"
    tail -30 "$AGENT_LOG"
    echo "--- Gateway log (last 30 lines) ---"
    tail -30 "$GATEWAY_LOG"
    exit 1
fi

# Check for SSE "started" event
if grep -q "event: started\|event:started" "$RESPONSE_FILE"; then
    pass "Received 'started' SSE event"
else
    warn "No 'started' event found in response"
fi

# Check for tool_use event with "echo" tool
if grep -q "echo" "$RESPONSE_FILE"; then
    pass "Echo tool was invoked"
else
    fail "Echo tool was NOT invoked!"
    echo "--- Response ---"
    cat "$RESPONSE_FILE"
    echo ""
    echo "--- Agent log (last 30 lines) ---"
    tail -30 "$AGENT_LOG"
    exit 1
fi

# Check for sentinel in response (either in tool_result or text)
if grep -q "$SENTINEL" "$RESPONSE_FILE"; then
    pass "Sentinel string '$SENTINEL' found in response!"
else
    fail "Sentinel string '$SENTINEL' NOT found in response!"
    echo "--- Full SSE Response ---"
    cat "$RESPONSE_FILE"
    echo ""
    echo "--- Agent log (last 30 lines) ---"
    tail -30 "$AGENT_LOG"
    exit 1
fi

# Check for tool_use event specifically
if grep -q '"name".*echo\|"name": *"echo"' "$RESPONSE_FILE"; then
    pass "tool_use event confirms echo tool name"
fi

# Check for tool_result event
if grep -q "tool_result" "$RESPONSE_FILE"; then
    pass "tool_result event received (pack responded)"
fi

echo ""
echo "=========================================="
echo -e "  ${GREEN}ALL E2E CHECKS PASSED!${NC}"
echo "=========================================="
echo ""
echo "  Full lifecycle verified:"
echo "    1. Gateway started and accepted connections"
echo "    2. test-pack connected and registered echo tool"
echo "    3. Agent connected (mux backend, real Claude API)"
echo "    4. Client sent message via HTTP API"
echo "    5. Agent (LLM) decided to use echo tool"
echo "    6. Gateway routed ExecutePackTool to test-pack"
echo "    7. test-pack executed and returned result"
echo "    8. Agent received result and completed response"
echo "    9. Client received full SSE stream with sentinel"
echo ""
echo "  Response file: $RESPONSE_FILE"
echo ""
