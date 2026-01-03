# PolliNet Examples

This directory contains examples demonstrating PolliNet SDK functionality.

## Transaction Queue Flow (Reference Code)

**Files:**
- `transaction_flow_test.rs` - Reference implementation
- `QUEUE_FLOW_REFERENCE.md` - Detailed explanation
- `TRANSACTION_FLOW_DEMO.md` - Complete walkthrough

### ⚠️ Important: Cannot Compile on macOS

The example code **cannot be compiled on macOS** because:
- PolliNet depends on `btleplug` (BLE library)
- `btleplug` requires `dbus` (Linux-only)
- macOS doesn't have `dbus`

### This is Reference Code

The example serves as **documentation** showing:
- How fragmentation works
- How queues are managed (outbound → inbound → received → confirmation)
- How reassembly happens
- Complete transaction lifecycle

### How to Use This Example

#### Option 1: Read the Code (Recommended for macOS)

1. **Review `transaction_flow_test.rs`** - See the queue operations
2. **Read `QUEUE_FLOW_REFERENCE.md`** - Understand the flow with examples
3. **Check expected output** - Know what should happen

#### Option 2: Test on Android (Production Implementation)

```bash
cd pollinet-android
./gradlew installDebug
```

Use the **MWA Transaction Demo** screen:
- Create transaction with random amount/recipient
- Send via BLE to another device
- Receiver automatically reassembles and submits
- Confirmation relayed back to sender

#### Option 3: Run on Linux (If Available)

```bash
# Install dependencies
sudo apt install libdbus-1-dev pkg-config

# Run the example
cargo run --example transaction_flow_test
```

## What the Example Demonstrates

### Queue Flow

```
Create TX → Fragment → Outbound Queue → Dequeue
                                          ↓
Confirmation ← Received Queue ← Reassemble ← Inbound Buffers
```

### Key Operations

1. **`queue_transaction()`** - Fragment and add to outbound queue
2. **`next_outbound()`** - Dequeue fragment for transmission
3. **`push_inbound()`** - Add received fragment to reassembly buffers
4. **`metrics()`** - Check reassembly status
5. **`next_received_transaction()`** - Get complete reassembled transaction
6. **`queue_confirmation()`** - Add signature to confirmation queue
7. **`next_confirmation()`** - Get confirmation for relay

### Expected Output

For a 282-byte transaction:
- ✅ **1 fragment** (fits in single BLE packet)
- ✅ **Instant reassembly** (1/1 complete immediately)
- ✅ **Moves to received queue**
- ✅ **Confirmation queued**

## Other Examples

See the root `examples/` directory for:
- Offline transaction creation
- Nonce account management  
- MWA integration
- SPL token transfers

These may also have platform-specific requirements.

