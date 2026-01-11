# PolliNet Transaction Flow Demo

This document demonstrates the complete transaction lifecycle through PolliNet's BLE mesh network.

## Overview

The flow consists of 8 main steps:

1. **Fragment** - Break transaction into BLE-sized chunks
2. **Queue (Outbound)** - Add fragments to sender's outbound queue
3. **Transmit** - Send fragments over BLE
4. **Receive** - Receiver gets fragments
5. **Reassemble** - Reconstruct complete transaction
6. **Queue (Received)** - Add to receiver's submission queue
7. **Submit** - Send to Solana RPC
8. **Confirm** - Queue confirmation for relay back to sender

## Code Example

See `examples/transaction_flow_test.rs` for a complete working example.

## Step-by-Step Flow

### 1. Create Transaction (Sender)

```rust
use solana_sdk::{signature::Keypair, system_instruction, transaction::Transaction};

let sender = Keypair::new();
let recipient = Keypair::new();
let amount = 1_000_000; // 0.001 SOL

let instruction = system_instruction::transfer(
    &sender.pubkey(),
    &recipient.pubkey(),
    amount,
);

let mut transaction = Transaction::new_with_payer(
    &[instruction],
    Some(&sender.pubkey()),
);

transaction.sign(&[&sender], blockhash);
let tx_bytes = bincode1::serialize(&transaction)?;
```

### 2. Fragment Transaction (Sender)

```rust
use pollinet::ffi::transport::HostDrivenBleTransport;

let transport = HostDrivenBleTransport::new();

// Fragment with MTU-aware payload size
let mtu = 512; // Typical BLE MTU after negotiation
let max_payload = mtu - 10; // Reserve for overhead

let fragments = transport.queue_transaction(tx_bytes.clone(), Some(max_payload))?;
// Returns: Vec<Fragment> with metadata (id, index, total, data, type, checksum)
```

**Output:**
```
✅ Fragmented and queued: 1 fragments
   Fragment 1/1: 333 bytes (data: 289 bytes)
📊 Outbound queue size: 1 fragments
```

### 3. Transmit Fragments (Sender → BLE)

```rust
let mut transmitted_fragments = Vec::new();

while let Some(fragment_bytes) = transport.next_outbound() {
    // Send over BLE (notify or write characteristic)
    bluetooth_gatt.notify(RX_CHAR_UUID, &fragment_bytes)?;
    transmitted_fragments.push(fragment_bytes);
}
```

**Output:**
```
📤 Transmitted fragment 1/1 (333 bytes)
✅ All 1 fragments transmitted
📊 Outbound queue after transmission: 0 fragments
```

### 4. Receive Fragments (BLE → Receiver)

```rust
// On receiver device
let receiver_transport = HostDrivenBleTransport::new();

for fragment_bytes in transmitted_fragments {
    receiver_transport.push_inbound(fragment_bytes)?;
    
    let metrics = receiver_transport.metrics();
    println!("Fragments buffered: {}, Complete: {}", 
        metrics.fragments_buffered, 
        metrics.transactions_complete);
}
```

**Output:**
```
📥 Received fragment 1/1
   Fragments buffered: 0, Transactions complete: 1
✅ All fragments received
```

### 5. Check Reassembly Status (Receiver)

```rust
let metrics = receiver_transport.metrics();

println!("📊 Reassembly metrics:");
println!("   Fragments buffered: {}", metrics.fragments_buffered);
println!("   Transactions complete: {}", metrics.transactions_complete);
println!("   Reassembly failures: {}", metrics.reassembly_failures);
```

**Output:**
```
📊 Reassembly metrics:
   Fragments buffered: 0
   Transactions complete: 1
   Reassembly failures: 0
```

### 6. Read from Received Queue (Receiver)

```rust
let queue_size = receiver_transport.received_queue_size();

if queue_size > 0 {
    if let Some((tx_id, reassembled_bytes, received_at)) = 
        receiver_transport.next_received_transaction() 
    {
        println!("✅ Retrieved transaction:");
        println!("   ID: {}", tx_id);
        println!("   Size: {} bytes", reassembled_bytes.len());
        println!("   Received at: {}", received_at);
        
        // Verify integrity
        assert_eq!(reassembled_bytes, tx_bytes);
    }
}
```

**Output:**
```
📊 Received queue size: 1 transactions
✅ Retrieved received transaction:
   Transaction ID: 5ebe0c26-5388-861c-a080-f765b2dc669c
   Received at: 1704298836
   Size: 289 bytes
   ✅ Transaction matches original!
```

### 7. Submit to Solana RPC (Receiver)

```rust
use solana_client::rpc_client::RpcClient;

let rpc_client = RpcClient::new("https://api.devnet.solana.com");

// Deserialize transaction
let transaction: Transaction = bincode1::deserialize(&reassembled_bytes)?;

// Submit to blockchain
let signature = rpc_client.send_and_confirm_transaction(&transaction)?;

println!("✅ Transaction submitted: {}", signature);
```

**Output:**
```
🌐 Submitting transaction to Solana RPC...
✅ Transaction submitted SUCCESSFULLY!
   Signature: 5j7s8K9L2m3n4p5q6r7s8t9u0v1w2x3y4z5a6b7c8d9e0f1g2h3i4j5k6l7m8n9o0p
   Transaction is now on-chain
```

### 8. Queue Confirmation (Receiver → Sender)

```rust
// Queue confirmation for relay back to sender
receiver_transport.queue_confirmation(&tx_id, &signature)?;

let conf_queue_size = receiver_transport.confirmation_queue_size();
println!("📊 Confirmation queue: {} confirmations", conf_queue_size);

// Read confirmation for transmission
if let Some((conf_tx_id, sig, confirmed_at)) = receiver_transport.next_confirmation() {
    println!("✅ Confirmation ready to relay:");
    println!("   TX ID: {}", conf_tx_id);
    println!("   Signature: {}", sig);
    
    // Send confirmation back to sender over BLE
    // (Same fragmentation process as step 2-3)
}
```

**Output:**
```
✅ Confirmation queued for relay
📊 Confirmation queue size: 1 confirmations
✅ Retrieved confirmation:
   Transaction ID: 5ebe0c26-5388-861c-a080-f765b2dc669c
   Signature: 5j7s8K9L2m3n4p5q6r7s8t9u0v1w2x3y4z5a6b7c8d9e0f1g2h3i4j5k6l7m8n9o0p
   Confirmed at: 1704298840
```

## Complete Flow Summary

```
SENDER DEVICE                    BLE MESH                    RECEIVER DEVICE
─────────────                    ────────                    ───────────────

1. Create TX (289 bytes)
2. Fragment (1 fragment)
3. Queue outbound
                    ──────────────────────────────>
4. Transmit (333 bytes)          BLE Notify/Write           5. Receive fragment
                                                             6. Push to inbound buffers
                                                             7. Reassemble (289 bytes)
                                                             8. Queue received
                                                             9. Submit to Solana RPC
                                                             10. Get signature
                                                             11. Queue confirmation
                    <──────────────────────────────
12. Receive confirmation         BLE Notify/Write           13. Transmit confirmation
14. Update TX status
```

## Key Points

### Fragmentation
- **MTU-aware**: Fragments sized based on negotiated BLE MTU (typically 512 bytes)
- **Overhead**: ~44 bytes per fragment (transaction_id: 32, indices: 4, data length: 8)
- **Checksum**: SHA-256 hash of complete transaction for verification

### Reassembly
- **Buffering**: Fragments stored until all pieces received
- **Deduplication**: Transaction hash prevents duplicate submissions
- **Metrics**: Real-time tracking of buffered fragments and completed transactions

### Queues
- **Outbound**: Fragments waiting to be sent
- **Inbound**: Fragments being reassembled
- **Received**: Complete transactions ready for submission
- **Confirmation**: Signatures ready to relay back to sender

### Error Handling
- **Missing fragments**: Timeout after 30 seconds
- **Corrupt data**: Checksum verification fails
- **Duplicate transactions**: Rejected by hash comparison
- **RPC failures**: Added to retry queue with exponential backoff

## Testing

### Running the Example

**Note:** This example requires Linux or Android with `dbus` installed. It **cannot run on macOS** due to BLE library dependencies.

#### On Linux:
```bash
# Install dependencies
sudo apt install libdbus-1-dev pkg-config  # Ubuntu/Debian
sudo dnf install dbus-devel pkgconf-pkg-config  # Fedora

# Run the example
cargo run --example transaction_flow_test
```

#### On macOS:
The example code is provided for reference but cannot be executed on macOS. Instead:
1. Review the code in `examples/transaction_flow_test.rs`
2. Read this documentation for the expected flow
3. Test on Android devices using the PolliNet app

#### On Android:
The actual flow is implemented and can be tested using the Android app:
```bash
cd pollinet-android
./gradlew installDebug
# Use the MWA Transaction Demo screen to test the complete flow
```

### Expected Output

When running successfully on Linux, you'll see all 8 steps complete with ✅ checkmarks:

```
=== PolliNet Transaction Flow Test ===

Step 1: Initializing Host-Driven BLE Transport...
✅ Transport initialized

Step 2: Creating sample transaction...
✅ Transaction created: 289 bytes
   Sender: 7xJ8...
   Recipient: 9kL2...
   Amount: 1000000 lamports

Step 3: Fragmenting transaction (SENDER)...
✅ Fragmented and queued: 1 fragments
   Fragment 1/1: 333 bytes (data: 289 bytes)

📊 Outbound queue size: 1 fragments

Step 4: Simulating BLE transmission...
   📤 Transmitted fragment 1/1 (333 bytes)
✅ All 1 fragments transmitted

📊 Outbound queue after transmission: 0 fragments

Step 5: Receiving and reassembling (RECEIVER)...
   📥 Received fragment 1/1
      Fragments buffered: 0, Transactions complete: 1
✅ All fragments received

Step 6: Checking reassembly status...
📊 Reassembly metrics:
   Fragments buffered: 0
   Transactions complete: 1
   Reassembly failures: 0

Step 7: Reading from received queue...
📊 Received queue size: 1 transactions
✅ Retrieved received transaction:
   Transaction ID: 5ebe0c26-5388-861c-a080-f765b2dc669c
   Received at: 1704298836
   Size: 289 bytes
   ✅ Transaction matches original!

Step 8: Simulating RPC submission and queueing confirmation...
   Mock RPC signature: 5j7s8K9L...
✅ Confirmation queued for relay

📊 Confirmation queue size: 1 confirmations
✅ Retrieved confirmation:
   Transaction ID: 5ebe0c26-5388-861c-a080-f765b2dc669c
   Signature: 5j7s8K9L...
   Confirmed at: 1704298840

=== Transaction Flow Summary ===
✅ 1. Transaction created: 289 bytes
✅ 2. Fragmented: 1 fragments
✅ 3. Queued to outbound queue
✅ 4. Transmitted 1 fragments
✅ 5. Received all fragments
✅ 6. Transaction reassembled successfully
✅ 7. Added to received queue
✅ 8. Confirmation queued for relay

🎉 Transaction flow test complete!
```

## Android Integration

In Android, this flow is managed by `BleService.kt`:

1. **Sender**: `queueSignedTransaction()` → fragments → `sendToGatt()`
2. **Receiver**: `onCharacteristicChanged()` → `handleReceivedData()` → `pushInbound()`
3. **Auto-submission**: `processReceivedQueue()` → `submitOfflineTransaction()`
4. **Confirmation**: `queueConfirmation()` → relay back to sender

See `pollinet-android/pollinet-sdk/src/main/java/xyz/pollinet/sdk/BleService.kt` for implementation.

