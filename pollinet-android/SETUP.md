# PolliNet Android - Setup Guide

This guide will help you set up your development environment to build and run PolliNet on Android.

## Quick Start (if you already have the prerequisites)

```bash
cd pollinet-android
./gradlew :app:installDebug
```

## Detailed Setup Instructions

### 1. Install Rust

If you don't have Rust installed:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### 2. Install Android Targets for Rust

```bash
rustup target add aarch64-linux-android
rustup target add armv7-linux-androideabi  
rustup target add x86_64-linux-android
```

### 3. Install cargo-ndk

```bash
cargo install cargo-ndk
```

If you get a permission error, make sure `~/.cargo/bin` is in your PATH and you have write access to `~/.cargo/`.

### 4. Install Android SDK and NDK

#### Option A: Via Android Studio (Recommended)

1. Download and install Android Studio from https://developer.android.com/studio
2. Open Android Studio
3. Go to **Settings** (or **Preferences** on Mac)
4. Navigate to **Appearance & Behavior → System Settings → Android SDK**
5. Click on **SDK Tools** tab
6. Check:
   - **Android SDK Build-Tools**
   - **NDK (Side by side)** - version 27.x or later
   - **Android SDK Command-line Tools**
7. Click **Apply** and wait for installation

#### Option B: Via Command Line

```bash
# Install Android SDK command-line tools
# Download from: https://developer.android.com/studio#command-tools

# Install NDK
sdkmanager "ndk;27.0.12077973"
```

### 5. Set Environment Variables

Add these to your shell profile (`~/.zshrc`, `~/.bashrc`, or `~/.profile`):

```bash
# Android SDK location (adjust path if different)
export ANDROID_HOME="$HOME/Library/Android/sdk"  # macOS
# export ANDROID_HOME="$HOME/Android/Sdk"        # Linux
# export ANDROID_HOME="$LOCALAPPDATA/Android/Sdk" # Windows (Git Bash)

# NDK location (use your installed version)
export ANDROID_NDK_ROOT="$ANDROID_HOME/ndk/27.0.12077973"

# Add to PATH
export PATH="$PATH:$ANDROID_HOME/platform-tools:$ANDROID_HOME/tools:$HOME/.cargo/bin"
```

**Reload your shell:**
```bash
source ~/.zshrc  # or ~/.bashrc
```

### 6. Verify Installation

```bash
# Check Rust
rustc --version
cargo --version

# Check Rust Android targets
rustup target list | grep android

# Check cargo-ndk
cargo ndk --version

# Check Android SDK
adb --version

# Check NDK (adjust path)
ls $ANDROID_NDK_ROOT
```

All commands should complete successfully without errors.

### 7. Build the Project

```bash
cd pollinet-android

# Clean build (first time or after major changes)
./gradlew clean

# Build Rust library
./gradlew :pollinet-sdk:buildRustLib

# Build and install app
./gradlew :app:installDebug
```

## Troubleshooting

### Issue: `cargo-ndk: command not found`

**Solution:**
```bash
# Make sure cargo bin is in PATH
export PATH="$HOME/.cargo/bin:$PATH"

# Reinstall cargo-ndk
cargo install cargo-ndk --force
```

### Issue: `ANDROID_NDK_ROOT not set` or `NDK not found`

**Solution:**
```bash
# Find your NDK installation
ls $ANDROID_HOME/ndk

# Set the variable (replace with your version)
export ANDROID_NDK_ROOT="$ANDROID_HOME/ndk/27.0.12077973"

# Make it permanent by adding to ~/.zshrc or ~/.bashrc
```

### Issue: Rust build fails with "linker not found"

**Solution:**

Make sure you have the correct NDK version and that `ANDROID_NDK_ROOT` points to it:

```bash
# Verify NDK structure
ls $ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/*/bin/

# Should see files like:
# - aarch64-linux-android34-clang
# - armv7a-linux-androideabi34-clang
```

### Issue: Gradle build fails with "SDK location not found"

**Solution:**

Create `local.properties` file in `pollinet-android/`:

```properties
sdk.dir=/Users/YOUR_USERNAME/Library/Android/sdk
ndk.dir=/Users/YOUR_USERNAME/Library/Android/sdk/ndk/27.0.12077973
```

Replace `YOUR_USERNAME` with your actual username.

### Issue: App crashes with `UnsatisfiedLinkError: dlopen failed: library "libpollinet.so" not found`

**Solution:**

The Rust library wasn't built or wasn't included in the APK:

```bash
# Rebuild Rust library
cd pollinet-android
./gradlew :pollinet-sdk:buildRustLib

# Verify .so files exist
ls pollinet-sdk/src/main/jniLibs/arm64-v8a/libpollinet.so
ls pollinet-sdk/src/main/jniLibs/armeabi-v7a/libpollinet.so
ls pollinet-sdk/src/main/jniLibs/x86_64/libpollinet.so

# If files don't exist, check build logs for errors
./gradlew :pollinet-sdk:buildRustLib --stacktrace
```

### Issue: Permission errors when running on device

**Solution:**

1. For Android 12+, ensure you're using the new Bluetooth permissions
2. Grant permissions manually: Settings → Apps → PolliNet → Permissions
3. Check that manifest includes all required permissions
4. For Android 10-11, location permission is required for BLE scanning

### Issue: Gradle sync fails in Android Studio

**Solution:**

1. **File → Invalidate Caches / Restart**
2. Delete `.gradle` and `.idea` directories in `pollinet-android/`
3. Reimport project: **File → Open** → select `pollinet-android` directory
4. Wait for indexing and Gradle sync to complete

## Device Requirements

### Minimum Requirements
- Android 10.0 (API level 29) or higher
- Bluetooth Low Energy (BLE) support
- ARM64 or ARM architecture (x86_64 for emulator)

### Recommended
- Android 12+ for better BLE permission model
- Physical device (BLE doesn't work well in emulators)
- Two devices for end-to-end testing

## Testing the Build

### 1. Check the build outputs

```bash
# SDK AAR
ls pollinet-sdk/build/outputs/aar/

# Demo APK
ls app/build/outputs/apk/debug/
```

### 2. Install on device

```bash
# List connected devices
adb devices

# Install
./gradlew :app:installDebug

# Launch
adb shell am start -n xyz.pollinet.android/.MainActivity
```

### 3. View logs

```bash
# Filter PolliNet logs
adb logcat | grep -i pollinet

# Or view all system logs
adb logcat
```

## Next Steps

Once setup is complete:

1. ✅ Run the app and verify it launches
2. ✅ Grant Bluetooth permissions when prompted
3. ✅ Check that SDK version is displayed on the main screen
4. ✅ Try "Start Advertise" and "Start Scan" buttons
5. ✅ Monitor metrics in real-time

## Getting Help

If you encounter issues not covered here:

1. Check the main [README.md](README.md)
2. Look at the [TODO.md](../TODO.md) for implementation status
3. Review Android Studio's Build output for detailed error messages
4. Check Rust compiler errors in the Gradle build logs

## Development Tips

### Fast iteration

```bash
# Watch Rust changes and rebuild
cd ..
cargo watch -x "ndk -t arm64-v8a -o pollinet-android/pollinet-sdk/src/main/jniLibs build --release"
```

### Clean rebuild

```bash
cd pollinet-android
./gradlew clean
rm -rf pollinet-sdk/src/main/jniLibs
./gradlew :app:installDebug
```

### Debugging Rust code

Add print statements with `tracing::info!()`:

```rust
tracing::info!("Debug value: {:?}", my_variable);
```

View in logcat:
```bash
adb logcat | grep "Debug value"
```

