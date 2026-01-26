#!/bin/bash
# ABOUTME: Scenario 2 - gRPC reflection test (bypass fold-client)
# ABOUTME: Tests that gRPC services are discoverable

echo "=== Scenario 2: gRPC Service Discovery ==="
echo "Testing: grpcurl list on localhost:50051"
echo ""

# Check if grpcurl is installed
if ! command -v grpcurl &> /dev/null; then
    echo "⚠️  grpcurl not installed, trying grpc_cli..."
    if ! command -v grpc_cli &> /dev/null; then
        echo "❌ SKIP: Neither grpcurl nor grpc_cli available"
        echo "   Install with: brew install grpcurl"
        exit 2
    fi
fi

echo "Listing gRPC services..."
services=$(grpcurl -plaintext localhost:50051 list 2>&1)
exit_code=$?

echo "$services"
echo ""

if [ $exit_code -eq 0 ]; then
    echo "✅ PASS: gRPC services are discoverable"
    exit 0
else
    echo "❌ FAIL: Cannot discover gRPC services"
    exit 1
fi
