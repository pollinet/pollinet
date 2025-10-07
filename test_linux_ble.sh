#!/bin/bash
# Test macOS BLE implementation

set -e

echo "🧪 Testing linux BLE Implementation"

echo "📦 Building with linux feature..."
cargo build --features linux --bin pollinet

echo "✅ Build successful!"

echo "🚀 Running PolliNet with linux BLE..."
cargo run --features linux --bin pollinet


