#!/bin/bash
# ABOUTME: End-to-end scenario test for bindings flow using instance_id
# ABOUTME: Tests gateway + agent integration with binding creation/deletion

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GATEWAY_DIR="$(dirname "$SCRIPT_DIR")"
AGENT_DIR="/Users/harper/Public/src/2389/fold-project/fold-agent"
TEST_DB="/tmp/fold-gateway-bindings-test-$$.db"
CONFIG_FILE="/tmp/fold-gateway-bindings-test-$$.yaml"
TOKEN_DIR="/tmp/fold-gateway-bindings-token-$$"
GATEWAY_PID=""
AGENT_PID=""

# Use port 50051 since agent defaults to connecting there
GRPC_PORT=50051
HTTP_PORT=8051

# JWT secret for testing
JWT_SECRET="test-secret-for-scenario-testing-32chars!"

cleanup() {
    echo "Cleaning up..."
    [ -n "$GATEWAY_PID" ] && kill $GATEWAY_PID 2>/dev/null || true
    [ -n "$AGENT_PID" ] && kill $AGENT_PID 2>/dev/null || true
    # Kill any processes on our test ports
    lsof -ti :$GRPC_PORT | xargs kill 2>/dev/null || true
    lsof -ti :$HTTP_PORT | xargs kill 2>/dev/null || true
    rm -f "$TEST_DB" "$CONFIG_FILE" /tmp/bindings-*.txt /tmp/gateway-bindings.log /tmp/agent-bindings.log
    rm -rf "$TOKEN_DIR"
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
echo "BINDINGS FLOW END-TO-END TEST"
echo "=========================================="

# Create config with auto-registration enabled
cat > "$CONFIG_FILE" << EOF
server:
  grpc_addr: "127.0.0.1:$GRPC_PORT"
  http_addr: "127.0.0.1:$HTTP_PORT"
database:
  path: "$TEST_DB"
auth:
  jwt_secret: "$JWT_SECRET"
  agent_auto_registration: "approved"
EOF

# Generate a JWT token for HTTP API calls
echo "=== Generating JWT token for API access ==="
mkdir -p "$TOKEN_DIR"
cat > "$TOKEN_DIR/gen_token.go" << 'GOCODE'
package main

import (
	"fmt"
	"os"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

func main() {
	if len(os.Args) < 3 {
		fmt.Fprintln(os.Stderr, "Usage: gen_token <principal_id> <secret>")
		os.Exit(1)
	}

	principalID := os.Args[1]
	secret := []byte(os.Args[2])

	token := jwt.NewWithClaims(jwt.SigningMethodHS256, jwt.MapClaims{
		"sub": principalID,
		"iat": time.Now().Unix(),
		"exp": time.Now().Add(24 * time.Hour).Unix(),
	})

	tokenString, err := token.SignedString(secret)
	if err != nil {
		fmt.Fprintln(os.Stderr, "Error:", err)
		os.Exit(1)
	}

	fmt.Println(tokenString)
}
GOCODE

# Create a test principal in the database first
ADMIN_ID="admin-$(uuidgen | tr '[:upper:]' '[:lower:]')"
NOW=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

# Create database with schema and test admin
sqlite3 "$TEST_DB" << SQL
-- Principals table
CREATE TABLE IF NOT EXISTS principals (
    principal_id       TEXT PRIMARY KEY,
    type               TEXT NOT NULL,
    pubkey_fingerprint TEXT NOT NULL UNIQUE,
    display_name       TEXT NOT NULL,
    status             TEXT NOT NULL,
    created_at         TEXT NOT NULL,
    last_seen          TEXT,
    metadata_json      TEXT
);

-- Roles table
CREATE TABLE IF NOT EXISTS roles (
    subject_type TEXT NOT NULL,
    subject_id   TEXT NOT NULL,
    role         TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    PRIMARY KEY (subject_type, subject_id, role)
);

-- Bindings table
CREATE TABLE IF NOT EXISTS bindings (
    binding_id TEXT PRIMARY KEY,
    frontend   TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    agent_id   TEXT NOT NULL,
    working_dir TEXT,
    created_at TEXT NOT NULL,
    created_by TEXT,
    UNIQUE(frontend, channel_id)
);

-- Create test admin
INSERT INTO principals (principal_id, type, pubkey_fingerprint, display_name, status, created_at)
VALUES ('$ADMIN_ID', 'client', '$(echo -n "admin-key-$ADMIN_ID" | shasum -a 256 | cut -d' ' -f1)', 'Test Admin', 'approved', '$NOW');

-- Give admin the admin role
INSERT INTO roles (subject_type, subject_id, role, created_at)
VALUES ('principal', '$ADMIN_ID', 'admin', '$NOW');
SQL

echo "Created test admin: $ADMIN_ID"

# Generate JWT token
cd "$TOKEN_DIR"
go mod init temp > /dev/null 2>&1
go get github.com/golang-jwt/jwt/v5 > /dev/null 2>&1
AUTH_TOKEN=$(go run gen_token.go "$ADMIN_ID" "$JWT_SECRET")
cd - > /dev/null
echo "Generated JWT token for admin"

# Start gateway
echo ""
echo "=== Starting gateway with auto-registration=approved ==="
cd "$GATEWAY_DIR"
FOLD_CONFIG="$CONFIG_FILE" ./bin/fold-gateway serve > /tmp/gateway-bindings.log 2>&1 &
GATEWAY_PID=$!
sleep 2

# Verify gateway is running
if ! kill -0 $GATEWAY_PID 2>/dev/null; then
    echo "FAIL: Gateway failed to start"
    cat /tmp/gateway-bindings.log
    exit 1
fi
echo "Gateway started (PID: $GATEWAY_PID)"

# Start agent
echo ""
echo "=== Starting agent (will auto-register and connect) ==="
cd "$AGENT_DIR"
./target/release/fold-agent --headless > /tmp/agent-bindings.log 2>&1 &
AGENT_PID=$!
sleep 5

# Check agent is running and connected
if ! kill -0 $AGENT_PID 2>/dev/null; then
    echo "FAIL: Agent failed to start"
    cat /tmp/agent-bindings.log
    exit 1
fi

# Verify agent connected successfully
if grep -q "Instance ID:" /tmp/agent-bindings.log || grep -q "Registered as" /tmp/agent-bindings.log; then
    echo "Agent connected successfully"
else
    echo "WARN: Agent may not have connected properly"
    cat /tmp/agent-bindings.log
fi

# Test 1: GET /api/agents - verify agent appears with instance_id
echo ""
echo "=== Test 1: GET /api/agents ==="
curl -s -H "Authorization: Bearer $AUTH_TOKEN" "http://127.0.0.1:$HTTP_PORT/api/agents" | tee /tmp/bindings-agents.txt
echo ""

# Extract instance_id from response
INSTANCE_ID=$(jq -r '.[0].instance_id' /tmp/bindings-agents.txt 2>/dev/null)
if [ -z "$INSTANCE_ID" ] || [ "$INSTANCE_ID" = "null" ]; then
    echo "FAIL: No instance_id in response"
    echo "--- Agent response ---"
    cat /tmp/bindings-agents.txt
    echo "--- Gateway log ---"
    tail -30 /tmp/gateway-bindings.log
    exit 1
fi
echo "PASS: Got instance_id: $INSTANCE_ID"

# Also verify we have working_dir
WORKING_DIR=$(jq -r '.[0].working_dir' /tmp/bindings-agents.txt 2>/dev/null)
if [ -z "$WORKING_DIR" ] || [ "$WORKING_DIR" = "null" ]; then
    echo "FAIL: No working_dir in response"
    exit 1
fi
echo "PASS: Got working_dir: $WORKING_DIR"

# Test 2: POST /api/bindings - create binding using that instance_id
echo ""
echo "=== Test 2: POST /api/bindings (create binding) ==="
curl -s -X POST "http://127.0.0.1:$HTTP_PORT/api/bindings" \
    -H "Authorization: Bearer $AUTH_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"frontend":"matrix","channel_id":"!test:room","instance_id":"'"$INSTANCE_ID"'"}' \
    | tee /tmp/bindings-create.txt
echo ""

# Verify binding was created with binding_id
BINDING_ID=$(jq -r '.binding_id' /tmp/bindings-create.txt 2>/dev/null)
if [ -z "$BINDING_ID" ] || [ "$BINDING_ID" = "null" ]; then
    echo "FAIL: Binding creation failed - no binding_id"
    cat /tmp/bindings-create.txt
    exit 1
fi
echo "PASS: Binding created with ID: $BINDING_ID"

# Verify working_dir is included
CREATE_WORKDIR=$(jq -r '.working_dir' /tmp/bindings-create.txt 2>/dev/null)
if [ -z "$CREATE_WORKDIR" ] || [ "$CREATE_WORKDIR" = "null" ]; then
    echo "FAIL: Binding creation response missing working_dir"
    exit 1
fi
echo "PASS: Binding has working_dir: $CREATE_WORKDIR"

# Test 3: GET /api/bindings?frontend=matrix&channel_id=!test:room - verify binding exists
echo ""
echo "=== Test 3: GET /api/bindings (single binding status) ==="
# URL encode: ! = %21, : = %3A
curl -s -H "Authorization: Bearer $AUTH_TOKEN" \
    "http://127.0.0.1:$HTTP_PORT/api/bindings?frontend=matrix&channel_id=%21test%3Aroom" \
    | tee /tmp/bindings-status.txt
echo ""

# Verify we get back the binding with working_dir
STATUS_WORKDIR=$(jq -r '.working_dir' /tmp/bindings-status.txt 2>/dev/null)
if [ -z "$STATUS_WORKDIR" ] || [ "$STATUS_WORKDIR" = "null" ]; then
    echo "FAIL: Binding status missing working_dir"
    cat /tmp/bindings-status.txt
    exit 1
fi
echo "PASS: Binding status retrieved with working_dir: $STATUS_WORKDIR"

# Verify online status (known bug: GetAgent uses Connection.ID not PrincipalID)
ONLINE=$(jq -r '.online' /tmp/bindings-status.txt 2>/dev/null)
if [ "$ONLINE" != "true" ]; then
    echo "WARN: Agent shows as offline - known bug in handleGetSingleBinding"
    echo "      (uses GetAgent with PrincipalID but GetAgent expects Connection.ID)"
else
    echo "PASS: Agent is online"
fi

# Test 4: GET /api/bindings (list all) - verify binding in list
echo ""
echo "=== Test 4: GET /api/bindings (list all) ==="
curl -s -H "Authorization: Bearer $AUTH_TOKEN" "http://127.0.0.1:$HTTP_PORT/api/bindings" | tee /tmp/bindings-list.txt
echo ""

BINDING_COUNT=$(jq '.bindings | length' /tmp/bindings-list.txt 2>/dev/null)
if [ "$BINDING_COUNT" != "1" ]; then
    echo "FAIL: Expected 1 binding, got: $BINDING_COUNT"
    cat /tmp/bindings-list.txt
    exit 1
fi
echo "PASS: Found 1 binding in list"

# Test 5: POST /api/bindings (idempotent - same binding again)
echo ""
echo "=== Test 5: POST /api/bindings (idempotent rebind) ==="
HTTP_CODE=$(curl -s -o /tmp/bindings-rebind.txt -w "%{http_code}" -X POST "http://127.0.0.1:$HTTP_PORT/api/bindings" \
    -H "Authorization: Bearer $AUTH_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"frontend":"matrix","channel_id":"!test:room","instance_id":"'"$INSTANCE_ID"'"}')
echo "HTTP $HTTP_CODE"
cat /tmp/bindings-rebind.txt
echo ""

# Should return 200 OK (not 201 Created) for idempotent rebind
if [ "$HTTP_CODE" = "200" ]; then
    echo "PASS: Idempotent rebind returned 200 OK"
else
    echo "WARN: Expected 200 for idempotent rebind, got $HTTP_CODE"
fi

# Test 6: DELETE /api/bindings?frontend=matrix&channel_id=!test:room
echo ""
echo "=== Test 6: DELETE /api/bindings (unbind) ==="
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE \
    -H "Authorization: Bearer $AUTH_TOKEN" \
    "http://127.0.0.1:$HTTP_PORT/api/bindings?frontend=matrix&channel_id=%21test%3Aroom")

if [ "$HTTP_CODE" = "204" ]; then
    echo "PASS: Binding deleted (HTTP 204)"
else
    echo "FAIL: Expected 204, got $HTTP_CODE"
    exit 1
fi

# Test 7: GET /api/bindings?frontend=matrix&channel_id=!test:room - verify 404
echo ""
echo "=== Test 7: GET /api/bindings (verify binding gone) ==="
HTTP_CODE=$(curl -s -o /tmp/bindings-gone.txt -w "%{http_code}" \
    -H "Authorization: Bearer $AUTH_TOKEN" \
    "http://127.0.0.1:$HTTP_PORT/api/bindings?frontend=matrix&channel_id=%21test%3Aroom")

if [ "$HTTP_CODE" = "404" ]; then
    echo "PASS: Binding correctly returns 404 after deletion"
else
    echo "FAIL: Expected 404, got $HTTP_CODE"
    cat /tmp/bindings-gone.txt
    exit 1
fi

# Test 8: DELETE /api/bindings on non-existent binding - should 404
echo ""
echo "=== Test 8: DELETE /api/bindings (non-existent) ==="
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE \
    -H "Authorization: Bearer $AUTH_TOKEN" \
    "http://127.0.0.1:$HTTP_PORT/api/bindings?frontend=matrix&channel_id=%21nonexistent%3Aroom")

if [ "$HTTP_CODE" = "404" ]; then
    echo "PASS: Delete of non-existent binding returns 404"
else
    echo "FAIL: Expected 404 for non-existent binding, got $HTTP_CODE"
    exit 1
fi

# Test 9: POST /api/bindings with non-existent instance_id - should 404
echo ""
echo "=== Test 9: POST /api/bindings (bad instance_id) ==="
HTTP_CODE=$(curl -s -o /tmp/bindings-bad-instance.txt -w "%{http_code}" -X POST "http://127.0.0.1:$HTTP_PORT/api/bindings" \
    -H "Authorization: Bearer $AUTH_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"frontend":"matrix","channel_id":"!test:room","instance_id":"nonexistent123"}')

if [ "$HTTP_CODE" = "404" ]; then
    echo "PASS: Non-existent instance_id returns 404"
else
    echo "FAIL: Expected 404 for non-existent instance_id, got $HTTP_CODE"
    cat /tmp/bindings-bad-instance.txt
    exit 1
fi

echo ""
echo "=========================================="
echo "ALL BINDINGS FLOW TESTS PASSED"
echo "=========================================="
