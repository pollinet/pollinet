# Comprehensive RPC Method Fix for iOS Build

## The Problem

When building for iOS without the `rpc-client` feature, these methods fail to compile because they access `self.rpc_client` which doesn't exist:

### `src/transaction/mod.rs` - Methods accessing `rpc_client`:
1. Line 792: `send_and_confirm_transaction`
2. Line 1481: (method TBD)
3. Line 1640: `discover_and_cache_nonce_accounts_by_authority`
4. Line 1748: `prepare_offline_bundle`
5. Line 2130: `create_unsigned_nonce_transactions`
6. Line 2388: `refresh_blockhash_in_unsigned_transaction`
7. Line 2461: (method TBD)
8. Line 2700: `submit_to_solana`

### `src/nonce/mod.rs` - Methods accessing `rpc_client`:
1. Line 89: `check_nonce_account_exists`

## The Solution

All these methods need to be wrapped with `#[cfg(feature = "rpc-client")]` and have stub implementations for non-RPC builds.

## Two Approaches

### Approach 1: Make ALL RPC Methods Conditional (RECOMMENDED)

Wrap each method that accesses `self.rpc_client`:

```rust
#[cfg(feature = "rpc-client")]
pub async fn send_and_confirm_transaction(&self, ...) -> Result<...> {
    let client = self.rpc_client.as_ref().ok_or_else(|| ...)?;
    // actual implementation
}

#[cfg(not(feature = "rpc-client"))]
pub async fn send_and_confirm_transaction(&self, ...) -> Result<...> {
    Err(TransactionError::RpcClient(
        "RPC not available on iOS. Use native URLSession.".to_string()
    ))
}
```

**Pros:**
- iOS FFI compiles cleanly
- Clear error messages if methods are called
- Android unchanged

**Cons:**
- Need to update ~10 methods
- Tedious but straightforward

### Approach 2: Enable `rpc-client` for iOS (NOT RECOMMENDED)

Enable the `rpc-client` feature for iOS builds:

```toml
ios = ["rpc-client"]
```

**Pros:**
- All methods compile
- No code changes needed

**Cons:**
- Pulls in `solana-client`
- Likely pulls in OpenSSL → back to the original linker error
- Adds unnecessary code to iOS binary

## Recommended Action

**Use Approach 1** - Make all RPC-dependent methods conditional.

### Methods to Fix

Here's the complete list to make conditional:

#### `src/transaction/mod.rs`:
- `send_and_confirm_transaction` (line ~788)
- `discover_and_cache_nonce_accounts_by_authority` (line ~1631)
- `prepare_offline_bundle` (line ~1742)
- `create_unsigned_nonce_transactions` (line ~2123) - already has `#[cfg(any(feature = "android", feature = "ios"))]`, change to just `"android"`
- `submit_offline_transaction` (line ~2296)
- `refresh_blockhash_in_unsigned_transaction` (line ~2434)
- `submit_to_solana` (line ~2699)
- `cast_vote` (line ~2788)
- `create_spl_transaction` (line ~2876)

#### `src/nonce/mod.rs`:
- `check_nonce_account_exists` (line ~83)

### Implementation Pattern

For each method:

1. Add `#[cfg(feature = "rpc-client")]` before the existing implementation
2. Add a stub implementation with `#[cfg(not(feature = "rpc-client"))]` that returns an error

### Why iOS FFI Doesn't Need These

The iOS FFI (`src/ffi/ios.rs`) only uses:
- Transaction building (unsigned transactions)
- Signature operations (add, verify)
- Queue management
- Offline bundle management (loading/saving)

It does NOT call:
- `send_and_confirm_transaction`
- `submit_to_solana`
- `refresh_blockhash_in_unsigned_transaction`
- Other RPC methods

iOS handles RPC calls natively using `URLSession`.

## Quick Fix Script

Here's a pattern you can use to quickly fix each method:

```bash
# For each method in the list above:
# 1. Find the method definition
# 2. Add #[cfg(feature = "rpc-client")] before it
# 3. Copy the method signature
# 4. Create stub with #[cfg(not(feature = "rpc-client"))]
```

## Alternative: Just Enable rpc-client for Now

If you want to get iOS building ASAP and deal with OpenSSL later:

```toml
ios = ["rpc-client"]  # Temporarily enable RPC client
```

Then deal with the OpenSSL linker issue separately.

## Status

- ❌ iOS build fails: 9 methods accessing `rpc_client`
- ✅ Solution identified: Make methods conditional
- ⏳ Action needed: Update ~10 methods with conditional compilation

## Time Estimate

- 30-60 minutes to update all methods
- Pattern is repetitive and straightforward

## Bottom Line

**The code is 99% done.** Just need to make RPC-dependent methods conditional so iOS builds cleanly without them.
