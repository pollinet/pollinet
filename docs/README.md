# PolliNet Android

Android implementation of PolliNet - decentralized Solana transaction propagation over BLE mesh networks.

## Architecture

This project consists of two main components:

1. **pollinet-sdk** - Android library (AAR) that wraps the Rust core via JNI
2. **app** - Demo application showcasing the SDK functionality

The Rust core (located in `../src`) handles:
- Transaction building and fragmentation
- BLE protocol and reassembly
- Offline nonce management
- Cryptographic operations

The Android layer (Kotlin) handles:
- BLE stack integration (GATT, scanning, advertising)
- Android permissions and lifecycle
- Foreground service for background operation
- UI and app-level logic

## Prerequisites

### Required Tools

1. **Android Studio** (2024.1 or later)
   - Download from https://developer.android.com/studio

2. **Rust toolchain**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

3. **Android NDK** (via Android Studio SDK Manager)
   - Open Android Studio → Settings/Preferences → Appearance & Behavior → System Settings → Android SDK
   - Switch to "SDK Tools" tab
   - Check "NDK (Side by side)" and install

4. **cargo-ndk** - Tool to build Rust for Android
   ```bash
   cargo install cargo-ndk
   ```

5. **Android targets for Rust**
   ```bash
   rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
   ```

### Environment Setup

Set the `ANDROID_NDK_ROOT` environment variable:

```bash
# macOS/Linux - add to ~/.zshrc or ~/.bashrc
export ANDROID_NDK_ROOT="$HOME/Library/Android/sdk/ndk/<version>"

# Or on macOS, if installed via Android Studio:
export ANDROID_NDK_ROOT="$HOME/Library/Android/sdk/ndk-bundle"
```

Replace `<version>` with your installed NDK version (e.g., `27.0.12077973`).

## Building

### Option 1: Build via Android Studio

1. Open `pollinet-android` directory in Android Studio
2. Wait for Gradle sync to complete
3. The Rust library will automatically build when you build the project
4. Click "Run" to install on a device/emulator

### Option 2: Build via Command Line

```bash
cd pollinet-android

# Build the Rust library for all ABIs
./gradlew :pollinet-sdk:buildRustLib

# Build the SDK AAR
./gradlew :pollinet-sdk:assembleRelease

# Build and install the demo app
./gradlew :app:installDebug
```

## Project Structure

```
pollinet-android/
├── app/                           # Demo application
│   └── src/main/
│       ├── AndroidManifest.xml
│       └── java/xyz/pollinet/android/
│           ├── MainActivity.kt
│           └── ui/
│               └── DiagnosticsScreen.kt
│
├── pollinet-sdk/                  # Android SDK (AAR)
│   ├── build.gradle.kts          # Cargo-ndk integration
│   └── src/main/
│       ├── AndroidManifest.xml   # BLE permissions
│       ├── java/xyz/pollinet/sdk/
│       │   ├── PolliNetFFI.kt    # JNI bindings
│       │   ├── PolliNetSDK.kt    # High-level Kotlin API
│       │   └── BleService.kt     # Foreground service
│       └── jniLibs/              # Native libraries (generated)
│           ├── arm64-v8a/
│           ├── armeabi-v7a/
│           └── x86_64/
│
└── ../src/                        # Rust core (in parent directory)
    ├── lib.rs
    ├── ffi/
    │   ├── mod.rs
    │   ├── types.rs              # FFI data types
    │   ├── runtime.rs            # Async runtime
    │   ├── transport.rs          # Host-driven BLE
    │   └── android.rs            # JNI interface
    └── ...
```

## Running the Demo

1. **Enable Bluetooth** on your Android device
2. **Grant permissions** when prompted:
   - Bluetooth Scan
   - Bluetooth Connect
   - Bluetooth Advertise (Android 12+)
   - Location (Android 10-11)
3. **Use the Diagnostics Screen**:
   - "Start Advertise" - Make this device discoverable
   - "Start Scan" - Scan for other PolliNet devices
   - Monitor connection status and metrics in real-time

## Development Workflow

### Making Changes to Rust Code

After modifying Rust code in `../src/`:

```bash
cd pollinet-android
./gradlew :pollinet-sdk:buildRustLib
```

Or simply rebuild in Android Studio - Gradle will automatically rebuild Rust.

### Testing on Device

```bash
# Run on connected device/emulator
./gradlew :app:installDebug

# View logs
adb logcat | grep "pollinet"
```

### Debugging

1. **Rust logs**: Use `tracing::info!()` / `tracing::error!()` - visible in logcat
2. **Kotlin logs**: Standard Android Studio debugger
3. **JNI errors**: Check logcat for `JNI WARNING` or `FATAL EXCEPTION`

## Troubleshooting

### Build Issues

**Problem**: `cargo-ndk: command not found`
```bash
cargo install cargo-ndk
```

**Problem**: `ANDROID_NDK_ROOT not set`
```bash
export ANDROID_NDK_ROOT="$HOME/Library/Android/sdk/ndk/<version>"
```

**Problem**: Rust target not found
```bash
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
```

### Runtime Issues

**Problem**: App crashes on startup with `UnsatisfiedLinkError`
- Ensure Rust library was built successfully
- Check that `libpollinet.so` exists in `pollinet-sdk/src/main/jniLibs/<abi>/`
- Try cleaning and rebuilding: `./gradlew clean :pollinet-sdk:buildRustLib`

**Problem**: Permissions not granted
- Ensure manifest permissions are correct
- Request runtime permissions on Android 6.0+
- For Android 12+, use `BLUETOOTH_SCAN` without location

**Problem**: Service not starting in background
- Foreground service is required on Android 8.0+
- Ensure notification channel is created
- Check battery optimization settings

## Implementation Status

✅ **Completed:**
- Project structure and cargo-ndk integration
- Host-driven BLE transport in Rust
- FFI facade with JNI bindings
- Android BLE Service with GATT plumbing
- GATT-to-FFI bridge
- Fragmentation APIs
- Basic diagnostics UI

🚧 **Pending:**
- Transaction builders (SOL/SPL/Vote)
- Signature helpers
- Solana Mobile Wallet Adapter integration
- Android Keystore fallback signer
- Comprehensive testing
- CI/CD pipeline

## Next Steps

1. **Implement transaction builders** (M4)
   - Integrate existing Rust transaction API via FFI
   - Add UI screens for composing transactions

2. **Add signing support** (M5, M10, M11)
   - Integrate Solana Mobile Wallet Adapter
   - Implement Android Keystore fallback
   - Add signature UI flow

3. **Testing** (M13)
   - Unit tests for Rust FFI layer
   - Android instrumented tests
   - End-to-end testing on real devices

4. **Production readiness**
   - Error handling and retry logic
   - Battery optimization
   - Memory management
   - Release signing

## Contributing

See the main project README and TODO.md for contribution guidelines.

## License

[Your license here]

