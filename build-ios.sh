#!/bin/bash
# Build script for iOS static library

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}Building PolliNet for iOS...${NC}"

# Check if rustup is installed
if ! command -v rustup &> /dev/null; then
    echo -e "${RED}Error: rustup is not installed. Please install it from https://rustup.rs/${NC}"
    exit 1
fi

# Add iOS targets if not already added
echo -e "${YELLOW}Adding iOS targets...${NC}"
rustup target add aarch64-apple-ios        # iOS device (arm64)
rustup target add x86_64-apple-ios         # iOS simulator (Intel)
rustup target add aarch64-apple-ios-sim    # iOS simulator (Apple Silicon)

# Create output directory
OUTPUT_DIR="target/ios"
mkdir -p "$OUTPUT_DIR"

# Clean previous builds
echo -e "${YELLOW}Cleaning previous builds...${NC}"
cargo clean

# Critical: Unset Homebrew OpenSSL paths to force vendored build
unset OPENSSL_DIR
unset OPENSSL_LIB_DIR
unset OPENSSL_INCLUDE_DIR
unset OPENSSL_STATIC
unset PKG_CONFIG_PATH
unset PKG_CONFIG_ALLOW_CROSS

# Build for iOS device (arm64)
echo -e "${YELLOW}Building for iOS device (arm64)...${NC}"
cargo build --release \
    --target aarch64-apple-ios \
    --features ios \
    --no-default-features

# Build for iOS simulator (Intel)
echo -e "${YELLOW}Building for iOS simulator (x86_64)...${NC}"
cargo build --release \
    --target x86_64-apple-ios \
    --features ios \
    --no-default-features

# Build for iOS simulator (Apple Silicon)
echo -e "${YELLOW}Building for iOS simulator (aarch64)...${NC}"
cargo build --release \
    --target aarch64-apple-ios-sim \
    --features ios \
    --no-default-features

# Create universal binary for simulator (combines Intel and Apple Silicon)
echo -e "${YELLOW}Creating universal simulator library...${NC}"
lipo -create \
    target/x86_64-apple-ios/release/libpollinet.a \
    target/aarch64-apple-ios-sim/release/libpollinet.a \
    -output "$OUTPUT_DIR/libpollinet_sim.a"

# Copy device library
cp target/aarch64-apple-ios/release/libpollinet.a "$OUTPUT_DIR/libpollinet_device.a"

echo -e "${GREEN}✅ Build complete!${NC}"
echo -e "${GREEN}Device library: $OUTPUT_DIR/libpollinet_device.a${NC}"
echo -e "${GREEN}Simulator library: $OUTPUT_DIR/libpollinet_sim.a${NC}"
echo ""
echo -e "${YELLOW}Next steps:${NC}"
echo "1. Add these libraries to your Xcode project"
echo "2. Link against the appropriate library based on build target"
echo "3. Add the header files to your project"