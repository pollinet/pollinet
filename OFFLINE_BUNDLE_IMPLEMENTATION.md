# PolliNet Offline Bundle Management - Android FFI Implementation

## Overview

The **Offline Bundle Management** system is the **CORE** of PolliNet. It enables:
- 📴 **Offline Transaction Creation** - Create signed transactions without internet
- 💰 **Cost Optimization** - Reuse nonce accounts instead of creating new ones ($0.00 vs $0.20 each)
- 🔄 **Smart Bundle Management** - Automatically refresh used nonces, only create what's needed
- 📡 **BLE/Mesh Ready** - Compressed transactions ready for low-bandwidth transmission

## Implementation Status

### ✅ Rust Core (Already Implemented)
The Rust SDK in `src/transaction/mod.rs` and `src/lib.rs` already has:
- ✅ `prepare_offline_bundle()` - Creates/refreshes nonce accounts
- ✅ `create_offline_transaction()` - Creates transactions completely offline  
- ✅ `submit_offline_transaction()` - Submits offline transactions to blockchain

### ✅ FFI Layer (Just Implemented)
**Files Modified:**
1. **`src/ffi/android.rs`** - Added JNI bindings:
   - `Java_xyz_pollinet_sdk_PolliNetFFI_prepareOfflineBundle`
   - `Java_xyz_pollinet_sdk_PolliNetFFI_createOfflineTransaction`
   - `Java_xyz_pollinet_sdk_PolliNetFFI_submitOfflineTransaction`

2. **`src/ffi/types.rs`** - Added FFI request types:
   - `PrepareOfflineBundleRequest` with camelCase serialization
   - `CreateOfflineTransactionRequest` with camelCase serialization
   - `SubmitOfflineTransactionRequest` with camelCase serialization

3. **`pollinet-sdk/src/main/java/xyz/pollinet/sdk/PolliNetFFI.kt`** - Added external declarations:
   - `external fun prepareOfflineBundle(handle: Long, requestJson: ByteArray): String`
   - `external fun createOfflineTransaction(handle: Long, requestJson: ByteArray): String`
   - `external fun submitOfflineTransaction(handle: Long, requestJson: ByteArray): String`

### ⏭️ Next Steps (Kotlin Wrappers)
**Still TODO:**
1. Add Kotlin data classes to `PolliNetSDK.kt`:
   ```kotlin
   data class PrepareOfflineBundleRequest(
       val count: Int,
       val senderKeypairBase64: String,
       val bundleFile: String? = null
   )
   
   data class CreateOfflineTransactionRequest(
       val senderKeypairBase64: String,
       val nonceAuthorityKeypairBase64: String,
       val recipient: String,
       val amount: Long,
       val cachedNonce: CachedNonceData
   )
   
   data class SubmitOfflineTransactionRequest(
       val transactionBase64: String,
       val verifyNonce: Boolean = true
   )
   
   data class CachedNonceData(
       val nonceAccount: String,
       val authority: String,
       val blockhash: String,
       val lamportsPerSignature: Long,
       val cachedAt: Long,
       val used: Boolean
   )
   
   data class OfflineTransactionBundle(
       val nonceCaches: List<CachedNonceData>,
       val maxTransactions: Int,
       val createdAt: Long
   )
   ```

2. Add high-level suspend functions:
   ```kotlin
   suspend fun prepareOfflineBundle(
       count: Int,
       senderKeypair: ByteArray,
       bundleFile: String? = null
   ): Result<OfflineTransactionBundle>
   
   suspend fun createOfflineTransaction(
       senderKeypair: ByteArray,
       nonceAuthorityKeypair: ByteArray,
       recipient: String,
       amount: Long,
       cachedNonce: CachedNonceData
   ): Result<String>
   
   suspend fun submitOfflineTransaction(
       transactionBase64: String,
       verifyNonce: Boolean = true
   ): Result<String>
   ```

## How It Works

### 1. Prepare Offline Bundle (While Online)
```kotlin
// Create 10 nonce accounts for offline use
val bundle = sdk.prepareOfflineBundle(
    count = 10,
    senderKeypair = senderKeypairBytes,
    bundleFile = "/path/to/bundle.json"
)
// First time: Creates 10 new nonce accounts (~$2.00)
// Next time: Refreshes used nonces (FREE!), only creates more if needed
```

### 2. Create Transaction (Completely Offline)
```kotlin
// NO INTERNET REQUIRED
val transaction = sdk.createOfflineTransaction(
    senderKeypair = senderKeypairBytes,
    nonceAuthorityKeypair = senderKeypairBytes,
    recipient = "RtsKQm3gAGL1Tayhs7ojWE9qytWqVh4G7eJTaNJs7vX",
    amount = 1_000_000, // lamports
    cachedNonce = bundle.nonceCaches[0]
)
// Returns compressed, signed transaction ready for BLE transmission
```

### 3. Submit Transaction (When Online Again)
```kotlin
// Back online - submit the offline-created transaction
val signature = sdk.submitOfflineTransaction(
    transactionBase64 = transaction,
    verifyNonce = true // Check if nonce is still valid
)
println("Transaction confirmed: $signature")
```

## Cost Optimization Example

```kotlin
// Day 1: Create bundle with 10 nonces
val bundle1 = sdk.prepareOfflineBundle(10, keypair, "bundle.json")
// Cost: 10 * $0.20 = $2.00

// Use 7 transactions...

// Day 2: Refresh bundle (only 3 unused remain)
val bundle2 = sdk.prepareOfflineBundle(10, keypair, "bundle.json")
// Cost: $0.00! (Refreshed 7 used nonces for free)
// Saved: 7 * $0.20 = $1.40
```

## Key Features

### Smart Bundle Management
- **Loads existing bundle** from file if available
- **Refreshes used nonces** by fetching new blockhash (FREE!)
- **Only creates new accounts** when total < requested count
- **Saves only unused nonces** to file (compact storage)

### True Offline Operation
- **No RPC calls** during `createOfflineTransaction()`
- **All data from cache** - blockhash, authority, nonce account
- **Perfect for mesh networks** where internet is intermittent

### BLE/Mesh Ready
- **Compressed transactions** using LZ4 (if > threshold)
- **Ready for fragmentation** via existing `fragment()` API
- **Small payloads** optimized for BLE MTU (512 bytes)

## Testing Plan

1. **Unit Tests** (Kotlin):
   - Serialize/deserialize requests
   - Base64 encoding/decoding
   - Bundle JSON parsing

2. **Integration Tests** (With Rust FFI):
   - Prepare bundle with valid keypair
   - Create offline transaction
   - Submit to devnet

3. **UI Demo** (Diagnostics Screen):
   - Button: "Prepare Offline Bundle"
   - Button: "Create Offline Transaction"  
   - Button: "Submit Transaction"
   - Display bundle stats, transaction status

## References

- **Rust Examples**:
  - `examples/offline_bundle_management.rs` - Full demo
  - `examples/offline_transaction_flow.rs` - Step-by-step
  - `examples/offline_transaction_sender.rs` - Sender side
  - `examples/offline_transaction_receiver.rs` - Receiver side

- **TODO.md**: M7 - Offline Bundle Management

---

**Status**: ✅ Rust Core Complete | ✅ FFI Layer Complete | ⏭️ Kotlin Wrappers TODO  
**Last Updated**: 2025-11-05

