#!/bin/bash
# ABOUTME: E2E test runner script
# ABOUTME: Builds images, starts services, runs tests, and reports results

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log() {
    echo -e "${BLUE}[E2E]${NC} $1"
}

# Detect $COMPOSE command
if $COMPOSE version &>/dev/null; then
    COMPOSE="$COMPOSE"
elif docker-compose version &>/dev/null; then
    COMPOSE="docker-compose"
else
    error "Neither '$COMPOSE' nor 'docker-compose' found"
    exit 1
fi

error() {
    echo -e "${RED}[E2E ERROR]${NC} $1"
}

success() {
    echo -e "${GREEN}[E2E]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[E2E]${NC} $1"
}

# Check for required env vars
if [ -z "$ANTHROPIC_API_KEY" ]; then
    error "ANTHROPIC_API_KEY is required for E2E tests"
    echo "Export it before running: export ANTHROPIC_API_KEY=sk-ant-..."
    exit 1
fi

# Parse arguments
REBUILD=false
KEEP_RUNNING=false
while [[ $# -gt 0 ]]; do
    case $1 in
        --rebuild)
            REBUILD=true
            shift
            ;;
        --keep-running)
            KEEP_RUNNING=true
            shift
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--rebuild] [--keep-running]"
            exit 1
            ;;
    esac
done

# Clean up on exit
cleanup() {
    if [ "$KEEP_RUNNING" = false ]; then
        log "Cleaning up..."
        $COMPOSE down -v 2>/dev/null || true
    else
        warn "Keeping services running (--keep-running)"
        echo "To stop: $COMPOSE -f $SCRIPT_DIR/docker-compose.yml down -v"
    fi
}
trap cleanup EXIT

log "═══════════════════════════════════════════════"
log "       COVEN E2E TEST SUITE"
log "═══════════════════════════════════════════════"

# Build images
if [ "$REBUILD" = true ]; then
    log "Building Docker images (forced rebuild)..."
    $COMPOSE build --no-cache
else
    log "Building Docker images..."
    $COMPOSE build
fi

# Start services
log "Starting services..."
$COMPOSE up -d gateway

# Wait for gateway
log "Waiting for gateway to be healthy..."
for i in {1..30}; do
    if $COMPOSE exec -T gateway curl -sf http://localhost:8080/health >/dev/null 2>&1; then
        success "Gateway is healthy"
        break
    fi
    if [ $i -eq 30 ]; then
        error "Gateway failed to become healthy"
        $COMPOSE logs gateway
        exit 1
    fi
    sleep 2
done

# Start agents
log "Starting agents..."
$COMPOSE up -d agent-standalone swarm-supervisor

# Wait for agents to register
log "Waiting for agents to register..."
sleep 10

# Check registered agents
AGENTS=$($COMPOSE exec -T gateway curl -s http://localhost:8080/api/agents 2>/dev/null || echo "[]")
log "Registered agents: $AGENTS"

# Run tests
log "Running E2E tests..."
$COMPOSE run --rm test-runner

# Get results
if [ -f results/e2e-results.json ]; then
    PASSED=$(jq -r '.passed' results/e2e-results.json)
    FAILED=$(jq -r '.failed' results/e2e-results.json)

    echo ""
    log "═══════════════════════════════════════════════"
    if [ "$FAILED" -eq 0 ]; then
        success "ALL TESTS PASSED: $PASSED passed, $FAILED failed"
    else
        error "SOME TESTS FAILED: $PASSED passed, $FAILED failed"
    fi
    log "═══════════════════════════════════════════════"

    exit $FAILED
else
    error "No test results found"
    exit 1
fi
