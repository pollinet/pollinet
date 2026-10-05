#!/usr/bin/env bash
# Build PolliNetRust.xcframework for the iOS SDK (pollinet-sdk-ios).
#
# Produces:
#   pollinet-sdk-ios/PolliNetRust.xcframework   (device arm64 + simulator arm64)
#
# Requirements: rustup targets aarch64-apple-ios + aarch64-apple-ios-sim,
# cbindgen, Xcode command-line tools.
#
# NOTE: like the committed Android .so, the xcframework lags source —
# re-run this script after ANY change to src/ffi/ (especially ios.rs/common.rs).
set -euo pipefail

cd "$(dirname "$0")/.."  # repo root (pollinet/)

OUT_DIR="pollinet-sdk-ios"
XCFRAMEWORK="$OUT_DIR/PolliNetRust.xcframework"
HEADER_STAGING="target/ios-headers"

# Match the Swift package's minimum platforms (iOS 15 / macOS 13); without this,
# rustc targets ios10 and the C deps (blake3, vendored openssl) target the SDK
# default, producing mismatched-min-version link warnings.
export IPHONEOS_DEPLOYMENT_TARGET=15.0
export MACOSX_DEPLOYMENT_TARGET=13.0

echo "==> Building Rust staticlib (device: aarch64-apple-ios)"
cargo build --release --target aarch64-apple-ios --no-default-features --features ios

echo "==> Building Rust staticlib (simulator: aarch64-apple-ios-sim)"
cargo build --release --target aarch64-apple-ios-sim --no-default-features --features ios

echo "==> Building Rust staticlib (macOS host, for swift test: aarch64-apple-darwin)"
cargo build --release --target aarch64-apple-darwin --no-default-features --features ios

echo "==> Generating C header with cbindgen"
rm -rf "$HEADER_STAGING"
mkdir -p "$HEADER_STAGING"
cbindgen --config cbindgen.toml --crate pollinet --output "$HEADER_STAGING/pollinet.h"
cat > "$HEADER_STAGING/module.modulemap" <<'EOF'
module PolliNetRust {
    header "pollinet.h"
    export *
}
EOF

echo "==> Assembling $XCFRAMEWORK"
rm -rf "$XCFRAMEWORK"
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libpollinet.a -headers "$HEADER_STAGING" \
  -library target/aarch64-apple-ios-sim/release/libpollinet.a -headers "$HEADER_STAGING" \
  -library target/aarch64-apple-darwin/release/libpollinet.a -headers "$HEADER_STAGING" \
  -output "$XCFRAMEWORK"

echo "==> Done:"
find "$XCFRAMEWORK" -maxdepth 2
