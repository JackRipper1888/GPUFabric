#!/bin/bash
set -e

# Get the directory where this script is located
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
# Calculate Workspace Root (assuming script is in gpuf-c/scripts/)
WORKSPACE_ROOT="$SCRIPT_DIR/../.."

echo "=== Android Build Script (Linux) ==="
echo "Script Dir: $SCRIPT_DIR"
echo "Workspace Root: $WORKSPACE_ROOT"

# Configuration
NDK_VERSION="26.1.10909125" # Adjust based on your installed NDK version
# Try to find NDK in standard locations if ANDROID_NDK_HOME is not set
if [ -z "$ANDROID_NDK_HOME" ]; then
    if [ -d "$HOME/Android/Sdk/ndk" ]; then
        # Find the latest version
        LATEST_NDK=$(ls -1 "$HOME/Android/Sdk/ndk" | sort -V | tail -n 1)
        export ANDROID_NDK_HOME="$HOME/Android/Sdk/ndk/$LATEST_NDK"
    elif [ -d "/usr/lib/android-sdk/ndk" ]; then
        export ANDROID_NDK_HOME="/usr/lib/android-sdk/ndk"
    fi
fi

echo "=== Android Build Script (Linux) ==="
echo "NDK Path: $ANDROID_NDK_HOME"

# Fix for crates like aws-lc-sys that expect ANDROID_NDK_ROOT or specific CMake settings
export ANDROID_NDK_ROOT="$ANDROID_NDK_HOME"
export CMAKE_GENERATOR="Ninja"

if [ -z "$ANDROID_NDK_HOME" ] || [ ! -d "$ANDROID_NDK_HOME" ]; then
    echo "Error: ANDROID_NDK_HOME not set or invalid."
    echo "Please install Android NDK and set ANDROID_NDK_HOME environment variable."
    exit 1
fi

# 1. Install dependencies
echo "[1/3] Checking dependencies..."
if ! command -v cargo-ndk &> /dev/null; then
    echo "Installing cargo-ndk..."
    cargo install cargo-ndk
fi

# 2. Add targets
echo "[2/3] Adding Rust targets..."
rustup target add aarch64-linux-android \
    armv7-linux-androideabi \
    x86_64-linux-android

# 3. Build
echo "[3/3] Building for Android..."
# Build for all standard architectures
# Using --no-default-features to avoid desktop-specific dependencies if needed,
# but gpuf-c seems to handle it via target_os cfg.

# Move to workspace root
cd "$WORKSPACE_ROOT"

# Build the library (cdylib/.so) for Android JNI
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build --release --lib --features android

echo ""
echo "=== Build Successful! ==="
echo "Libraries/Binaries are located in:"
echo "  - target/aarch64-linux-android/release/"
echo "  - target/armv7-linux-androideabi/release/"
echo "  - target/x86_64-linux-android/release/"
