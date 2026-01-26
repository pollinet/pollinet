# Fixing OpenSSL Build Error for iOS

## Problem

The build fails with:
```
error: failed to run custom build command for `openssl-sys v0.9.109`
Could not find directory of OpenSSL installation
```

This happens because `solana-client` (which depends on OpenSSL) is being pulled in transitively, even though iOS doesn't need RPC client functionality.

## Solution Applied

1. **Made `solana-client` optional** in `Cargo.toml`
2. **Created `rpc-client` feature** that enables `solana-client`
3. **Updated build script** to use `--no-default-features` for iOS builds
4. **Added `#[cfg(feature = "rpc-client")]`** to RPC-dependent methods

## Current Status

The following methods require RPC client and are conditionally compiled:
- `prepare_offline_nonce_data()` - Only available with `rpc-client` feature
- `refresh_blockhash_in_unsigned_transaction()` - Only available with `rpc-client` feature  
- `submit_offline_transaction()` - Only available with `rpc-client` feature

## For iOS FFI

These methods are called by iOS FFI but won't work without RPC client. Options:

### Option 1: Return Errors (Current)
These methods will return `RpcClient("RPC client not initialized")` errors when called without RPC.

### Option 2: Stub Implementations (Recommended for iOS)
Add iOS-specific implementations that return appropriate errors:
```rust
#[cfg(all(feature = "ios", not(feature = "rpc-client")))]
impl TransactionService {
    pub async fn refresh_blockhash_in_unsigned_transaction(...) -> Result<String, TransactionError> {
        Err(TransactionError::RpcClient(
            "RPC operations not available in iOS FFI. Use iOS app's RPC client instead.".to_string()
        ))
    }
}
```

### Option 3: Make FFI Methods Conditional
Wrap iOS FFI methods that call RPC-dependent code in `#[cfg(feature = "rpc-client")]`.

## Recommended Approach for iOS

Since iOS apps should handle RPC calls externally, the best approach is:

1. **Document** that these FFI methods are not available for iOS
2. **Provide alternatives**: iOS app makes RPC calls directly, then uses FFI for transaction building/signing only
3. **Update Swift wrapper** to document these limitations

## Testing the Build

Try building again:
```bash
./build-ios.sh
```

If it still fails, the issue might be that some dependency is still pulling in `solana-client` transitively. Check with:
```bash
cargo tree --target aarch64-apple-ios --features ios --no-default-features | grep solana-client
```
