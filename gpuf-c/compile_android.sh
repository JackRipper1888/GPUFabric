#!/bin/bash

# Android NDK compilation script for android_test with callback support
set -e

echo "🔥 Compiling android_test for Android (with callback support)..."

# NDK paths
NDK_PATH="/home/jack/android-ndk-r27d"
if [ ! -d "$NDK_PATH" ]; then
    NDK_PATH="/home/jack/Android/Sdk/ndk/25.1.8937393"
fi

if [ ! -d "$NDK_PATH" ]; then
    echo "❌ Android NDK not found!"
    exit 1
fi

echo "📱 Using NDK: $NDK_PATH"

# Create build directory
mkdir -p build_android
cd build_android

# Copy minimal header
cp ../gpuf_c_minimal.h .

echo "🔧 Compiling android_test.c..."
# Compile using clang directly
$NDK_PATH/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android21-clang \
    -I.. \
    -I$NDK_PATH/sysroot/usr/include \
    -L.. \
    -lgpuf_c_sdk_v9 \
    -llog \
    -landroid \
    -pie \
    -o android_test ../examples/android_test.c

if [ $? -eq 0 ]; then
    echo "✅ Compilation completed!"
    echo "📦 Binary: build_android/android_test"
    
    # Check if adb is available
    if command -v adb &> /dev/null; then
        echo ""
        echo "📱 Deploying to Android device..."
        
        # Check if device is connected
        if adb devices | grep -q "device$"; then
            # Push binary
            echo "   Pushing android_test..."
            adb push android_test /data/local/tmp/
            
            # Push library
            echo "   Pushing libgpuf_c_sdk_v9.so..."
            adb push ../libgpuf_c_sdk_v9.so /data/local/tmp/
            
            # Set permissions
            echo "   Setting permissions..."
            adb shell "chmod 755 /data/local/tmp/android_test"
            adb shell "chmod 644 /data/local/tmp/libgpuf_c_sdk_v9.so"
            
            echo ""
            echo "✅ Deployment completed!"
            echo ""
            echo "🚀 To run the test:"
            echo "   adb shell \"cd /data/local/tmp && LD_LIBRARY_PATH=/data/local/tmp ./android_test\""
            echo ""
            echo "📊 Expected callback output:"
            echo "   📢 [CALLBACK] STARTING - Initializing background tasks..."
            echo "   📢 [CALLBACK] HEARTBEAT - Sending heartbeat to server"
            echo "   📢 [CALLBACK] HANDLER_START - Handler thread started"
            echo "   📢 [CALLBACK] LOGIN_SUCCESS - Login successful"
            echo "   📢 [CALLBACK] COMMAND_RECEIVED - V1(InferenceTask {...})"
            echo "   📢 [CALLBACK] INFERENCE_START - Task: xxx-xxx-xxx"
            echo "   📢 [CALLBACK] INFERENCE_SUCCESS - Task: xxx-xxx-xxx in XXXms"
            echo ""
        else
            echo ""
            echo "⚠️  No Android device connected"
            echo "   Connect device and run: adb push build_android/android_test /data/local/tmp/"
            echo "                           adb push libgpuf_c_sdk_v9.so /data/local/tmp/"
        fi
    else
        echo ""
        echo "⚠️  adb not found in PATH"
        echo "   Manual deployment required:"
        echo "   1. adb push build_android/android_test /data/local/tmp/"
        echo "   2. adb push libgpuf_c_sdk_v9.so /data/local/tmp/"
        echo "   3. adb shell \"cd /data/local/tmp && LD_LIBRARY_PATH=/data/local/tmp ./android_test\""
    fi
else
    echo "❌ Compilation failed!"
    exit 1
fi
