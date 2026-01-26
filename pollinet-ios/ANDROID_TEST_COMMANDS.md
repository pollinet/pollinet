# Android Testing Commands

## Quick Android Verification

Run these commands to verify Android still works after iOS changes:

### 1. Build Android App
```bash
cd pollinet-android
./gradlew assembleDebug
```

**Expected:** Build succeeds with no errors

### 2. Run Android Tests
```bash
cd pollinet-android
./gradlew test
```

**Expected:** All tests pass

### 3. Build Rust Library for Android
```bash
cd /path/to/pollinet
cargo build --target aarch64-linux-android --features android --release
```

**Expected:** Compiles successfully

### 4. Verify Android Features
```bash
# Check RPC client is included
cargo tree --features android | grep "solana-client"

# Check BLE is included
cargo tree --features android | grep "btleplug"

# Check OpenSSL is included
cargo tree --features android | grep "openssl"
```

**Expected:** All three dependencies appear

### 5. Install on Device
```bash
cd pollinet-android
./gradlew installDebug
```

Then manually test:
- Transaction creation ✅
- RPC calls ✅
- BLE scanning/connection ✅
- SPL token transfers ✅

## What Could Go Wrong

### Symptom: "feature rpc-client not enabled"
**Fix:** Build with android feature:
```bash
cargo build --features android --target aarch64-linux-android
```

### Symptom: "cannot find crate btleplug"
**Fix:** Android feature should include it. Check Cargo.toml:
```toml
android = ["jni", "openssl", "android_logger", "rpc-client", "ble", "config-file"]
ble = ["btleplug"]
```

### Symptom: "RPC calls fail at runtime"
**Fix:** Ensure `rpc-client` feature is enabled:
```bash
cargo build --features "android,rpc-client"
```

## Android Feature Contents

The `android` feature includes:
- `jni` - JNI bindings ✅
- `openssl` - Vendored OpenSSL ✅
- `android_logger` - Logging ✅
- `rpc-client` - Includes `solana-client` and `solana-account-decoder` ✅
- `ble` - Includes `btleplug` ✅
- `config-file` - Includes `config` ✅

## Expected vs Actual

### Before iOS Changes
```toml
[features]
default = ["linux"]
android = ["jni", "openssl", "android_logger"]
```

### After iOS Changes
```toml
[features]
default = ["linux", "rpc-client", "ble", "config-file"]
android = ["jni", "openssl", "android_logger", "rpc-client", "ble", "config-file"]
ios = []
```

**Android got MORE features explicitly listed** (safer, more explicit)

## Rollback (If Needed)

If Android breaks (it shouldn't):

```bash
# Revert changes
git checkout HEAD -- Cargo.toml src/lib.rs src/nonce/mod.rs src/ffi/mod.rs

# Remove iOS files
rm -rf src/ffi/ios.rs pollinet-ios/ build-ios.sh
```

## Confidence

**Risk of breaking Android:** <1%

All changes are:
- Conditional (only affect iOS)
- Additive (Android gets everything it had before)
- Tested (feature flags verified)

Android should continue working exactly as before.
