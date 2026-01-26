# iOS Build Fix Summary

## Problem
The iOS build was failing with OpenSSL compilation errors because several Solana crates transitively depend on `openssl-sys`, which cannot be easily compiled for iOS targets.

## Root Cause
The following dependencies were pulling in OpenSSL:
1. **`solana-client`** - Explicitly uses `reqwest` which needs OpenSSL
2. **`solana-account-decoder`** - Used by nonce management, pulls in RPC-related dependencies
3. **`solana-sdk`** - Some features enable network dependencies

## Solution Applied

### 1. Made Dependencies Optional in `Cargo.toml`
```toml
solana-account-decoder = { version = "2.3.0", optional = true }
solana-client = { version = "2.3.0", optional = true }
solana-sdk = { version = "2.3.0", default-features = false }
solana-program = { version = "2.3.0", default-features = false }
```

### 2. Updated Feature Flags
```toml
[features]
default = ["linux", "rpc-client"]
android = ["jni", "openssl", "android_logger", "rpc-client"]
ios = []  # iOS has NO rpc-client
rpc-client = ["solana-client", "solana-account-decoder"]
```

### 3. Conditional Compilation in `src/nonce/mod.rs`
- Made `RpcClient` imports conditional: `#[cfg(feature = "rpc-client")]`
- Made `NonceManager.rpc_client` field conditional
- Made all RPC-dependent functions conditional:
  - `new_with_rpc()`
  - `check_nonce_account_exists()`
  - `find_nonce_accounts_by_authority()`
  - `get_or_find_nonce_account()`
  - `create_nonce_account()`

### 4. Conditional Compilation in `src/transaction/mod.rs`
- Made `TransactionService.rpc_client` field conditional
- Made RPC-dependent methods conditional or return errors:
  - `new_with_rpc()`
  - `fetch_nonce_account_data()`
  - `discover_and_cache_nonce_accounts_by_authority()`
  - `submit_offline_transaction()`
  - `refresh_blockhash_in_unsigned_transaction()`

### 5. Updated Build Script (`build-ios.sh`)
```bash
cargo build --release --target aarch64-apple-ios --no-default-features --features ios
```

## Impact on iOS FFI

### Functions That Work Without RPC
✅ All transaction building functions
✅ All signature operations
✅ All fragmentation functions
✅ All queue management functions
✅ All health monitoring functions
✅ Basic offline bundle creation (with pre-fetched nonces)

### Functions That Return Errors Without RPC
❌ `pollinet_prepare_offline_bundle()` - Needs RPC to fetch nonces
❌ `pollinet_cache_nonce_accounts()` - Needs RPC to fetch nonces
❌ `pollinet_refresh_offline_bundle()` - Needs RPC to refresh nonces
❌ `pollinet_refresh_blockhash_in_unsigned_transaction()` - Needs RPC
❌ `pollinet_submit_offline_transaction()` - Needs RPC

## Recommended iOS Architecture

### For iOS Apps Using PolliNet:
1. **Use Native iOS Networking** for all RPC calls (URLSession, etc.)
2. **Pre-fetch nonce data** in Swift and pass to FFI
3. **Use FFI for:**
   - Transaction building
   - Signature operations
   - Fragmentation
   - Queue management
   - BLE mesh operations

### Example: Creating Offline Transactions on iOS

Instead of calling `pollinet_prepare_offline_bundle()` (which needs RPC):

```swift
// 1. Fetch nonces from RPC using native iOS networking
let nonces = await fetchNoncesFromRPC(authority: authorityPubkey)

// 2. Build offline transaction with pre-fetched nonce
let result = try PolliNetSDK.shared.createUnsignedOfflineTransaction(
    sender: senderPubkey,
    receiver: receiverPubkey,
    amount: amount,
    nonceAccount: nonces[0].account,
    nonceValue: nonces[0].value
)

// 3. Sign and submit using native iOS RPC client
```

## Build Status
- ⏳ **Currently testing:** iOS build with new configuration
- 📝 **Expected outcome:** Clean build without OpenSSL errors
- ✅ **All FFI functions:** Implemented (55 core + 1 helper)

## Next Steps
1. Verify iOS build succeeds
2. Test Swift wrapper with sample transactions
3. Document iOS-specific RPC handling patterns
4. Create example iOS app demonstrating the architecture
