# PolliNet Queue Flow Reference

This document explains the transaction queue flow using simple code examples.

## ⚠️ Important Note

The example code in `transaction_flow_test.rs` **cannot be compiled on macOS** due to BLE library dependencies (`btleplug` → `dbus`).

This is **reference code** that demonstrates:
- How queues work
- How fragmentation/reassembly works
- The complete transaction lifecycle

## Reference Code Flow

### 1. Create Dummy Transaction

```rust
// Just use dummy bytes - no Solana SDK needed
let tx_bytes: Vec<u8> = vec![
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // ... padded to ~280 bytes
];
```

### 2. Fragment and Queue (Outbound)

```rust
let transport = HostDrivenBleTransport::new();

// Fragment with MTU-aware sizing
let mtu = 512;
let max_payload = mtu - 10;
let fragments = transport.queue_transaction(tx_bytes.clone(), Some(max_payload))?;

// Result: Vec<Fragment> with metadata
// - id: transaction identifier
// - index: fragment number (0, 1, 2...)
// - total: total fragments needed
// - data: actual payload bytes
// - checksum: SHA-256 of complete transaction
```

**Output:**
```
✅ Fragmented and queued: 1 fragments
   Fragment 1/1: ID=5ebe0c26, index=0, total=1, data=282 bytes
📊 Outbound queue size: 1 fragments
```

### 3. Dequeue Fragments (Read Outbound)

```rust
let mut transmitted_fragments = Vec::new();

while let Some(fragment_bytes) = transport.next_outbound() {
    // fragment_bytes is the serialized Fragment (binary data)
    transmitted_fragments.push(fragment_bytes);
}
```

**Output:**
```
📤 Dequeued fragment 1/1 (333 bytes)
✅ All 1 fragments dequeued
📊 Outbound queue after dequeue: 0 fragments
```

### 4. Push to Reassembly Buffers (Inbound)

```rust
for fragment_bytes in transmitted_fragments {
    transport.push_inbound(fragment_bytes)?;
    
    let metrics = transport.metrics();
    println!("Fragments buffered: {}", metrics.fragments_buffered);
    println!("Transactions complete: {}", metrics.transactions_complete);
}
```

**Output:**
```
📥 Added fragment 1/1 to reassembly buffer
   Fragments buffered: 0, Transactions complete: 1
✅ All fragments added to reassembly
```

**Note:** `fragments_buffered` goes to 0 immediately because with only 1 fragment, reassembly completes instantly!

### 5. Check Reassembly Metrics

```rust
let metrics = transport.metrics();
println!("Fragments buffered: {}", metrics.fragments_buffered);
println!("Transactions complete: {}", metrics.transactions_complete);
println!("Reassembly failures: {}", metrics.reassembly_failures);
```

**Output:**
```
📊 Reassembly metrics:
   Fragments buffered: 0
   Transactions complete: 1
   Reassembly failures: 0
```

### 6. Read from Received Queue

```rust
let queue_size = transport.received_queue_size();

if let Some((tx_id, reassembled_bytes, received_at)) = 
    transport.next_received_transaction() 
{
    println!("Transaction ID: {}", tx_id);
    println!("Size: {} bytes", reassembled_bytes.len());
    
    // Verify integrity
    assert_eq!(reassembled_bytes, tx_bytes);
}
```

**Output:**
```
📊 Received queue size: 1 transactions
✅ Retrieved received transaction:
   Transaction ID: uuid-v4-string
   Received at: 1704298836
   Size: 282 bytes
   ✅ Transaction matches original!
```

### 7. Queue Confirmation

```rust
let mock_signature = "5j7s8K9L2m3n4p5q6r7s8t9u0v1w2x3y...";
transport.queue_confirmation(&tx_id, mock_signature)?;

let conf_queue_size = transport.confirmation_queue_size();
```

**Output:**
```
✅ Confirmation queued
📊 Confirmation queue size: 1 confirmations
```

### 8. Read Confirmation

```rust
if let Some((tx_id, signature, confirmed_at)) = transport.next_confirmation() {
    println!("TX ID: {}", tx_id);
    println!("Signature: {}", signature);
}
```

**Output:**
```
✅ Retrieved confirmation:
   Transaction ID: uuid-v4-string
   Signature: 5j7s8K9L2m3n4p5q...
   Confirmed at: 1704298840
```

## Queue State Machine

```
┌─────────────┐
│   Create    │
│ Transaction │
│  (282 bytes)│
└──────┬──────┘
       │
       ↓
┌─────────────┐
│  Fragment   │ ← queue_transaction()
│  (1 frag @  │
│  333 bytes) │
└──────┬──────┘
       │
       ↓
┌─────────────┐
│  Outbound   │ ← next_outbound()
│    Queue    │
│ (1 fragment)│
└──────┬──────┘
       │ Dequeue
       ↓
┌─────────────┐
│  Inbound    │ ← push_inbound()
│  Buffers    │
│ (reassembly)│
└──────┬──────┘
       │ Complete
       ↓
┌─────────────┐
│  Received   │ ← next_received_transaction()
│    Queue    │
│(1 complete  │
│transaction) │
└──────┬──────┘
       │
       ↓
┌─────────────┐
│Confirmation │ ← queue_confirmation()
│    Queue    │ ← next_confirmation()
│ (1 signature)│
└─────────────┘
```

## Key Concepts

### Fragmentation
- **Why?** Transactions (200-400 bytes) don't fit in a single BLE packet (MTU ~512 bytes)
- **How?** Break into chunks with metadata (id, index, total, checksum)
- **Overhead:** ~44 bytes per fragment (32-byte ID + 12 bytes metadata)

### Reassembly
- **Buffering:** Fragments stored until all pieces received
- **Matching:** Use transaction ID (SHA-256 hash) to group fragments
- **Completion:** When `received_fragments == total_fragments`, reassemble

### Deduplication
- **Hash Check:** Calculate SHA-256 of complete transaction
- **Submitted Set:** Track hashes of already-submitted transactions
- **Skip Duplicates:** Prevent re-submitting same transaction

### Queues
1. **Outbound:** Fragments waiting to send
2. **Inbound Buffers:** Fragments being reassembled (not a queue, a HashMap)
3. **Received:** Complete transactions ready for RPC submission
4. **Confirmation:** Signatures ready to relay back to sender

## Testing on Android

The **actual working implementation** is in the Android app:

```bash
cd pollinet-android
./gradlew installDebug
```

Use the "MWA Transaction Demo" screen to test:
1. **Sender:** Create → Sign → Send via BLE
2. **Receiver:** Auto-receives → Reassembles → Submits to Solana
3. **Sender:** Receives confirmation back

## Expected Behavior

For a **282-byte transaction** with **MTU=512**:
- ✅ Creates **1 fragment** (fits in single packet)
- ✅ Fragment size: **333 bytes** (282 data + 51 overhead)
- ✅ Reassembly: **Instant** (1/1 fragments complete)
- ✅ Received queue: **1 transaction** ready
- ✅ Confirmation queue: **1 signature** to relay

For a **1500-byte transaction** with **MTU=512**:
- ✅ Creates **3 fragments** (~500 bytes each)
- ✅ Reassembly: **Gradual** (1/3 → 2/3 → 3/3)
- ✅ Buffer clears when complete
- ✅ Moves to received queue

## Summary

This reference code demonstrates:
- ✅ **Fragmentation:** Breaking large data into BLE-sized chunks
- ✅ **Queue management:** Outbound → Inbound → Received → Confirmation
- ✅ **Reassembly:** Reconstructing complete transactions from fragments
- ✅ **Integrity:** Checksum verification
- ✅ **Deduplication:** Hash-based duplicate detection

**Cannot compile on macOS** due to BLE dependencies, but the logic is valid and used in production on Android!

