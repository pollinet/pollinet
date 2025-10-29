# GATT Server Implementation - Testing Guide

## ✅ **What Was Implemented**

### **GATT Server with Custom Characteristics**

The PolliNet BLE adapter now includes a **full GATT server** with custom characteristics:

```
Service: 7E2A9B1F-4B8C-4D93-BB19-2C4EAC4E12A7 (PolliNet Service)
├── TX Characteristic: 7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12a8
│   ├── Properties: Write, Write Without Response
│   └── Purpose: Receive data from client (Central writes to Peripheral)
│
└── Status Characteristic: 7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12aa
    ├── Properties: Read
    └── Purpose: Return connection status (0x01 = ready)
```

### **Implementation Details**

- **File**: `src/ble/linux.rs`
- **Method**: `start_gatt_server()` - Called automatically when advertising starts
- **Integration**: GATT server is registered with BlueZ before advertising begins
- **Handler**: BlueZ automatically routes GATT write events through D-Bus

---

## 🧪 **Testing Steps**

### **Step 1: Test with LightBlue (BLE Scanner App)**

This verifies the GATT server is working and characteristics are visible.

1. **Start the sender or receiver**:
   ```bash
   cargo run --example offline_transaction_sender
   ```

2. **Open LightBlue** on your phone or another device

3. **Scan for BLE devices**

4. **Find "PolliNet" device** and tap to connect

5. **Verify you now see**:
   ```
   Service: 7E2A9B1F-4B8C-4D93-BB19-2C4EAC4E12A7
   
   Characteristics:
   ✅ 7E2A9B1F-4B8C-4D93-BB19-2C4EAC4E12A8 (TX - Writable)
   ✅ 7E2A9B1F-4B8C-4D93-BB19-2C4EAC4E12AA (Status - Readable)
   ```

6. **Test Write Operation**:
   - Tap on TX characteristic (`...12a8`)
   - Choose "Write"
   - Enter hex data: `48656C6C6F` (Hello in ASCII)
   - Send
   - Check terminal logs for: "📥 GATT TX characteristic received X bytes"

7. **Test Read Operation**:
   - Tap on Status characteristic (`...12aa`)
   - Choose "Read"
   - Should return: `01` (ready status)

**Expected Result**: ✅ Characteristics are visible and writable (unlike before where the service was empty)

---

### **Step 2: Test End-to-End Data Transfer**

This verifies transactions can be sent between sender and receiver.

#### **Machine 1 (Sender)**:
```bash
cargo run --example offline_transaction_sender
```

**Expected Logs**:
```
✅ BLE state reset - cleared all previous connections
🔧 Registering GATT server...
✅ GATT server started successfully
   📥 TX Characteristic (writable): 7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12a8
   📊 Status Characteristic (readable): 7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12aa
📢 BLE advertising started successfully on Linux with GATT server
🔍 Scanning started to detect receiver
⏳ Still waiting for receiver connection... (5s, 0 connected)
📊 Connected clients: 0 outbound, 1 inbound, 1 total
✅ Receiver has connected!
📤 Fragmenting compressed transaction using SDK...
📤 Writing X bytes to device: 90:65:84:5C:9B:2A
✅ Found writable characteristic: 7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12a8
✅ All fragments sent successfully via GATT
```

#### **Machine 2 (Receiver)**:
```bash
cargo run --example offline_transaction_receiver
```

**Expected Logs**:
```
✅ BLE state reset - cleared all previous connections
🔧 Registering GATT server...
✅ GATT server started successfully
📢 BLE advertising and scanning started fresh
🔍 Discovery attempt #1/20
✅ Connected to sender: 90:65:84:5C:9B:2A
⏳ Waiting for transaction fragments...
📥 Received fragment 1/1 for transaction: abc123...
✅ Transaction reassembled successfully
📤 Submitting transaction to Solana...
```

**Success Criteria**:
1. ✅ Sender detects receiver connection (inbound)
2. ✅ Sender finds writable TX characteristic on receiver
3. ✅ Sender successfully writes fragments via GATT
4. ✅ Receiver receives and processes fragments
5. ✅ Transaction is submitted to Solana

---

## 🔍 **Troubleshooting**

### **Issue 1: "Failed to start GATT server"**

**Symptoms**:
```
⚠️  Failed to start GATT server: ...
```

**Possible Causes**:
- BlueZ daemon not running with experimental features
- Permission issues

**Fix**:
```bash
# Restart BlueZ with experimental features
sudo systemctl stop bluetooth
sudo bluetoothd --experimental &
```

---

### **Issue 2: Characteristics Still Not Visible**

**Symptoms**: LightBlue shows service but no characteristics

**Possible Causes**:
- GATT server failed to register
- BlueZ caching old service info

**Fix**:
```bash
# Clear BlueZ cache
sudo systemctl stop bluetooth
sudo rm -rf /var/lib/bluetooth/*
sudo systemctl start bluetooth

# Restart your app
cargo run --example offline_transaction_sender
```

---

### **Issue 3: Write Operation Fails**

**Symptoms**: LightBlue shows "Write Failed" when trying to write to TX characteristic

**Check**:
1. Verify characteristic UUID is correct: `7e2a9b1f-4b8c-4d93-bb19-2c4eac4e12a8`
2. Verify permissions (should show "Write" and "Write Without Response")
3. Check terminal logs for error messages

---

### **Issue 4: Data Not Received**

**Symptoms**: Write succeeds in LightBlue but no log on device

**Possible Causes**:
- Receive callback not set up
- BlueZ not routing write events

**Debug**:
```bash
# Monitor BlueZ D-Bus messages
dbus-monitor --system "type='method_call',interface='org.bluez.GattCharacteristic1'"
```

Look for `WriteValue` method calls with your data.

---

## 📊 **Verification Checklist**

### **GATT Server**:
- [ ] GATT server starts without errors
- [ ] TX characteristic appears in LightBlue
- [ ] Status characteristic appears in LightBlue
- [ ] TX characteristic shows "Write" property
- [ ] Status characteristic shows "Read" property

### **Data Transfer**:
- [ ] Can write to TX characteristic from LightBlue
- [ ] Device logs show received data
- [ ] Sender finds TX characteristic on receiver
- [ ] Sender writes fragments successfully
- [ ] Receiver processes fragments

### **End-to-End**:
- [ ] Sender detects receiver connection
- [ ] Transaction fragments sent via GATT
- [ ] Receiver reassembles transaction
- [ ] Transaction submitted to Solana
- [ ] No "Characteristic not found" errors

---

## 📝 **Next Steps After Testing**

### **If GATT Server Works**:
1. ✅ Mark `gatt-3` and `gatt-4` as completed
2. Test with real Solana transactions
3. Add error handling and retries
4. Implement MTU negotiation for large packets

### **If Issues Persist**:
1. Check BlueZ version: `bluetoothctl --version` (need 5.50+)
2. Verify experimental features are enabled
3. Check D-Bus permissions
4. Review BlueZ logs: `journalctl -u bluetooth -f`

---

## 🎯 **Success Indicators**

You'll know it's working when:
1. **LightBlue shows 2 characteristics** (not empty service)
2. **Sender logs**: "✅ Found writable characteristic: ...12a8"
3. **Sender logs**: "✅ All fragments sent successfully via GATT"
4. **Receiver logs**: "📥 Received fragment X/Y"
5. **No more**: "❌ Failed to find writable characteristic"

---

## 🚀 **Quick Test Command**

Run both in separate terminals:

```bash
# Terminal 1
cargo run --example offline_transaction_sender 2>&1 | grep -E "(GATT|characteristic|fragment|Connected clients)"

# Terminal 2  
cargo run --example offline_transaction_receiver 2>&1 | grep -E "(GATT|characteristic|fragment|Received)"
```

This filters logs to show only GATT-related messages.

---

**Status**: ✅ GATT Server Implemented  
**Next**: Test with LightBlue to verify characteristics appear  
**Goal**: End-to-end transaction transfer via BLE GATT

