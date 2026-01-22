#!/bin/bash
# Test script for JitPack build configuration

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "🧪 Testing JitPack Build Configuration"
echo "======================================"
echo ""

cd "$REPO_ROOT"

# Check if jitpack.yml exists
if [ ! -f "jitpack.yml" ]; then
    echo "❌ jitpack.yml not found in repository root!"
    exit 1
fi
echo "✅ jitpack.yml found"

# Check if pollinet-android directory exists
if [ ! -d "pollinet-android" ]; then
    echo "❌ pollinet-android directory not found!"
    exit 1
fi
echo "✅ pollinet-android directory found"

# Check if gradlew exists
if [ ! -f "pollinet-android/gradlew" ]; then
    echo "❌ gradlew not found!"
    exit 1
fi
echo "✅ gradlew found"

# Make gradlew executable
chmod +x pollinet-android/gradlew
echo "✅ gradlew is executable"

# Check Gradle configuration
echo ""
echo "🔨 Testing Gradle configuration..."
cd pollinet-android

# Check if we can at least see the tasks (dry run)
if ./gradlew :pollinet-sdk:tasks --all > /dev/null 2>&1; then
    echo "✅ Gradle configuration is valid"
else
    echo "❌ Gradle configuration has issues"
    echo "   Run './gradlew :pollinet-sdk:tasks' manually to see errors"
    exit 1
fi

# Check if Rust is available (optional, but good to know)
echo ""
echo "🔧 Checking Rust toolchain..."
if command -v cargo &> /dev/null; then
    echo "✅ Rust is installed: $(cargo --version)"
    if command -v cargo-ndk &> /dev/null; then
        echo "✅ cargo-ndk is installed"
    else
        echo "⚠️  cargo-ndk not found (may be needed for native library build)"
    fi
else
    echo "⚠️  Rust not found (JitPack will try to install it)"
fi

# Test the actual build commands (optional, can be slow)
if [ "$1" == "--full" ]; then
    echo ""
    echo "🔨 Running full build test (this may take a while)..."
    echo "   This simulates what JitPack will do"
    
    # Build the SDK
    if ./gradlew :pollinet-sdk:build -x test; then
        echo "✅ Build successful"
    else
        echo "❌ Build failed"
        exit 1
    fi
    
    # Publish to Maven Local
    if ./gradlew :pollinet-sdk:publishToMavenLocal -x test; then
        echo "✅ Maven Local publication successful"
        
        # Check if artifact exists
        VERSION=$(grep -oP 'version\s*=\s*"[^"]+"' pollinet-sdk/build.gradle.kts | head -1 | grep -oP '"[^"]+"' | tr -d '"')
        if [ -n "$VERSION" ]; then
            ARTIFACT_PATH="$HOME/.m2/repository/xyz/pollinet/pollinet-sdk/$VERSION"
            if [ -d "$ARTIFACT_PATH" ]; then
                echo "✅ Artifact published to: $ARTIFACT_PATH"
                ls -lh "$ARTIFACT_PATH" | grep -E '\.(aar|jar|pom)$' || echo "   (No AAR/JAR files found)"
            else
                echo "⚠️  Artifact path not found: $ARTIFACT_PATH"
            fi
        fi
    else
        echo "❌ Maven Local publication failed"
        exit 1
    fi
else
    echo ""
    echo "ℹ️  Skipping full build (use --full flag to test actual build)"
fi

echo ""
echo "✅ All basic checks passed!"
echo ""
echo "📋 Next steps:"
echo "   1. Commit and push jitpack.yml:"
echo "      git add jitpack.yml"
echo "      git commit -m 'Add JitPack configuration'"
echo "      git push"
echo ""
echo "   2. Create a test tag:"
echo "      git tag -a v0.1.0-test -m 'Test JitPack build'"
echo "      git push origin v0.1.0-test"
echo ""
echo "   3. Check build status at:"
echo "      https://jitpack.io/#YOUR_GITHUB_USERNAME/pollinet/v0.1.0-test"
echo "      (Replace YOUR_GITHUB_USERNAME with your actual GitHub username)"
echo ""
echo "   4. For full build test, run:"
echo "      ./scripts/test_jitpack_build.sh --full"
