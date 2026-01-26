#!/bin/bash
# ABOUTME: Scenario 3 - gRPC GetMe call (what fold-client check_health does)
# ABOUTME: Tests the actual RPC that fold-client uses for health checks

echo "=== Scenario 3: gRPC GetMe (ClientService) ==="
echo "Testing: fold.client.ClientService/GetMe on localhost:50051"
echo ""

# Check if grpcurl is installed
if ! command -v grpcurl &> /dev/null; then
    echo "❌ SKIP: grpcurl not available"
    exit 2
fi

echo "Calling GetMe (unauthenticated)..."
result=$(grpcurl -plaintext localhost:50051 fold.client.ClientService/GetMe 2>&1)
exit_code=$?

echo "$result"
echo ""

# GetMe without auth should return an error (Unauthenticated)
# But the connection itself should work
if echo "$result" | grep -q "Unauthenticated\|UNAUTHENTICATED"; then
    echo "✅ PASS: gRPC connection works (auth required as expected)"
    exit 0
elif [ $exit_code -eq 0 ]; then
    echo "✅ PASS: gRPC GetMe succeeded"
    exit 0
else
    echo "❌ FAIL: gRPC connection failed: $result"
    exit 1
fi
