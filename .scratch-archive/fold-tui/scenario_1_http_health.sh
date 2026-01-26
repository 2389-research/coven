#!/bin/bash
# ABOUTME: Scenario 1 - Direct HTTP health check (bypass fold-client)
# ABOUTME: Tests that the gateway is reachable at the HTTP level

echo "=== Scenario 1: Direct HTTP Health Check ==="
echo "Testing: GET http://localhost:8080/health"
echo ""

response=$(curl -s -w "\nHTTP_CODE:%{http_code}" http://localhost:8080/health 2>&1)
http_code=$(echo "$response" | grep "HTTP_CODE:" | cut -d: -f2)
body=$(echo "$response" | grep -v "HTTP_CODE:")

echo "Response body: $body"
echo "HTTP code: $http_code"
echo ""

if [ "$http_code" == "200" ]; then
    echo "✅ PASS: Gateway HTTP endpoint is reachable"
    exit 0
else
    echo "❌ FAIL: Gateway HTTP endpoint not reachable (code: $http_code)"
    exit 1
fi
