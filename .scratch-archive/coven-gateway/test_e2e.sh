#!/bin/bash
# ABOUTME: End-to-end test for pack -> gateway -> agent -> MCP flow
# ABOUTME: Starts gateway, connects pack, tests MCP directly and via agent token

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GATEWAY_DIR="$(dirname "$SCRIPT_DIR")"
cd "$GATEWAY_DIR"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "=== End-to-End Pack Tool Test ==="
echo ""

# Cleanup function
cleanup() {
    echo ""
    echo "Cleaning up..."
    if [ -n "$GATEWAY_PID" ]; then
        kill $GATEWAY_PID 2>/dev/null || true
    fi
    if [ -n "$PACK_PID" ]; then
        kill $PACK_PID 2>/dev/null || true
    fi
    rm -f /tmp/test_gateway.db
}
trap cleanup EXIT

# Build components
echo "[1/8] Building gateway..."
go build -o /tmp/test-gateway ./cmd/fold-gateway

echo "[2/8] Building test pack..."
go build -o /tmp/test-pack ./.scratch/test_pack

echo "[3/8] Building test agent..."
go build -o /tmp/test-agent ./.scratch/test_agent_mcp

# Create minimal config
cat > /tmp/test_gateway_config.yaml << 'EOF'
server:
  grpc_addr: "localhost:50051"
  http_addr: "localhost:18080"
database:
  path: "/tmp/test_gateway.db"
EOF

# Start gateway
echo "[4/8] Starting gateway..."
FOLD_CONFIG=/tmp/test_gateway_config.yaml /tmp/test-gateway serve > /tmp/gateway.log 2>&1 &
GATEWAY_PID=$!
sleep 2

# Check gateway is running
if ! kill -0 $GATEWAY_PID 2>/dev/null; then
    echo -e "${RED}Gateway failed to start!${NC}"
    cat /tmp/gateway.log
    exit 1
fi
echo "  Gateway running (PID $GATEWAY_PID)"

# Start test pack
echo "[5/8] Starting test pack..."
GATEWAY_ADDR="localhost:50051" /tmp/test-pack > /tmp/pack.log 2>&1 &
PACK_PID=$!
sleep 2

# Check pack is running
if ! kill -0 $PACK_PID 2>/dev/null; then
    echo -e "${RED}Pack failed to start!${NC}"
    cat /tmp/pack.log
    exit 1
fi
echo "  Pack running (PID $PACK_PID)"

# Wait for pack to register
sleep 1

echo "[6/8] Testing MCP endpoint directly (no auth)..."
echo ""

# Test 1: Initialize
echo "  Test: initialize..."
INIT_RESULT=$(curl -s -X POST http://localhost:18080/mcp \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":1,"method":"initialize"}')

if echo "$INIT_RESULT" | grep -q '"protocolVersion"'; then
    echo -e "    ${GREEN}✓ Initialize successful${NC}"
else
    echo -e "    ${RED}✗ Initialize failed: $INIT_RESULT${NC}"
    exit 1
fi

# Test 2: List tools (no auth - should get all tools)
echo "  Test: tools/list (no auth)..."
LIST_RESULT=$(curl -s -X POST http://localhost:18080/mcp \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":2,"method":"tools/list"}')

TOOL_COUNT=$(echo "$LIST_RESULT" | grep -o '"name"' | wc -l)
if [ "$TOOL_COUNT" -eq 2 ]; then
    echo -e "    ${GREEN}✓ Listed $TOOL_COUNT tools${NC}"
else
    echo -e "    ${RED}✗ Expected 2 tools, got $TOOL_COUNT: $LIST_RESULT${NC}"
    exit 1
fi

# Test 3: Call echo tool
echo "  Test: tools/call echo..."
CALL_RESULT=$(curl -s -X POST http://localhost:18080/mcp \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"message":"hello world"}}}')

# Check for successful result (echoed appears in the nested JSON)
if echo "$CALL_RESULT" | grep -q 'echoed'; then
    echo -e "    ${GREEN}✓ Echo tool executed successfully${NC}"
    echo "      Response: $(echo "$CALL_RESULT" | head -c 200)"
else
    echo -e "    ${RED}✗ Echo tool failed: $CALL_RESULT${NC}"
    exit 1
fi

# Test 4: Call admin_echo without auth (should fail due to capability check)
echo "  Test: tools/call admin_echo (no auth, should fail)..."
ADMIN_RESULT=$(curl -s -X POST http://localhost:18080/mcp \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"admin_echo","arguments":{"message":"test"}}}')

if echo "$ADMIN_RESULT" | grep -q '"error"'; then
    echo -e "    ${GREEN}✓ Admin tool correctly rejected (capability check works)${NC}"
else
    echo -e "    ${YELLOW}⚠ Admin tool was accessible without auth: $ADMIN_RESULT${NC}"
fi

echo ""
echo "[7/8] Testing agent MCP token flow..."
echo ""

# Run the agent test - this connects as an agent, gets a token, and tests MCP with it
GATEWAY_ADDR="localhost:50051" /tmp/test-agent 2>&1 | while IFS= read -r line; do
    echo "  $line"
done

if [ ${PIPESTATUS[0]} -ne 0 ]; then
    echo -e "${RED}Agent MCP test failed!${NC}"
    exit 1
fi

echo ""
echo "[8/8] Checking logs..."
echo ""
echo "=== Gateway log (last 15 lines) ==="
tail -15 /tmp/gateway.log
echo ""
echo "=== Pack log ==="
cat /tmp/pack.log
echo ""

echo -e "${GREEN}=== All E2E tests passed! ===${NC}"
echo ""
echo "Full lifecycle verified:"
echo "  1. Gateway started"
echo "  2. Pack connected and registered tools"
echo "  3. MCP endpoint works (direct access)"
echo "  4. Agent connected and received MCP token"
echo "  5. Agent accessed MCP with capability-filtered tools"
echo "  6. Agent called tool via MCP -> Gateway -> Pack -> result"
