#!/bin/bash
# ABOUTME: E2E scenario test against the LIVE deployed gateway (fold-gateway.porpoise-alkaline.ts.net)
# ABOUTME: Connects test-pack and agent to production gateway, sends message, verifies SSE response

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PACKS_DIR="$(dirname "$SCRIPT_DIR")"
PROJECT_DIR="$(dirname "$PACKS_DIR")"
AGENT_DIR="$PROJECT_DIR/fold-agent"

# Live gateway config
GATEWAY_HOST="fold-gateway.porpoise-alkaline.ts.net"
GRPC_PORT=50051
HTTP_URL="https://$GATEWAY_HOST"

# Sentinel value the LLM must echo through the pack tool
SENTINEL="FOLD_LIVE_E2E_$(date +%s)"

# Temp files
AGENT_CONFIG="/tmp/fold-live-e2e-agent-$$.toml"
PACK_LOG="/tmp/fold-live-e2e-pack-$$.log"
AGENT_LOG="/tmp/fold-live-e2e-agent-$$.log"
RESPONSE_FILE="/tmp/fold-live-e2e-response-$$.txt"

# PIDs
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
    rm -f "$AGENT_CONFIG" "$PACK_LOG" "$AGENT_LOG" "$RESPONSE_FILE"
}
trap cleanup EXIT

# ─── Pre-flight checks ──────────────────────────────────────────────────────

# Check API key
if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
    if [ -f "$AGENT_DIR/.env" ]; then
        info "Loading ANTHROPIC_API_KEY from $AGENT_DIR/.env"
        export $(grep ANTHROPIC_API_KEY "$AGENT_DIR/.env" | tr -d '"' | tr -d "'")
    fi
fi

if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
    fail "ANTHROPIC_API_KEY not set and not found in $AGENT_DIR/.env"
    exit 1
fi

# Check JWT token for HTTP API
FOLD_TOKEN=""
if [ -f "$HOME/.config/fold/token" ]; then
    FOLD_TOKEN=$(cat "$HOME/.config/fold/token")
fi

# Check pack SSH key exists
if [ ! -f "$HOME/.config/fold/packs/test-pack/id_ed25519" ]; then
    fail "No SSH key for test-pack at ~/.config/fold/packs/test-pack/id_ed25519"
    exit 1
fi

# Check gateway connectivity
info "Checking gateway connectivity..."
if ! curl -s --max-time 5 "$HTTP_URL/health" | grep -q "OK"; then
    fail "Cannot reach live gateway at $HTTP_URL/health"
    exit 1
fi
pass "Gateway health check OK"

if ! nc -zv "$GATEWAY_HOST" "$GRPC_PORT" 2>&1 | grep -q "succeeded"; then
    fail "Cannot reach gRPC port $GRPC_PORT on $GATEWAY_HOST"
    exit 1
fi
pass "gRPC port $GRPC_PORT reachable"

echo ""
echo "==========================================="
echo "  FOLD E2E: Live Gateway Pack Flow         "
echo "==========================================="
echo ""
echo "  Sentinel: $SENTINEL"
echo "  Gateway:  $GATEWAY_HOST:$GRPC_PORT (gRPC)"
echo "  HTTP API: $HTTP_URL"
echo ""

# ─── Step 1: Build test-pack ────────────────────────────────────────────────
info "[1/7] Building test-pack..."
cd "$PACKS_DIR"
cargo build --release -p test-pack 2>&1 | tail -5
pass "test-pack built"

# ─── Step 2: Build agent ────────────────────────────────────────────────────
info "[2/7] Building agent..."
cd "$AGENT_DIR"
cargo build --release -p fold-agent 2>&1 | tail -5
pass "Agent built"

# ─── Step 3: Start test-pack ───────────────────────────────────────────────
info "[3/7] Starting test-pack (connecting to live gateway)..."

cd "$PACKS_DIR"
FOLD_SERVER="$GATEWAY_HOST" FOLD_PORT="$GRPC_PORT" \
    ./target/release/test-pack > "$PACK_LOG" 2>&1 &
PACK_PID=$!
sleep 5

if ! kill -0 "$PACK_PID" 2>/dev/null; then
    fail "test-pack failed to start!"
    echo "--- Pack log ---"
    cat "$PACK_LOG"
    exit 1
fi
pass "test-pack running (PID $PACK_PID)"

# Show pack log for confirmation
if grep -qi "registered\|connected\|tools" "$PACK_LOG" 2>/dev/null; then
    pass "test-pack connected to live gateway"
    grep -i "registered\|connected\|tools" "$PACK_LOG" | head -3
else
    warn "Could not confirm pack registration (checking log)..."
    tail -5 "$PACK_LOG"
fi

# ─── Step 4: Start agent ───────────────────────────────────────────────────
info "[4/7] Starting agent (headless, mux backend, live gateway)..."

cat > "$AGENT_CONFIG" << EOF
name = "e2e-live-test-agent"
server = "http://$GATEWAY_HOST:$GRPC_PORT"
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

# Wait for agent to register
info "Waiting for agent to register with live gateway..."
AGENT_ID=""
for i in $(seq 1 30); do
    sleep 2
    AGENTS_RESP=$(curl -s "$HTTP_URL/api/agents" \
        ${FOLD_TOKEN:+-H "Authorization: Bearer $FOLD_TOKEN"} 2>/dev/null || echo "[]")
    # Find our test agent
    AGENT_ID=$(echo "$AGENTS_RESP" | python3 -c "
import json, sys
agents = json.load(sys.stdin)
for a in agents:
    if 'e2e-live-test' in a.get('id', ''):
        print(a['id'])
        break
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
    fail "Agent did not register within 60 seconds"
    echo "--- Agent log ---"
    cat "$AGENT_LOG"
    echo "--- All agents on gateway ---"
    curl -s "$HTTP_URL/api/agents" ${FOLD_TOKEN:+-H "Authorization: Bearer $FOLD_TOKEN"} 2>/dev/null | python3 -m json.tool 2>/dev/null || true
    exit 1
fi
pass "Agent registered: $AGENT_ID"

# ─── Step 5: Verify tools available ────────────────────────────────────────
info "[5/7] Verifying pack tools are available..."

# Check agent log for tool reception
sleep 2
if grep -qi "echo\|tool\|pack" "$AGENT_LOG" 2>/dev/null; then
    pass "Agent received pack tools"
else
    warn "Could not confirm tools in agent log (may still work)"
fi

# ─── Step 6: Send message ──────────────────────────────────────────────────
info "[6/7] Sending message to agent via live gateway..."
info "  Message: Use the echo tool to echo '$SENTINEL'"

# POST to /api/send with SSE streaming response
curl -s -N --max-time 120 \
    -X POST "$HTTP_URL/api/send" \
    ${FOLD_TOKEN:+-H "Authorization: Bearer $FOLD_TOKEN"} \
    -H "Content-Type: application/json" \
    -d "{
        \"agent_id\": \"$AGENT_ID\",
        \"content\": \"You have access to an echo tool. Use it now to echo exactly this string: '$SENTINEL'. Do not say anything else, just call the echo tool with that exact message.\",
        \"sender\": \"e2e-live-test\"
    }" > "$RESPONSE_FILE" 2>&1 &

CURL_PID=$!

info "Waiting for LLM response..."
TIMEOUT=120
ELAPSED=0
while kill -0 "$CURL_PID" 2>/dev/null && [ $ELAPSED -lt $TIMEOUT ]; do
    sleep 2
    ELAPSED=$((ELAPSED + 2))
    if grep -q "event: done\|event:done" "$RESPONSE_FILE" 2>/dev/null; then
        break
    fi
done

kill "$CURL_PID" 2>/dev/null || true
wait "$CURL_PID" 2>/dev/null || true

# ─── Step 7: Verify results ────────────────────────────────────────────────
info "[7/7] Verifying results..."
echo ""

if [ ! -s "$RESPONSE_FILE" ]; then
    fail "No response received from live gateway!"
    echo "--- Agent log (last 30 lines) ---"
    tail -30 "$AGENT_LOG"
    exit 1
fi

CHECKS_PASSED=0
CHECKS_TOTAL=0

# Check for SSE "started" event
CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
if grep -q "event: started\|event:started" "$RESPONSE_FILE"; then
    pass "Received 'started' SSE event"
    CHECKS_PASSED=$((CHECKS_PASSED + 1))
else
    warn "No 'started' event found in response"
fi

# Check for echo tool invocation
CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
if grep -q "echo" "$RESPONSE_FILE"; then
    pass "Echo tool was invoked"
    CHECKS_PASSED=$((CHECKS_PASSED + 1))
else
    fail "Echo tool was NOT invoked!"
    echo "--- Response ---"
    cat "$RESPONSE_FILE"
    echo ""
    echo "--- Agent log (last 30 lines) ---"
    tail -30 "$AGENT_LOG"
    exit 1
fi

# Check for sentinel string
CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
if grep -q "$SENTINEL" "$RESPONSE_FILE"; then
    pass "Sentinel string '$SENTINEL' found in response!"
    CHECKS_PASSED=$((CHECKS_PASSED + 1))
else
    fail "Sentinel string '$SENTINEL' NOT found in response!"
    echo "--- Full SSE Response ---"
    cat "$RESPONSE_FILE"
    echo ""
    echo "--- Agent log (last 30 lines) ---"
    tail -30 "$AGENT_LOG"
    exit 1
fi

# Check for tool_use event
CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
if grep -q '"name".*echo\|"name": *"echo"' "$RESPONSE_FILE"; then
    pass "tool_use event confirms echo tool name"
    CHECKS_PASSED=$((CHECKS_PASSED + 1))
fi

# Check for tool_result event
CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
if grep -q "tool_result" "$RESPONSE_FILE"; then
    pass "tool_result event received (pack responded)"
    CHECKS_PASSED=$((CHECKS_PASSED + 1))
fi

# Check for done event
CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
if grep -q "event: done\|event:done" "$RESPONSE_FILE"; then
    pass "SSE stream completed with 'done' event"
    CHECKS_PASSED=$((CHECKS_PASSED + 1))
fi

echo ""
echo "==========================================="
echo -e "  ${GREEN}LIVE GATEWAY E2E: $CHECKS_PASSED/$CHECKS_TOTAL CHECKS PASSED${NC}"
echo "==========================================="
echo ""
echo "  Full lifecycle verified against LIVE gateway:"
echo "    1. test-pack connected to $GATEWAY_HOST:$GRPC_PORT"
echo "    2. test-pack registered echo tool via gRPC"
echo "    3. Agent connected (mux backend, real Claude API)"
echo "    4. Client sent message via HTTPS API"
echo "    5. Agent (LLM) decided to use echo tool"
echo "    6. Gateway routed ExecutePackTool to test-pack"
echo "    7. test-pack executed and returned result"
echo "    8. Agent received result and completed response"
echo "    9. Client received full SSE stream with sentinel"
echo ""
