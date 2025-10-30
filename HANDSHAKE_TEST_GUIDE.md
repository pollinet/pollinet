# BLE Connection Handshake Test Guide

## 🎯 What Changed

We implemented a **two-way handshake protocol** to ensure both sender and receiver are fully ready before transmitting transaction data.

### The Problem We Fixed
- **Before**: Sender transmitted immediately after connection, but receiver was still setting up GATT (6-7 seconds)
- **Result**: Data was sent before receiver was listening → data lost

### The Solution
1. Sender connects and sends `POLLINET_READY?` message
2. Receiver completes GATT setup, receives ready check
3. Receiver responds with `POLLINET_READY!` confirmation
4. Sender waits for confirmation (up to 30 seconds)
5. **Only then** does sender transmit transaction fragments

---

## 🧪 Testing Instructions

### Step 1: Rebuild Both Examples

```bash
cd /home/oghenekparob_r/pollinet
cargo build --examples --release
```

### Step 2: Start Receiver FIRST (Terminal 1)

```bash
cd /home/oghenekparob_r/pollinet
cargo run --example offline_transaction_receiver
```

**Expected receiver output:**
```
🔄 Resetting BLE state...
✅ BLE adapter initialized
📡 Starting as BLE peripheral (receiver mode)...
🎧 Receiver is advertising and waiting for sender...
```

### Step 3: Start Sender (Terminal 2)

```bash
cd /home/oghenekparob_r/pollinet
cargo run --example offline_transaction_sender
```

**Expected sender output:**
```
📡 Starting as BLE peripheral (sender mode)...
⏳ Waiting for receiver connection...
✅ Receiver connected!
🤝 Performing connection handshake...
   Sending READY check to receiver...
⏳ Waiting for receiver ready confirmation...
✅ Receiver confirmed ready!
⏳ Waiting 2 seconds for receiver to prepare...
📤 Sending transaction fragments...
📤 Using broadcast mode for 1 fragments (bypassing GATT MTU limitation)
✅ Transaction fragments sent successfully!
```

**Expected receiver output (after sender starts):**
```
✅ Sender connected: <MAC_ADDRESS>
🤝 Waiting for sender handshake...
✅ Received handshake from sender!
📤 Sending ready confirmation...
✅ Ready confirmation sent!
⏳ Preparing to receive data...
📡 Listening for transaction fragments...
📨 Received transaction data: XXX bytes
✅ Transaction received and validated!
```

---

## ✅ Success Criteria

### 1. **Handshake Completes**
- Sender logs: `✅ Receiver confirmed ready!`
- Receiver logs: `✅ Ready confirmation sent!`

### 2. **Broadcast Mode Used**
- Sender logs: `📤 Using broadcast mode for X fragments`
- This means GATT MTU limitation is bypassed

### 3. **Full Data Received**
- Receiver logs: `📨 Received transaction data: XXX bytes`
- Should be **200+ bytes**, NOT just 4 bytes
- If you see `📨 Received GATT notification: 4 bytes`, broadcast mode isn't working

### 4. **Transaction Validated**
- Receiver logs: `✅ Transaction decompressed successfully`
- Receiver logs: `✅ Transaction deserialized successfully`
- Receiver logs: `📤 Transaction submitted to Solana`

---

## 🐛 Troubleshooting

### Issue: "Timeout waiting for sender handshake"
**Cause**: Sender didn't connect or didn't send READY? message  
**Fix**: Make sure sender started after receiver was advertising

### Issue: "Receiver did not confirm ready state"
**Cause**: Receiver didn't get the READY? message or couldn't send response  
**Fix**: Check if both devices can send/receive text messages

### Issue: Still seeing "4 bytes" received
**Cause**: Broadcast mode isn't being used (old code still running)  
**Fix**: 
1. Kill all running instances: `pkill -f offline_transaction`
2. Rebuild: `cargo build --examples --release`
3. Start fresh

### Issue: Connection fails with "br-connection-unknown"
**Cause**: Previous connection not cleaned up  
**Fix**:
```bash
sudo systemctl restart bluetooth
# Wait 5 seconds
bluetoothctl power off
bluetoothctl power on
```

---

## 📊 What to Share

After testing, please share:

1. **Full sender terminal output** (from start to finish)
2. **Full receiver terminal output** (from start to finish)
3. **Confirm these key points:**
   - Did handshake complete? (both sides confirmed)
   - Was broadcast mode used? (sender logs)
   - How many bytes received? (receiver logs)
   - Did transaction validate? (receiver logs)

---

## 🎓 Technical Details

### Handshake Protocol
```
Sender                          Receiver
  |                                |
  |---- BLE Connection -------->   |
  |                                | (Setting up GATT...)
  |                                |
  |---- "POLLINET_READY?" ------>  |
  |                                | (GATT ready!)
  |<--- "POLLINET_READY!" ------   |
  |                                |
  | (Wait 2s)                      | (Wait 1s)
  |                                |
  |---- Transaction Data ------->  |
  |                                |
```

### Why 2 Waits?
- **Sender waits 2s**: Ensures receiver has time to set up buffers for incoming data
- **Receiver waits 1s**: Ensures ready confirmation is fully transmitted before data flood

This prevents the race condition where data arrives before receiver is listening!

