#!/bin/bash
# Test macOS BLE implementation

set -e

echo "🧪 Testing macOS BLE Implementation"

echo "📦 Building with macOS feature..."
cargo build --features macos --bin pollinet

echo "✅ Build successful!"

echo "🚀 Running PolliNet with macOS BLE..."
cargo run --features macos --bin pollinet


