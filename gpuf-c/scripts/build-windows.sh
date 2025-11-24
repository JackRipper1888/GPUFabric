#!/bin/bash
set -e

# Get the directory where this script is located
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
# Calculate Workspace Root (assuming script is in gpuf-c/scripts/)
WORKSPACE_ROOT="$SCRIPT_DIR/../.."

echo "=== Windows Cross-Compilation Script (Linux) ==="
echo "Script Dir: $SCRIPT_DIR"
echo "Workspace Root: $WORKSPACE_ROOT"

# Check for MinGW
if ! command -v x86_64-w64-mingw32-gcc &> /dev/null; then
    echo "Error: MinGW-w64 toolchain not found."
    echo "Please install it using: sudo apt-get install mingw-w64"
    exit 1
fi

# Add target
echo "[1/2] Adding Windows target..."
rustup target add x86_64-pc-windows-gnu

# Force use of our Wrapper Script to filter out bad flags
# This wrapper calls x86_64-w64-mingw32-gcc under the hood
export CC_x86_64_pc_windows_gnu="$SCRIPT_DIR/gcc-wrapper.sh"
export CXX_x86_64_pc_windows_gnu="$SCRIPT_DIR/g++-wrapper.sh"
export AR_x86_64_pc_windows_gnu="x86_64-w64-mingw32-ar"

# Configure Flags
# We define _WIN32_WINNT to expose Windows 10 APIs (which should include THREAD_POWER_THROTTLING_STATE)
# We also re-enable -include mingw_fix.h because MinGW headers might still lack the struct despite the macro
export CFLAGS_x86_64_pc_windows_gnu="-D_WIN32_WINNT=0x0A00 -DWINVER=0x0A00 -I$SCRIPT_DIR -include mingw_fix.h"
export CXXFLAGS_x86_64_pc_windows_gnu="-D_WIN32_WINNT=0x0A00 -DWINVER=0x0A00 -I$SCRIPT_DIR -include mingw_fix.h"

# Linker Flags: Statically link C++ runtime (needed for llama.cpp)
# We explicitly link stdc++, gcc, gcc_eh (exception handling), and pthread
export RUSTFLAGS="-C link-arg=-static -C link-arg=-lstdc++ -C link-arg=-lgcc -C link-arg=-lgcc_eh -C link-arg=-lpthread"

# Generic env vars fallback
export CC="$SCRIPT_DIR/gcc-wrapper.sh"
export CXX="$SCRIPT_DIR/g++-wrapper.sh"

# Build
echo "[2/2] Building for Windows (x86_64) with MinGW Wrapper..."
# We disable default features (which might include nvml linking) and enable cpu/vulkan if needed.
# According to WINDOWS_BUILD.md, --no-default-features is the quick solution.

# Move to workspace root to run cargo
cd "$WORKSPACE_ROOT"

cargo build --release \
    --target x86_64-pc-windows-gnu \
    --bin gpuf-c \
    --features vulkan

echo ""
echo "=== Build Successful! ==="
echo "Binary located at: target/x86_64-pc-windows-gnu/release/gpuf-c.exe"
