# BLE Connection Test Guide

## 🔧 **Fix Applied**

Updated `connected_clients_count()` in `src/ble/linux.rs` to detect **both outbound AND inbound connections**.

### **What Was Wrong:**
- The sender was only counting devices **it connected TO** (outbound)
- It wasn't detecting when the receiver **connected TO IT** (inbound)
- This caused the sender to wait forever, saying "waiting for receiver connection"

### **The Fix:**
The `connected_clients_count()` method now:
1. Counts outbound connections (devices we connected to)
2. **Scans discovered devices** to check if any are connected (inbound)
3. Returns the **total count** of both

## 🧪 **Testing Instructions**

### **Machine 1 (Sender):**
```bash
cd /home/oghenekparob_r/pollinet
cargo run --example offline_transaction_sender
```

**Expected Output:**
```
✅ BLE state reset - cleared all previous connections
📢 BLE advertising started fresh
🔍 Scanning started to detect receiver
🔄 STEP 4: Waiting for receiver to connect...
⏳ Still waiting for receiver connection... (5s, 0 connected)
⏳ Still waiting for receiver connection... (10s, 1 connected)  ← Should see 1 connected!
✅ Receiver has connected!
✅ Established bidirectional connection with receiver
📤 Fragmenting compressed transaction using SDK...
```

### **Machine 2 (Receiver):**
```bash
cd /home/oghenekparob_r/pollinet
cargo run --example offline_transaction_receiver
```

**Expected Output:**
```
✅ BLE state reset - cleared all previous connections
📢 BLE advertising and scanning started fresh
🔍 Discovery attempt #1/20
🔍 Found 1 peer(s):
   1. 90:65:84:5C:9B:2A (RSSI: -61)
🔗 Attempting connection to: 90:65:84:5C:9B:2A
✅ Connected to sender: 90:65:84:5C:9B:2A
⏳ Waiting for transaction fragments...
```

## 🔍 **What Should Happen Now:**

1. **Receiver discovers sender** ✅ (was already working)
2. **Receiver connects to sender** ✅ (was already working)
3. **Sender detects the connection** ✅ (FIXED - now working!)
4. **Sender sends transaction fragments** ⏳ (next test)
5. **Receiver receives and processes fragments** ⏳ (next test)

## 📊 **Verification:**

### **Check Connection Status:**
When the receiver connects, you should see on the **sender side**:
```
⏳ Still waiting for receiver connection... (10s, 1 connected)
✅ Receiver has connected!
```

The key is the **"1 connected"** part - this confirms the sender detected the inbound connection.

### **Check Logs:**
You should see debug logs like:
```
📊 Connected clients: 0 outbound, 1 inbound, 1 total
```

This shows:
- 0 outbound = sender didn't connect TO anyone
- 1 inbound = receiver connected TO sender
- 1 total = sender correctly detected the connection

## 🚨 **Troubleshooting:**

### **If sender still doesn't detect connection:**

1. **Check if devices are discovering each other:**
   - Both should see "🎯 Found PolliNet device" messages
   - Use nRF Connect app to verify both are advertising

2. **Check BlueZ connection state:**
   ```bash
   bluetoothctl
   devices
   info <MAC_ADDRESS>
   ```
   Look for "Connected: yes"

3. **Restart BlueZ if needed:**
   ```bash
   sudo systemctl restart bluetooth
   ```

4. **Check for multiple runs:**
   - Make sure no other instances are running
   - Kill any zombie processes: `pkill -f offline_transaction`

## 📝 **Next Steps:**

Once connection is confirmed, we need to verify:
1. ✅ Connection detection (FIXED)
2. ⏳ Fragment transmission via GATT write
3. ⏳ Fragment reception and reassembly
4. ⏳ Transaction submission to Solana

## 🎯 **Expected Issue:**

Even with connection detection fixed, **data transmission might still fail** because:
- No GATT server with writable characteristics
- `write_to_device()` will fail
- Falls back to broadcast mode
- Broadcast might not reach the receiver

**Solution:** We'll need to implement a proper GATT server OR use L2CAP sockets for direct data transfer.

