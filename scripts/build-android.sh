#!/bin/bash
# ABOUTME: Builds coven-client native libraries for Android integration
# ABOUTME: Creates JNI libraries and Kotlin bindings for all Android ABIs

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
OUTPUT_DIR="$ROOT_DIR/coven-client-android"
BINDINGS_DIR="$ROOT_DIR/crates/coven-client/bindings"
JNILIBS_DIR="$OUTPUT_DIR/jniLibs"

cd "$ROOT_DIR"

# Android target to ABI mapping
declare -A TARGET_TO_ABI=(
    ["aarch64-linux-android"]="arm64-v8a"
    ["armv7-linux-androideabi"]="armeabi-v7a"
    ["x86_64-linux-android"]="x86_64"
    ["i686-linux-android"]="x86"
)

# Minimum API level (Android 7.0 Nougat = API 24, widely supported)
MIN_SDK_VERSION="${MIN_SDK_VERSION:-24}"

echo "==> Checking prerequisites..."

# Check for cargo-ndk
if ! command -v cargo-ndk &> /dev/null; then
    echo "ERROR: cargo-ndk not found"
    echo "Install with: cargo install cargo-ndk"
    exit 1
fi
echo "  cargo-ndk: OK"

# Check for Android NDK
if [ -z "$ANDROID_NDK_HOME" ]; then
    # Try common locations
    if [ -d "$HOME/Library/Android/sdk/ndk" ]; then
        # Find latest NDK version
        ANDROID_NDK_HOME=$(find "$HOME/Library/Android/sdk/ndk" -maxdepth 1 -type d | sort -V | tail -1)
    elif [ -d "$ANDROID_HOME/ndk" ]; then
        ANDROID_NDK_HOME=$(find "$ANDROID_HOME/ndk" -maxdepth 1 -type d | sort -V | tail -1)
    elif [ -d "/usr/local/lib/android/sdk/ndk" ]; then
        # GitHub Actions location
        ANDROID_NDK_HOME=$(find "/usr/local/lib/android/sdk/ndk" -maxdepth 1 -type d | sort -V | tail -1)
    fi
fi

if [ -z "$ANDROID_NDK_HOME" ] || [ ! -d "$ANDROID_NDK_HOME" ]; then
    echo "ERROR: Android NDK not found"
    echo "Set ANDROID_NDK_HOME or install via Android Studio"
    exit 1
fi
export ANDROID_NDK_HOME
echo "  Android NDK: $ANDROID_NDK_HOME"

# Verify Rust targets are installed
echo "==> Checking Rust targets..."
REQUIRED_TARGETS="${!TARGET_TO_ABI[@]}"
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

# Clean output directory
echo "==> Preparing output directory..."
rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

# Build for each Android target
echo "==> Building coven-client for Android targets..."
for target in "${!TARGET_TO_ABI[@]}"; do
    abi="${TARGET_TO_ABI[$target]}"
    echo "  -> $target ($abi)"

    cargo ndk \
        --target "$target" \
        --platform "$MIN_SDK_VERSION" \
        build --release --package coven-client

    # Copy to jniLibs structure
    mkdir -p "$JNILIBS_DIR/$abi"
    cp "target/$target/release/libcoven_client.so" "$JNILIBS_DIR/$abi/"
done

# Generate Kotlin bindings
echo "==> Generating Kotlin bindings..."
KOTLIN_DIR="$OUTPUT_DIR/kotlin"
mkdir -p "$KOTLIN_DIR"

# Use any of the built libraries (they all have the same interface)
# Pick arm64-v8a as the reference
cargo run --bin uniffi-bindgen -- generate \
    --library "target/aarch64-linux-android/release/libcoven_client.so" \
    --language kotlin \
    --out-dir "$KOTLIN_DIR"

echo "==> Build complete!"
echo ""
echo "Output structure:"
find "$OUTPUT_DIR" -type f | head -20
echo ""
echo "Contents:"
echo "  $JNILIBS_DIR/ - Native libraries for each ABI"
echo "  $KOTLIN_DIR/  - Kotlin bindings"
echo ""
echo "Integration:"
echo "  1. Copy jniLibs/ to your Android project's app/src/main/"
echo "  2. Copy kotlin/ sources to your project"
echo "  3. Add uniffi-runtime dependency to build.gradle:"
echo "     implementation 'net.java.dev.jna:jna:5.13.0@aar'"
