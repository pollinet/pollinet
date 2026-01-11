# Implementation Plan: processOutboundQueue()

## Architecture Overview

There are **TWO queue systems** in the codebase:

1. **High-Level Transaction Queue** (`queue_manager().outbound`)
   - Stores `OutboundTransaction` structs with metadata
   - Contains: `tx_id`, `original_bytes`, `fragments` (pre-fragmented), `priority`, etc.
   - Accessed via: `popOutboundTransaction()` / `pushOutboundTransaction()`
   - Purpose: Transaction management, priority handling, retry logic

2. **Low-Level Fragment Queue** (`HostBleTransport.outbound_queue`)
   - Stores serialized fragment bytes (bincode-encoded `TransactionFragment`)
   - Ready-to-send binary data
   - Accessed via: `nextOutbound()` / `queue_transaction()`
   - Purpose: Direct BLE transmission

## Current Flow

When `queueSignedTransaction()` is called:
1. Calls `fragment()` → calls `queue_transaction()` in Rust
   - Fragments the transaction
   - Serializes fragments to binary
   - Adds to **LOW-LEVEL queue** ✅
2. Calls `pushOutboundTransaction()`
   - Adds transaction metadata to **HIGH-LEVEL queue** ✅
3. Triggers `WorkEvent.OutboundReady`
4. Sending loop uses `nextOutbound()` to read from LOW-LEVEL queue ✅

## The Problem

`processOutboundQueue()` is supposed to process the HIGH-LEVEL queue, but:
- It only logs "Would transmit" without actually doing anything
- The sending loop already handles transmission via LOW-LEVEL queue
- These two systems are disconnected

## Implementation Options

### Option 1: Remove processOutboundQueue() (Current Fix)
**Status:** ✅ Already implemented (disabled)

**Pros:**
- Simplest solution
- No duplication
- Sending loop already works

**Cons:**
- HIGH-LEVEL queue becomes unused/abandoned
- Can't leverage priority/retry features of HIGH-LEVEL queue

### Option 2: Connect HIGH-LEVEL → LOW-LEVEL Queue
**Complexity:** Medium
**Purpose:** Use HIGH-LEVEL queue as the source of truth

**Implementation:**
```kotlin
private suspend fun processOutboundQueue() {
    val sdkInstance = sdk ?: return
    
    if (_connectionState.value != ConnectionState.CONNECTED) {
        appendLog("⚠️ Not connected - outbound processing skipped")
        return
    }
    
    var processedCount = 0
    val batchSize = 10
    
    repeat(batchSize) {
        val outboundTx = sdkInstance.popOutboundTransaction().getOrNull() ?: return@repeat
        
        appendLog("📤 Processing outbound tx: ${outboundTx.txId.take(8)}... (priority: ${outboundTx.priority})")
        
        // Decode the original transaction bytes
        val txBytes = android.util.Base64.decode(outboundTx.originalBytes, android.util.Base64.DEFAULT)
        
        // Re-fragment with current MTU (in case MTU changed)
        val maxPayload = (currentMtu - 10).coerceAtLeast(20)
        val fragmentResult = sdkInstance.fragment(txBytes, maxPayload)
        
        fragmentResult.fold(
            onSuccess = { fragmentList ->
                // fragment() already calls queue_transaction() which adds to LOW-LEVEL queue
                appendLog("✅ Queued ${fragmentList.fragments.size} fragments for transmission")
                processedCount++
            },
            onFailure = { error ->
                appendLog("❌ Failed to fragment transaction: ${error.message}")
                // Add to retry queue?
            }
        )
    }
    
    if (processedCount > 0) {
        appendLog("✅ Processed $processedCount outbound transactions")
        // Check if more work remains
        val remaining = sdkInstance.getOutboundQueueSize().getOrNull() ?: 0
        if (remaining > 0) {
            appendLog("📊 $remaining transactions remaining, re-triggering event")
            workChannel.trySend(WorkEvent.OutboundReady)
        }
    }
}
```

**Pros:**
- Uses HIGH-LEVEL queue system (priority, retry logic)
- Can handle MTU changes (re-fragment if needed)
- More control over transaction flow

**Cons:**
- Fragments are queued twice (once in `queueSignedTransaction`, once here)
- More complex
- Duplicates `fragment()` call

### Option 3: Make processOutboundQueue() Use Existing Fragments
**Complexity:** High
**Purpose:** Use fragments already stored in HIGH-LEVEL queue

**Requirements:**
- Need Rust FFI function to add fragments to LOW-LEVEL queue from HIGH-LEVEL queue
- Or: Modify `pushOutboundTransaction` to automatically queue fragments

**Implementation (Rust side needed):**
```rust
// New FFI function needed:
pub fn queue_fragments_from_outbound_transaction(&self, tx_id: &str) -> Result<(), String> {
    // 1. Get transaction from HIGH-LEVEL queue (without popping)
    // 2. Serialize fragments
    // 3. Add to LOW-LEVEL queue
    // 4. Return success
}
```

**Kotlin:**
```kotlin
private suspend fun processOutboundQueue() {
    // ... connection checks ...
    
    val outboundTx = sdkInstance.popOutboundTransaction().getOrNull() ?: return
    
    // Call new FFI function to queue fragments
    sdkInstance.queueFragmentsFromOutboundTransaction(outboundTx.txId)
        .fold(
            onSuccess = { 
                appendLog("✅ Queued fragments for tx ${outboundTx.txId}")
            },
            onFailure = { error ->
                appendLog("❌ Failed to queue fragments: $error")
            }
        )
}
```

**Pros:**
- No duplicate fragmentation
- Efficient (uses pre-fragmented data)
- Proper separation of concerns

**Cons:**
- Requires Rust changes (new FFI function)
- More complex architecture
- Need to handle MTU changes (fragments might need re-sizing)

### Option 4: Unify Queue Systems (Refactor)
**Complexity:** Very High
**Purpose:** Single queue system

**Requirements:**
- Remove one queue system
- Consolidate to either HIGH-LEVEL or LOW-LEVEL
- Update all code paths

**Pros:**
- Cleaner architecture
- No duplication
- Easier to maintain

**Cons:**
- Major refactoring
- Risk of breaking changes
- Time-consuming

## Recommended Approach

**Option 1 (Current Fix)** is the **best immediate solution** because:
1. ✅ Already works (sending loop handles transmission)
2. ✅ No code duplication
3. ✅ Simpler architecture
4. ✅ No breaking changes

**However**, if you want to use the HIGH-LEVEL queue features (priority, retry), then **Option 3** is the best long-term solution, but it requires:

1. **Rust Changes:**
   ```rust
   // In src/ffi/android.rs or src/ffi/transport.rs
   pub fn queue_fragments_from_outbound_transaction(&self, tx_id: &str) -> Result<(), String> {
       // Implementation
   }
   ```

2. **Kotlin Changes:**
   ```kotlin
   // Add to PolliNetFFI.kt
   external fun queueFragmentsFromOutboundTransaction(handle: Long, txId: String): String
   
   // Add to PolliNetSDK.kt
   suspend fun queueFragmentsFromOutboundTransaction(txId: String): Result<Unit>
   
   // Implement processOutboundQueue() as shown in Option 3
   ```

3. **Considerations:**
   - MTU changes: Fragments might need re-sizing if MTU increased
   - Error handling: What if transaction was already sent?
   - Retry logic: How to handle failures?

## Summary

| Option | Complexity | Pros | Cons | Recommended? |
|--------|-----------|------|------|--------------|
| Option 1: Remove | Low | Simple, works now | Abandons HIGH-LEVEL queue | ✅ Yes (immediate) |
| Option 2: Re-fragment | Medium | Uses HIGH-LEVEL queue | Duplicate fragmentation | ❌ No |
| Option 3: Use existing fragments | High | Efficient, proper architecture | Needs Rust changes | ✅ Yes (long-term) |
| Option 4: Unify | Very High | Cleanest | Major refactor | ⚠️ Future consideration |

**My Recommendation:** 
- Keep **Option 1** (current fix) for now
- Plan **Option 3** for future if priority/retry features are needed
- Consider **Option 4** for major refactoring if queue systems cause issues

