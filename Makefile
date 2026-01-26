# ABOUTME: Build and development commands for the coven monorepo
# ABOUTME: Handles building, testing, linting, and proto generation

.PHONY: all build release test check clippy fmt clean proto install

# Default target
all: check test clippy

# Build all workspace members (debug)
build:
	cargo build --workspace

# Build all workspace members (release)
release:
	cargo build --workspace --release

# Build individual binaries
coven:
	cargo build -p coven-cli

coven-agent:
	cargo build -p coven-agent

coven-swarm:
	cargo build -p coven-swarm

coven-chat:
	cargo build -p coven-tui

# Build release binaries
coven-release:
	cargo build -p coven-cli --release

coven-agent-release:
	cargo build -p coven-agent --release

coven-swarm-release:
	cargo build -p coven-swarm --release

# Run all tests
test:
	cargo test --workspace

# Run tests with output
test-verbose:
	cargo test --workspace -- --nocapture

# Type check without building
check:
	cargo check --workspace --all-targets

# Run clippy linter
clippy:
	cargo clippy --workspace --all-targets -- -D warnings

# Format code
fmt:
	cargo fmt --all

# Check formatting without modifying
fmt-check:
	cargo fmt --all -- --check

# Clean build artifacts
clean:
	cargo clean

# Regenerate protobuf code
proto:
	cd crates/coven-proto && cargo build

# Install main CLI locally
install:
	cargo install --path crates/coven-cli

# Install all binaries locally
install-all:
	cargo install --path crates/coven-cli
	cargo install --path crates/coven-agent
	cargo install --path crates/coven-swarm
	cargo install --path crates/coven-tui

# Generate UniFFI bindings for Swift/Kotlin
bindings:
	cd crates/coven-client && cargo run --bin uniffi-bindgen generate src/coven_client.udl --language swift --out-dir bindings

# Run the CLI
run:
	cargo run -p coven-cli -- $(ARGS)

# Run the agent
run-agent:
	cargo run -p coven-agent -- $(ARGS)

# Run the swarm
run-swarm:
	cargo run -p coven-swarm -- $(ARGS)

# Run the TUI chat
run-chat:
	cargo run -p coven-tui -- $(ARGS)

# Development: watch and rebuild on changes (requires cargo-watch)
watch:
	cargo watch -x 'check --workspace'

watch-test:
	cargo watch -x 'test --workspace'

# Update dependencies
update:
	cargo update

# Check for outdated dependencies (requires cargo-outdated)
outdated:
	cargo outdated -R

# Security audit (requires cargo-audit)
audit:
	cargo audit
