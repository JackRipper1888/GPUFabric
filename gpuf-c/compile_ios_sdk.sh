#!/bin/bash

set -e

echo "🍎 Compiling gpuf-c for iOS (staticlib + XCFramework)..."

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR"
WORKSPACE_ROOT="$(cd "$PROJECT_ROOT/.." && pwd)"

if [ "$(uname)" != "Darwin" ]; then
    echo "❌ iOS build requires macOS (Xcode toolchain)."
    echo "   Please run this script on a Mac with Xcode installed."
    exit 1
fi

if ! command -v rustup >/dev/null 2>&1; then
    echo "❌ rustup not found. Install Rust toolchain first."
    exit 1
fi

if ! command -v xcodebuild >/dev/null 2>&1; then
    echo "❌ xcodebuild not found. Install Xcode and run: xcode-select --install"
    exit 1
fi

BUILD_MODE="${BUILD_MODE:-release}"
FEATURES="${FEATURES:-metal}"

IOS_DEVICE_TARGET="aarch64-apple-ios"
IOS_SIM_ARM64_TARGET="aarch64-apple-ios-sim"
IOS_SIM_X64_TARGET="x86_64-apple-ios"

BUILD_DIR="$PROJECT_ROOT/build_ios"
DIST_DIR="$BUILD_DIR/dist"
INCLUDE_DIR="$DIST_DIR/include"

mkdir -p "$BUILD_DIR" "$DIST_DIR" "$INCLUDE_DIR"

if [ -f "$PROJECT_ROOT/gpuf_c_minimal.h" ]; then
    cp "$PROJECT_ROOT/gpuf_c_minimal.h" "$INCLUDE_DIR/"
fi
if [ -f "$PROJECT_ROOT/gpuf_c.h" ]; then
    cp "$PROJECT_ROOT/gpuf_c.h" "$INCLUDE_DIR/"
fi

echo "🦀 Ensuring Rust targets are installed..."
rustup target add "$IOS_DEVICE_TARGET" >/dev/null 2>&1 || true
rustup target add "$IOS_SIM_ARM64_TARGET" >/dev/null 2>&1 || true
rustup target add "$IOS_SIM_X64_TARGET" >/dev/null 2>&1 || true

echo "🔧 Building iOS device static library ($IOS_DEVICE_TARGET)..."
cd "$PROJECT_ROOT"
if [ "$BUILD_MODE" = "release" ]; then
    cargo rustc --target "$IOS_DEVICE_TARGET" --release --lib --crate-type=staticlib --features "$FEATURES"
else
    cargo rustc --target "$IOS_DEVICE_TARGET" --lib --crate-type=staticlib --features "$FEATURES"
fi

DEVICE_LIB="$WORKSPACE_ROOT/target/$IOS_DEVICE_TARGET/$BUILD_MODE/libgpuf_c.a"
if [ ! -f "$DEVICE_LIB" ]; then
    echo "❌ Device library not found: $DEVICE_LIB"
    exit 1
fi

echo "🔧 Building iOS simulator static library ($IOS_SIM_ARM64_TARGET)..."
if [ "$BUILD_MODE" = "release" ]; then
    cargo rustc --target "$IOS_SIM_ARM64_TARGET" --release --lib --crate-type=staticlib --features "$FEATURES"
else
    cargo rustc --target "$IOS_SIM_ARM64_TARGET" --lib --crate-type=staticlib --features "$FEATURES"
fi

SIM_ARM64_LIB="$WORKSPACE_ROOT/target/$IOS_SIM_ARM64_TARGET/$BUILD_MODE/libgpuf_c.a"

SIM_X64_LIB=""
if rustup target list --installed | grep -q "^$IOS_SIM_X64_TARGET$"; then
    echo "🔧 Building iOS simulator static library ($IOS_SIM_X64_TARGET)..."
    if [ "$BUILD_MODE" = "release" ]; then
        cargo rustc --target "$IOS_SIM_X64_TARGET" --release --lib --crate-type=staticlib --features "$FEATURES" || true
    else
        cargo rustc --target "$IOS_SIM_X64_TARGET" --lib --crate-type=staticlib --features "$FEATURES" || true
    fi
    CANDIDATE="$WORKSPACE_ROOT/target/$IOS_SIM_X64_TARGET/$BUILD_MODE/libgpuf_c.a"
    if [ -f "$CANDIDATE" ]; then
        SIM_X64_LIB="$CANDIDATE"
    fi
fi

if [ ! -f "$SIM_ARM64_LIB" ]; then
    echo "❌ Simulator (arm64) library not found: $SIM_ARM64_LIB"
    exit 1
fi

SIM_UNIVERSAL_LIB="$DIST_DIR/libgpuf_c_simulator.a"
if [ -n "$SIM_X64_LIB" ] && command -v lipo >/dev/null 2>&1; then
    echo "🔗 Creating universal simulator library (arm64 + x86_64)..."
    lipo -create "$SIM_ARM64_LIB" "$SIM_X64_LIB" -output "$SIM_UNIVERSAL_LIB"
else
    cp "$SIM_ARM64_LIB" "$SIM_UNIVERSAL_LIB"
fi

XCFRAMEWORK_OUT="$DIST_DIR/gpuf_c_sdk.xcframework"
rm -rf "$XCFRAMEWORK_OUT"

echo "📦 Creating XCFramework..."
xcodebuild -create-xcframework \
    -library "$DEVICE_LIB" -headers "$INCLUDE_DIR" \
    -library "$SIM_UNIVERSAL_LIB" -headers "$INCLUDE_DIR" \
    -output "$XCFRAMEWORK_OUT"

echo "✅ iOS SDK build completed!"
echo "📦 XCFramework: $XCFRAMEWORK_OUT"
echo "📁 Headers: $INCLUDE_DIR"
