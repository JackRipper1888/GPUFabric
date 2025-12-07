#!/bin/bash
# Build script for CUDA version with workarounds

set -e

echo "🔥 Building gpuf-c with CUDA support"
echo "================================================"

# Check if CUDA is available
if ! command -v nvcc &> /dev/null; then
    echo "❌ Error: nvcc not found. Please install CUDA toolkit."
    exit 1
fi

echo "✅ CUDA toolkit found"

# Backup original Cargo.toml
echo "📝 Backing up Cargo.toml..."
cp Cargo.toml Cargo.toml.backup

# Temporarily modify Cargo.toml to remove cdylib
echo "🔧 Temporarily removing cdylib from Cargo.toml..."
sed -i 's/crate-type = \["cdylib", "staticlib", "rlib"\]/crate-type = ["rlib"]/' Cargo.toml

# Build the binary
echo ""
echo "📦 Building binary with CUDA support..."
cargo build --release --bin gpuf-c --features cuda

BUILD_STATUS=$?

# Restore original Cargo.toml
echo "♻️  Restoring original Cargo.toml..."
mv Cargo.toml.backup Cargo.toml

if [ $BUILD_STATUS -eq 0 ]; then
    echo ""
    echo "✅ Binary built successfully: ../target/release/gpuf-c"
    echo ""
    echo "Note: Due to CUDA PIC limitations, the shared library (.so) was not built."
    echo "If you need the library, use one of these alternatives:"
    echo "  1. Use --features vulkan instead (supports shared library)"
    echo "  2. Build static library: cargo rustc --release --lib --features cuda --crate-type staticlib"
    echo "  3. For Android, use the Vulkan backend"
else
    echo "❌ Build failed"
    exit 1
fi

echo ""
echo "🎯 Build complete!"
