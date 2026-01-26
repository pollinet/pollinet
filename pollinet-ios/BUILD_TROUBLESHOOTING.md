# iOS Build Troubleshooting: OpenSSL Issue

## Quick Fix

The build script now uses `--no-default-features` to exclude `solana-client` (and thus OpenSSL) from iOS builds.

**Try building again:**
```bash
./build-ios.sh
```

## If Build Still Fails

### Check what's pulling in OpenSSL

```bash
cargo tree --target aarch64-apple-ios --features ios --no-default-features | grep -i openssl
```

### Verify RPC-dependent methods are stubbed

The following methods in `src/transaction/mod.rs` should be marked with `#[cfg(feature = "rpc-client")]`:
- `fetch_nonce_account_data()` - Already conditional ✅
- `prepare_offline_nonce_data()` - Already conditional ✅  
- `refresh_blockhash_in_unsigned_transaction()` - Needs fix
- `submit_offline_transaction()` - Needs fix
- `discover_and_cache_nonce_accounts_by_authority()` - Already conditional ✅

### iOS FFI Impact

These FFI functions will return errors when called without RPC:
- `pollinet_prepare_offline_bundle()` - Uses `prepare_offline_nonce_data()`
- `pollinet_cache_nonce_accounts()` - Uses `prepare_offline_nonce_data()`  
- `pollinet_refresh_offline_bundle()` - Uses `prepare_offline_nonce_data()`
- `pollinet_refresh_blockhash_in_unsigned_transaction()` - Uses RPC directly
- `pollinet_submit_offline_transaction()` - Uses RPC directly

**Workaround for iOS:**
- iOS app should handle RPC calls directly
- Only use FFI for transaction building/signing/fragmentation
- Skip RPC-dependent functions or implement them in Swift

## Alternative: Use Vendored OpenSSL

If you really need RPC functionality on iOS, you could configure OpenSSL to use vendored builds:

```toml
[target.'cfg(target_os = "ios")'.dependencies]
openssl = { version = "0.10", features = ["vendored"] }
solana-client = { version = "2.3.0", default-features = false }
```

But this adds significant build complexity and binary size.

## Recommended: No RPC in iOS FFI

For iOS, the recommended approach is:
1. **iOS app handles all RPC calls** (using native networking)
2. **FFI only handles**: Transaction building, signing, fragmentation
3. **RPC-dependent FFI methods** return clear errors directing users to use iOS app's RPC

This keeps the binary small and avoids cross-compilation issues.
