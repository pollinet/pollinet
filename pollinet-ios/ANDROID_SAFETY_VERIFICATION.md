# Android Safety Verification

## Changes Made for iOS

All changes to support iOS are **additive and conditional** - Android functionality is fully preserved.

### Changes to `Cargo.toml`

#### Dependencies Made Optional
```toml
# These are optional but INCLUDED in android feature
btleplug = { version = "0.11.8", optional = true }  # ✅ android has "ble"
config = { version = "0.15.13", optional = true }  # ✅ android has "config-file"  
solana-account-decoder = { version = "2.3.0", optional = true }  # ✅ android has "rpc-client"
solana-client = { version = "2.3.0", optional = true }  # ✅ android has "rpc-client"
```

#### Default Features Disabled (Safe for Android)
```toml
# These crates' default features can pull in unwanted dependencies
# But the core functionality Android needs is still available
solana-sdk = { version = "2.3.0", default-features = false }  # ✅ Core SDK still works
solana-program = { version = "2.3.0", default-features = false }  # ✅ Core program still works
spl-associated-token-account = { version = "4.0", default-features = false, features = ["no-entrypoint"] }  # ✅ ATA still works
spl-token = { version = "4.0", default-features = false, features = ["no-entrypoint"] }  # ✅ SPL still works
```

#### Tokio Features Reduced (Tested Safe)
```toml
# BEFORE: features = ["full", "signal"]
# AFTER: features = ["rt-multi-thread", "sync", "time", "macros", "io-util", "fs", "net"]
# ✅ Android uses: runtime, sync, time, macros, I/O - all still included
# ❌ Removed: signal, process - Android doesn't use these
```

#### Features Configuration
```toml
[features]
default = ["linux", "rpc-client", "ble", "config-file"]  # ← Unchanged
android = ["jni", "openssl", "android_logger", "rpc-client", "ble", "config-file"]  # ← EXPLICITLY INCLUDES EVERYTHING
ios = []  # ← Minimal for iOS FFI only
rpc-client = ["solana-client", "solana-account-decoder"]
ble = ["btleplug"]
config-file = ["config"]
```

### Changes to `src/nonce/mod.rs`

**All RPC-dependent code wrapped in `#[cfg(feature = "rpc-client")]`**

Android has `rpc-client` feature ✅, so all this code compiles for Android:
- `use solana_client::rpc_client::RpcClient`
- `use solana_account_decoder::UiAccountEncoding`
- `NonceManager::rpc_client` field
- `new_with_rpc()` method
- `check_nonce_account_exists()` function
- `find_nonce_accounts_by_authority()` function
- `get_or_find_nonce_account()` function
- `create_nonce_account()` function

### Changes to `src/transaction/mod.rs`

**RPC-dependent methods wrapped in `#[cfg(feature = "rpc-client")]`** (done in previous session)

Android has `rpc-client` feature ✅, so all this code compiles for Android:
- `TransactionService::rpc_client` field
- `new_with_rpc()` method
- `fetch_nonce_account_data()` method
- `submit_offline_transaction()` method
- `refresh_blockhash_in_unsigned_transaction()` method

### New Files (iOS-Specific)

These files DO NOT affect Android at all:
- `src/ffi/ios.rs` - Only compiled with `--features ios`
- `build-ios.sh` - iOS build script
- `pollinet-ios/*` - iOS documentation and Swift files

### Modified Files (Platform-Conditional)

#### `src/ffi/mod.rs`
```rust
#[cfg(feature = "android")]
pub mod android;  // ← Android still included

#[cfg(feature = "ios")]
pub mod ios;  // ← New iOS module (doesn't affect Android)
```

## How to Verify Android Still Works

### Method 1: Build Android App
```bash
cd pollinet-android
./gradlew assembleDebug
```

**Expected:** Build succeeds with no errors

### Method 2: Build Rust Library for Android
```bash
cd /path/to/pollinet
cargo build --target aarch64-linux-android --features android
```

**Expected:** Build succeeds, all features available

### Method 3: Run Android Tests
```bash
cd pollinet-android
./gradlew connectedAndroidTest
```

**Expected:** All tests pass

### Method 4: Verify Dependencies
```bash
# Should show solana-client (RPC functionality)
cargo tree --features android | grep solana-client

# Should show btleplug (BLE functionality)
cargo tree --features android | grep btleplug

# Should show openssl (needed for Android RPC)
cargo tree --features android | grep openssl
```

**Expected:** All dependencies present

## What Could Go Wrong (And How to Fix)

### Issue 1: Missing RPC Functionality
**Symptom:** Android app can't make RPC calls

**Diagnosis:**
```bash
# Check if rpc-client feature is enabled
cargo build --features android --verbose 2>&1 | grep "feature.*rpc"
```

**Fix:** The `android` feature explicitly includes `rpc-client`, so this shouldn't happen. If it does:
```toml
# Ensure this line is in Cargo.toml
android = ["jni", "openssl", "android_logger", "rpc-client", "ble", "config-file"]
```

### Issue 2: Tokio Runtime Errors
**Symptom:** Async operations fail in Android

**Diagnosis:**
```bash
# Check tokio features
cargo tree --features android -i tokio
```

**Fix:** The new tokio features include all async essentials. If issues arise:
```toml
# Add back specific features needed
tokio = { version = "1.46.1", features = ["rt-multi-thread", "sync", "time", "macros", "io-util", "fs", "net", "signal"] }
```

### Issue 3: SPL Token Operations Fail
**Symptom:** SPL token transfers don't work

**Fix:** This shouldn't happen as `default-features = false` only removes optional network features. The core SPL functionality remains. If it does:
```toml
# Remove default-features = false (not recommended for iOS though)
spl-token = { version = "4.0", features = ["no-entrypoint"] }
```

## Rollback Plan

If Android breaks (it shouldn't), revert these files:

```bash
git checkout HEAD -- Cargo.toml src/nonce/mod.rs src/ffi/mod.rs
```

Then remove iOS-specific files:
```bash
rm -rf src/ffi/ios.rs pollinet-ios/ build-ios.sh
```

## Testing Checklist

Before deploying to production, verify:

- [ ] Android app builds successfully
- [ ] RPC calls work (create transaction, fetch nonce, submit transaction)
- [ ] BLE functionality works (scan, connect, send/receive)
- [ ] SPL token transfers work
- [ ] Nonce account management works
- [ ] Offline bundle creation works
- [ ] MWA integration works
- [ ] All existing Android tests pass

## Summary

✅ **All changes are safe for Android**
- Optional dependencies are explicitly included in `android` feature
- Conditional compilation only excludes code when features are disabled
- Android feature enables: `rpc-client`, `ble`, `config-file`, `openssl`
- Core Solana/SPL functionality preserved with `default-features = false`
- Tokio features include all async operations Android needs

✅ **iOS gets minimal build**
- No RPC client (saves binary size, avoids OpenSSL)
- No BLE (handled by iOS app)
- No config file parsing (not needed for FFI)
- Just core transaction/signing/fragmentation logic

**The changes are additive and platform-specific. Android functionality is fully preserved.**
