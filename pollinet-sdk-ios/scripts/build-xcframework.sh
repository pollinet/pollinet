#!/usr/bin/env bash
# build-xcframework.sh
# Compiles the Rust core for all iOS targets and assembles an XCFramework.
#
# Usage:
#   ./scripts/build-xcframework.sh
#
# Output:
#   PolliNetRust.xcframework/   — ready to be consumed by Package.swift

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PACKAGE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RUST_DIR="$(cd "$PACKAGE_DIR/.." && pwd)"
BUILD_DIR="$RUST_DIR/target"
OUT_DIR="$PACKAGE_DIR/PolliNetRust.xcframework"
HEADER="$PACKAGE_DIR/Sources/PolliNetFFI/include/pollinet_sdk.h"
LIB_NAME="libpollinet.a"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  PolliNet iOS XCFramework build"
echo "  Rust root : $RUST_DIR"
echo "  Output    : $OUT_DIR"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# ── 1. Build all three targets ────────────────────────────────────────────────

cd "$RUST_DIR"

echo ""
echo "▶ Building aarch64-apple-ios (physical device)..."
cargo build --target aarch64-apple-ios --release --features ios

echo ""
echo "▶ Building aarch64-apple-ios-sim (Apple Silicon simulator)..."
cargo build --target aarch64-apple-ios-sim --release --features ios

echo ""
echo "▶ Building x86_64-apple-ios (Intel simulator)..."
cargo build --target x86_64-apple-ios --release --features ios

# ── 2. Create fat library for simulator (lipo arm64-sim + x86_64) ─────────────

SIM_LIPO_DIR="$BUILD_DIR/ios-sim-universal/release"
mkdir -p "$SIM_LIPO_DIR"

echo ""
echo "▶ Creating universal simulator library (lipo)..."
lipo -create \
    "$BUILD_DIR/aarch64-apple-ios-sim/release/$LIB_NAME" \
    "$BUILD_DIR/x86_64-apple-ios/release/$LIB_NAME" \
    -output "$SIM_LIPO_DIR/$LIB_NAME"

# ── 3. Assemble XCFramework ───────────────────────────────────────────────────

echo ""
echo "▶ Assembling XCFramework..."
rm -rf "$OUT_DIR"

xcodebuild -create-xcframework \
    -library "$BUILD_DIR/aarch64-apple-ios/release/$LIB_NAME" \
    -headers "$(dirname "$HEADER")" \
    -library "$SIM_LIPO_DIR/$LIB_NAME" \
    -headers "$(dirname "$HEADER")" \
    -output "$OUT_DIR"

echo ""
echo "✅ Done! XCFramework written to:"
echo "   $OUT_DIR"
echo ""
echo "Contents:"
find "$OUT_DIR" -type f | sort
