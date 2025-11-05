# PolliNet Android - Quick Start

## What Was Just Built

A complete Android implementation foundation for PolliNet including:

✅ **Rust FFI Layer**
- Host-driven BLE transport with push/pull interface
- JSON-based FFI protocol with versioning
- JNI bindings for Android
- Async runtime management

✅ **Android SDK**
- `pollinet-sdk` AAR library with Kotlin API
- BLE Service with GATT client/server
- Automatic fragmentation and reassembly
- Real-time metrics collection

✅ **Demo App**
- Modern Compose UI with diagnostics
- BLE scanning and advertising controls
- Live connection and metrics display
- Permission handling

## Building the Project

### 1. Install Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add Android targets
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android

# Install cargo-ndk
cargo install cargo-ndk
```

### 2. Set Environment Variables

```bash
# Add to ~/.zshrc or ~/.bashrc
export ANDROID_HOME="$HOME/Library/Android/sdk"  # Adjust for your OS
export ANDROID_NDK_ROOT="$ANDROID_HOME/ndk/27.0.12077973"  # Use your NDK version
export PATH="$PATH:$ANDROID_HOME/platform-tools:$HOME/.cargo/bin"
```

### 3. Build and Run

```bash
cd pollinet-android

# First time - build Rust library
./gradlew :pollinet-sdk:buildRustLib

# Build and install app
./gradlew :app:installDebug
```

### 4. Or Open in Android Studio

1. Open `pollinet-android/` directory
2. Wait for Gradle sync
3. Click **Run** (Rust will build automatically)

## Project Structure

```
pollinet/
├── src/                           # Rust core
│   └── ffi/                       # NEW: FFI layer for Android
│       ├── types.rs              # FFI data types (JSON v1)
│       ├── runtime.rs            # Async runtime management
│       ├── transport.rs          # Host-driven BLE transport
│       └── android.rs            # JNI interface
│
└── pollinet-android/              # NEW: Android implementation
    ├── pollinet-sdk/              # Library module (AAR)
    │   ├── src/main/
    │   │   ├── java/xyz/pollinet/sdk/
    │   │   │   ├── PolliNetFFI.kt       # JNI bindings
    │   │   │   ├── PolliNetSDK.kt       # High-level Kotlin API
    │   │   │   └── BleService.kt        # Foreground BLE service
    │   │   └── jniLibs/                 # Native .so files (generated)
    │   └── build.gradle.kts             # Cargo-ndk integration
    │
    └── app/                              # Demo app
        └── src/main/
            └── java/xyz/pollinet/android/
                ├── MainActivity.kt
                └── ui/DiagnosticsScreen.kt
```

## Testing the App

### On a Physical Device (Recommended)

1. Enable **Developer Options** and **USB Debugging**
2. Connect device via USB
3. Run: `./gradlew :app:installDebug`
4. Launch the app
5. Grant Bluetooth permissions when prompted
6. Try the controls:
   - **Start Advertise** - Make device discoverable
   - **Start Scan** - Find other PolliNet devices
   - Watch metrics update in real-time

### On Two Devices (Full Test)

1. Install app on Device A and Device B
2. Device A: Click **"Start Advertise"**
3. Device B: Click **"Start Scan"**
4. Devices should auto-connect
5. Check metrics for connection status

## What Works Now

| Feature | Status | Notes |
|---------|--------|-------|
| Rust FFI compilation | ✅ | Builds for all Android ABIs |
| JNI bindings | ✅ | Kotlin ↔ Rust communication |
| BLE Service | ✅ | Foreground service with GATT |
| Scanning/Advertising | ✅ | Device discovery |
| GATT Server | ✅ | TX/RX characteristics |
| Fragment/Reassemble | ✅ | Data packetization |
| Metrics | ✅ | Real-time diagnostics |
| Permissions | ✅ | Runtime permission handling |
| UI | ✅ | Modern Compose diagnostics |

## What's Next (To-Do)

| Feature | Priority | Estimated Time |
|---------|----------|----------------|
| Transaction builders | High | 4-6 hours |
| Signature helpers | High | 3-4 hours |
| Solana Mobile Wallet Adapter | Medium | 8-12 hours |
| Android Keystore signer | Medium | 6-8 hours |
| Comprehensive testing | High | 8-10 hours |
| CI/CD pipeline | Low | 2-3 hours |

See [ANDROID_IMPLEMENTATION_SUMMARY.md](ANDROID_IMPLEMENTATION_SUMMARY.md) for complete details.

## Troubleshooting

### "cargo-ndk: command not found"

```bash
cargo install cargo-ndk
export PATH="$HOME/.cargo/bin:$PATH"
```

### "ANDROID_NDK_ROOT not set"

```bash
# Find your NDK
ls $ANDROID_HOME/ndk

# Set the variable
export ANDROID_NDK_ROOT="$ANDROID_HOME/ndk/<your-version>"
```

### "UnsatisfiedLinkError: libpollinet.so not found"

```bash
# Rebuild Rust library
cd pollinet-android
./gradlew :pollinet-sdk:buildRustLib

# Verify .so files exist
ls pollinet-sdk/src/main/jniLibs/*/libpollinet.so
```

### App Crashes on Launch

Check logcat:
```bash
adb logcat | grep -E "(pollinet|AndroidRuntime)"
```

## Next Steps for Development

### Immediate (Complete M4)

1. Implement transaction builders in `src/ffi/android.rs`:
   ```rust
   // Wire up existing TransactionService
   pub extern "C" fn Java_xyz_pollinet_sdk_PolliNetFFI_createUnsignedTransaction(...)
   ```

2. Add Kotlin wrappers in `PolliNetSDK.kt`

3. Create transaction composition UI screen

### Short-term (M5, M10, M11)

1. Add signature helpers to FFI
2. Integrate Solana Mobile Wallet Adapter
3. Implement Android Keystore fallback

### Medium-term (M13-M15)

1. Write comprehensive tests
2. Set up CI/CD
3. Performance optimization
4. Production hardening

## Documentation Files

- **README.md** - Architecture and build instructions
- **SETUP.md** - Detailed environment setup
- **ANDROID_IMPLEMENTATION_SUMMARY.md** - Complete implementation status
- **QUICKSTART.md** (this file) - Get started quickly

## Getting Help

1. Check [SETUP.md](pollinet-android/SETUP.md) for detailed instructions
2. See [ANDROID_IMPLEMENTATION_SUMMARY.md](ANDROID_IMPLEMENTATION_SUMMARY.md) for architecture
3. Review TODO.md for overall project roadmap
4. Android Studio build output for errors

## Success!

If you can:
- ✅ Build the project without errors
- ✅ Install on a device
- ✅ See the diagnostics UI
- ✅ Start scanning/advertising
- ✅ View metrics updates

Then the foundation is working! Ready to implement remaining features.

---

**Current Phase:** Foundation Complete  
**Next Milestone:** Transaction Builders (M4)  
**Estimated to Full Demo:** 30-45 hours

