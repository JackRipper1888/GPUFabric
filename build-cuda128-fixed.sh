#!/bin/bash
# Build script using CUDA 12.8 with static linking
# Fixes: Use correct nvcc path for CUDA 12.8

set -e

# Use CUDA 12.8 explicitly
export CUDA_HOME=/usr/local/cuda-12.8
export PATH=$CUDA_HOME/bin:$PATH
export LD_LIBRARY_PATH=$CUDA_HOME/lib64:$LD_LIBRARY_PATH

# Verify
echo "=== CUDA Version Check ==="
which nvcc
nvcc --version

echo ""
echo "=== Cleaning previous builds ==="
rm -rf target/release/build/llama-cpp-sys-2-*

echo ""
echo "=== Building with CUDA 12.8 (static linking) ==="
# Set CMAKE_CUDA_COMPILER to force CMake to use correct nvcc
CC=gcc-11 \
CXX=g++-11 \
CUDAHOSTCXX=g++-11 \
CMAKE_CUDA_COMPILER=$CUDA_HOME/bin/nvcc \
CMAKE_CUDA_FLAGS="-arch=sm_80 --cudart=static -std=c++17" \
cargo build --release --bin gpuf-c --features cuda

echo ""
echo "=== Build complete ==="
ls -lh target/release/gpuf-c
