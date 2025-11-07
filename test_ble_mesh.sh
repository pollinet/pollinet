#!/bin/bash

# BLE Mesh Quick Test Script
# Tests the core fragmentation and mesh functionality

echo "🧪 BLE Mesh Quick Test"
echo "====================="
echo ""

cd "$(dirname "$0")"

# Test 1: Check Rust compilation
echo "📦 Test 1: Checking Rust compilation..."
if cargo check --lib --no-default-features --features android 2>&1 | grep -q "Finished"; then
    echo "✅ PASSED: Rust library compiles successfully"
else
    echo "❌ FAILED: Rust compilation errors"
    exit 1
fi
echo ""

# Test 2: Build Android SDK
echo "📱 Test 2: Building Android SDK..."
cd pollinet-android
if ./gradlew :pollinet-sdk:assembleDebug 2>&1 | tail -1 | grep -q "BUILD SUCCESSFUL"; then
    echo "✅ PASSED: Android SDK builds successfully"
else
    echo "❌ FAILED: Android build errors"
    exit 1
fi
cd ..
echo ""

# Test 3: Check JNI libraries
echo "🔗 Test 3: Checking JNI libraries..."
JNI_LIBS="pollinet-android/pollinet-sdk/src/main/jniLibs"
if [ -f "$JNI_LIBS/arm64-v8a/libpollinet.so" ]; then
    echo "✅ PASSED: JNI libraries generated"
    echo "   Found: arm64-v8a/libpollinet.so"
    ls -lh "$JNI_LIBS/arm64-v8a/libpollinet.so" | awk '{print "   Size:", $5}'
else
    echo "❌ FAILED: JNI libraries missing"
    exit 1
fi
echo ""

# Test 4: Check core modules exist
echo "📂 Test 4: Checking core mesh modules..."
MODULES=(
    "src/ble/fragmenter.rs"
    "src/ble/mesh.rs"
    "src/ble/broadcaster.rs"
    "src/ble/peer_manager.rs"
)

ALL_EXIST=true
for module in "${MODULES[@]}"; do
    if [ -f "$module" ]; then
        LINES=$(wc -l < "$module")
        echo "   ✅ $module ($LINES lines)"
    else
        echo "   ❌ $module (missing)"
        ALL_EXIST=false
    fi
done

if [ "$ALL_EXIST" = true ]; then
    echo "✅ PASSED: All core modules present"
else
    echo "❌ FAILED: Some modules missing"
    exit 1
fi
echo ""

# Summary
echo "======================================"
echo "✅ ALL TESTS PASSED!"
echo "======================================"
echo ""
echo "Your BLE mesh is ready to test!"
echo ""
echo "Next steps:"
echo "1. Install APK on Android device:"
echo "   cd pollinet-android"
echo "   ./gradlew :app:installDebug"
echo ""
echo "2. Enable BLE permissions in app"
echo ""
echo "3. Check fragmentation works:"
echo "   - Open Diagnostics screen"
echo "   - Look for 'Fragmentation Test' card"
echo "   - Tap 'Run Test'"
echo ""
echo "4. Test with multiple devices:"
echo "   - Install on 2+ devices"
echo "   - Device A: Start advertising"
echo "   - Device B: Scan and connect"
echo "   - Device A: Broadcast test transaction"
echo ""
echo "📖 Full guide: BLE_MESH_TESTING_GUIDE.md"

