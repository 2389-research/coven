#!/bin/bash
# ABOUTME: Builds CovenClientFFI.xcframework for iOS integration
# ABOUTME: Creates universal static library with Swift bindings for xtool/SwiftPM

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
OUTPUT_DIR="$ROOT_DIR/CovenClientFFI.xcframework"
BINDINGS_DIR="$ROOT_DIR/crates/coven-client/bindings"

cd "$ROOT_DIR"

# Verify required Rust targets are installed
echo "==> Checking Rust targets..."
REQUIRED_TARGETS="aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios aarch64-apple-darwin x86_64-apple-darwin"
INSTALLED_TARGETS=$(rustup target list --installed)
MISSING=""
for target in $REQUIRED_TARGETS; do
    if ! echo "$INSTALLED_TARGETS" | grep -q "$target"; then
        MISSING="$MISSING $target"
    fi
done
if [ -n "$MISSING" ]; then
    echo "ERROR: Missing Rust targets:$MISSING"
    echo "Install with: rustup target add$MISSING"
    exit 1
fi
echo "  All required targets installed"

echo "==> Building coven-client for iOS targets..."

# Build for iOS device (arm64)
echo "  -> aarch64-apple-ios (device)"
cargo build --release --package coven-client --target aarch64-apple-ios

# Build for iOS simulator (arm64)
echo "  -> aarch64-apple-ios-sim (simulator arm64)"
cargo build --release --package coven-client --target aarch64-apple-ios-sim

# Build for iOS simulator (x86_64)
echo "  -> x86_64-apple-ios (simulator x86_64)"
cargo build --release --package coven-client --target x86_64-apple-ios

echo "==> Building coven-client for macOS targets..."

# Build for macOS (arm64 - Apple Silicon)
echo "  -> aarch64-apple-darwin (macOS arm64)"
cargo build --release --package coven-client --target aarch64-apple-darwin

# Build for macOS (x86_64 - Intel)
echo "  -> x86_64-apple-darwin (macOS x86_64)"
cargo build --release --package coven-client --target x86_64-apple-darwin

echo "==> Creating universal simulator library..."
mkdir -p target/universal-sim
lipo -create \
    target/aarch64-apple-ios-sim/release/libcoven_client.a \
    target/x86_64-apple-ios/release/libcoven_client.a \
    -output target/universal-sim/libcoven_client.a

echo "==> Creating universal macOS library..."
mkdir -p target/universal-macos
lipo -create \
    target/aarch64-apple-darwin/release/libcoven_client.a \
    target/x86_64-apple-darwin/release/libcoven_client.a \
    -output target/universal-macos/libcoven_client.a

echo "==> Creating module map..."
mkdir -p "$BINDINGS_DIR"
cat > "$BINDINGS_DIR/module.modulemap" << 'EOF'
module coven_clientFFI {
    header "coven_clientFFI.h"
    export *
}
EOF

echo "==> Removing old XCFramework..."
rm -rf "$OUTPUT_DIR"

echo "==> Creating XCFramework..."
xcodebuild -create-xcframework \
    -library target/aarch64-apple-ios/release/libcoven_client.a \
    -headers "$BINDINGS_DIR" \
    -library target/universal-sim/libcoven_client.a \
    -headers "$BINDINGS_DIR" \
    -library target/universal-macos/libcoven_client.a \
    -headers "$BINDINGS_DIR" \
    -output "$OUTPUT_DIR"

echo "==> XCFramework created at: $OUTPUT_DIR"
echo ""
echo "Contents:"
find "$OUTPUT_DIR" -type f -name "*.a" -o -name "*.h" -o -name "*.modulemap" | head -20

echo ""
echo "Done! Add this to your Package.swift:"
echo ""
echo '    .binaryTarget('
echo '        name: "CovenClientFFI",'
echo '        path: "../coven/CovenClientFFI.xcframework"'
echo '    )'
