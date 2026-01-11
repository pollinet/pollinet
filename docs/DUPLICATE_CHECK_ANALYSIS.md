# Duplicate Transaction Check - Reverse Engineering Analysis

## Problem
Transactions are being successfully reassembled (`transactionsComplete=1`) but are **NOT** appearing in the `received_tx_queue`, causing worker timeouts.

## Root Cause

From the Android logs:
```
📊 Metrics: fragmentsBuffered=0, transactionsComplete=1, reassemblyFailures=0
📊 Received queue size: 0
⚠️ Transaction was completed but NOT in received queue!
✅ Fragments were successfully reassembled
❌ BUT transaction was rejected as a DUPLICATE
```

### Flow Analysis

1. **Fragment Reception** (`push_inbound()`)
   - Fragment arrives via BLE
   - Added to `inbound_buffers` for reassembly
   - When all fragments received, reassembly completes

2. **Reassembly Completion** (lines 220-282 in `transport.rs`)
   - Fragments reassembled into full transaction bytes
   - `metrics.transactions_complete += 1` ✅
   - Fragments removed from `inbound_buffers` ✅
   - **Calls `push_received_transaction(tx_bytes)`** ← Critical step

3. **Duplicate Check** (`push_received_transaction()` at line 463)
   - Calculates SHA-256 hash of transaction bytes
   - **Checks `submitted_tx_hashes` HashMap** (line 485)
   - If hash exists → **Returns `false`** (line 491)
   - If hash doesn't exist → Adds to `received_tx_queue` → Returns `true`

4. **The Bug**
   - Comment says "COMMENTED OUT" (line 480)
   - **But the code is STILL RUNNING** (lines 481-503)
   - Duplicate check at lines 485-492 is active
   - When duplicate detected, function returns `false`
   - Transaction never added to `received_tx_queue`

## Code Location

**File:** `src/ffi/transport.rs`
**Function:** `push_received_transaction()` (lines 463-526)
**Problematic Code:** Lines 480-503

```rust
// COMMENTED OUT: Check if already submitted (duplicate detection disabled for testing)
let mut submitted = self.submitted_tx_hashes.lock();
// ... code is STILL ACTIVE ...
if submitted.contains_key(&tx_hash) {
    // ... returns false, preventing queue addition
    return false;
}
```

## Why Transactions Are Marked as Duplicates

The `submitted_tx_hashes` HashMap persists in memory for the lifetime of the `HostBleTransport` instance:

1. **App Process Persistence**
   - If the Android app process doesn't fully terminate, the Rust library instance persists
   - `submitted_tx_hashes` accumulates hashes across app restarts (if process survives)
   - Even after "uninstalling" the app, if the process wasn't killed, the HashMap persists

2. **Same Transaction Bytes = Same Hash**
   - If you're testing with the same transaction (even with random amounts), the signed bytes might be identical
   - Solana transactions include blockhash, which is cached
   - If blockhash is the same and other fields match, the hash will match

3. **Testing Scenarios**
   - Sending the same transaction twice → Second one is duplicate
   - Same device as sender and receiver (loopback) → Receives its own transaction
   - App wasn't fully killed → Old hashes persist

## Verification: Rust Example Works

The Rust example (`transaction_flow_test.rs`) works because:
- It creates a **fresh** `HostBleTransport` instance
- `submitted_tx_hashes` starts empty
- No previous transactions have been processed
- First transaction is never a duplicate

## Solution

To properly disable the duplicate check for testing:

1. **Actually comment out** the duplicate check code (lines 481-503)
2. **Skip the hash check entirely** - always proceed to queue the transaction
3. **Keep the hash insertion** commented out too (so we don't pollute the cache)

### Fixed Code

```rust
// DUPLICATE CHECK DISABLED FOR TESTING
// The following code checks if a transaction was already submitted and rejects duplicates.
// DISABLED: Allow all transactions through for testing purposes.

/*
let mut submitted = self.submitted_tx_hashes.lock();
let submitted_count_before = submitted.len();
tracing::debug!("📊 Submitted transactions cache size: {}", submitted_count_before);

if submitted.contains_key(&tx_hash) {
    let submitted_at = submitted.get(&tx_hash).copied().unwrap_or(0);
    tracing::warn!("⏩ Skipping duplicate transaction {} (submitted at timestamp {})", 
        tx_hash_hex.chars().take(16).collect::<String>(), submitted_at);
    drop(submitted);
    tracing::info!("❌ push_received_transaction() returning false (duplicate detected)");
    return false;
}

tracing::debug!("✅ Transaction is not a duplicate, proceeding...");

// Add to submitted set with current timestamp
let now = Self::current_timestamp();
tracing::debug!("⏰ Current timestamp: {}", now);
submitted.insert(tx_hash.clone(), now);
let submitted_count_after = submitted.len();
drop(submitted);
tracing::debug!("✅ Added transaction hash to submitted set (cache size: {} -> {})", 
    submitted_count_before, submitted_count_after);
*/

// SKIP DUPLICATE CHECK - Always proceed
let now = Self::current_timestamp();
tracing::info!("⚠️ DUPLICATE CHECK DISABLED - Allowing transaction through");
```

## Testing After Fix

1. **Rebuild Rust library:**
   ```bash
   cd pollinet-android
   ./gradlew :pollinet-sdk:build
   ```

2. **Rebuild and install Android app:**
   ```bash
   ./gradlew installDebug
   ```

3. **Clear app data** (to ensure clean state):
   ```bash
   adb shell pm clear xyz.pollinet.android
   ```

4. **Test transaction flow:**
   - Send transaction from Device 1
   - Receive on Device 2
   - Check logs - transaction should appear in `received_tx_queue`

## Production Considerations

For production, the duplicate check should be **re-enabled** but with:
1. **Time-based expiration** (already implemented via `cleanup_old_submissions()`)
2. **Persistence** (save `submitted_tx_hashes` to disk for app restarts)
3. **Configurable** (enable/disable via feature flag)

