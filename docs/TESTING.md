# PolliNet Android Testing Guide

This document outlines how to test all features of the PolliNet Android app.

## 🎯 What's Already Working

Based on the successful build and launch:
- ✅ **FFI Bridge**: Native Rust library (`libpollinet.so`) loaded successfully
- ✅ **BLE Service**: Foreground service running with proper permissions
- ✅ **GATT Server**: Bluetooth GATT service registered (Service UUID: 00001820-...)
- ✅ **Permissions**: Runtime BLE permission handling on Android 12+
- ✅ **UI**: Compose-based diagnostics, transaction builder, and signing screens

## 📱 Running Manual Tests

### Test 1: FFI Communication (SDK Version)

**What it tests**: Rust ↔ Kotlin FFI bridge  
**Expected result**: SDK version displayed without crash

**Steps**:
1. Open the app
2. Navigate to **Diagnostics** tab (bottom navigation)
3. Look for "SDK Version: X.X.X" at the top
4. Check **Test Logs** section for: `✓ FFI initialized, version: X.X.X`

**What to verify**:
- Version number is displayed (e.g., "0.1.0")
- No crash or "Unknown" error

---

### Test 2: SDK Initialization & Metrics

**What it tests**: Full Rust SDK initialization and metrics retrieval  
**Expected result**: SDK initializes and returns metrics

**Steps**:
1. In **Diagnostics** tab, scroll to "FFI Tests" section
2. Tap **"Test SDK Init"** button
3. Wait 2-3 seconds
4. Check Test Logs for results

**Expected logs**:
```
[HH:MM:SS] Testing SDK initialization...
[HH:MM:SS] ✓ SDK initialized successfully
[HH:MM:SS] Testing metrics...
[HH:MM:SS] ✓ Metrics retrieved:
[HH:MM:SS]   Fragments: 0
[HH:MM:SS]   Completed: 0
```

**What to verify**:
- Green checkmarks (✓) appear
- No red crosses (✗)
- Metrics show `Fragments: 0`, `Completed: 0` (initial state)

---

### Test 3: Transaction Builder

**What it tests**: Solana transaction creation via Rust FFI  
**Expected result**: Unsigned transaction is built

**Steps**:
1. In **Diagnostics** tab, tap **"Test Transaction Builder"**
2. Wait 2-3 seconds
3. Check Test Logs

**Expected logs**:
```
[HH:MM:SS] Testing transaction builder...
[HH:MM:SS] ✓ Transaction created:
[HH:MM:SS]   AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA...
[HH:MM:SS]   Length: XXX chars
```

**What to verify**:
- Transaction base64 string is generated
- No errors about invalid addresses or RPC issues

---

### Test 4: BLE Transport Layer

**What it tests**: Push data to Rust transport, verify metrics update  
**Expected result**: Data accepted, metrics updated

**Steps**:
1. In **Diagnostics** tab, tap **"Test BLE Transport"**
2. Check Test Logs

**Expected logs**:
```
[HH:MM:SS] Testing BLE transport...
[HH:MM:SS] ✓ Pushed test data to transport
[HH:MM:SS]   Fragments buffered: 0
```

**What to verify**:
- Test data is pushed successfully
- Fragments buffered count (may be 0 if data was too small to be a fragment)

---

### Test 5: BLE Scanning

**What it tests**: Bluetooth LE scanner can start/stop  
**Expected result**: Scanner starts without crash

**Steps**:
1. In **Diagnostics** tab, tap **"Start Scan"** button
2. Wait 2 seconds
3. Tap **"Stop Scan"**
4. Check logcat: `adb logcat | grep "BLE\|Bluetooth"`

**What to verify**:
- No SecurityException errors
- "BLE State" shows scanning status

---

### Test 6: BLE Advertising

**What it tests**: Bluetooth LE advertising can start/stop  
**Expected result**: Device advertises PolliNet service

**Steps**:
1. In **Diagnostics** tab, tap **"Start Advertise"** button
2. Use a BLE scanner app (e.g., nRF Connect) on another device
3. Look for service UUID: `00001820-0000-1000-8000-00805f9b34fb`
4. Tap **"Stop Advertise"**

**What to verify**:
- PolliNet service is discoverable by other devices
- TX characteristic (UUID: 00001821-...) is readable
- RX characteristic (UUID: 00001822-...) is writable

---

### Test 7: Transaction Builder UI

**What it tests**: UI for building SOL and SPL transactions  
**Expected result**: Transactions can be created via UI

**Steps**:
1. Navigate to **Build Tx** tab
2. Fill in:
   - **From**: Any valid Solana address
   - **To**: Any valid Solana address  
   - **Amount**: 0.001 (for SOL) or 1 (for SPL)
3. For SPL: Toggle "SPL Token Transfer" and add mint address
4. Tap **"Create Transaction"**
5. Check status message

**What to verify**:
- Transaction is created without crash
- Base64 transaction string appears
- Can copy transaction to clipboard

---

### Test 8: Keystore Manager (Signing)

**What it tests**: Android Keystore key generation and signing  
**Expected result**: Keys are generated, signatures created

**Steps**:
1. Navigate to **Sign Tx** tab
2. Tap **"Generate New Keypair"**
3. View the generated public key
4. Enter test message (e.g., "Hello PolliNet")
5. Tap **"Sign Message"**

**What to verify**:
- Public key is displayed (base64 or hex)
- Signature is generated
- No KeyStore errors

**Note**: Current implementation uses ECDSA (for demo), not Ed25519 (Solana-compatible). This is expected.

---

## 🔍 Advanced Testing

### Test 9: Two-Device BLE Mesh (E2E Test)

**Requirements**: 2 Android devices with BLE

**Setup**:
1. Install app on both devices
2. Grant BLE permissions on both

**Steps**:
1. **Device A**: Start Advertising
2. **Device B**: Start Scanning
3. **Device B**: Should discover Device A
4. **Device B**: Connect to Device A (if auto-connect implemented)
5. **Device A**: Send test transaction fragment
6. **Device B**: Verify fragment received in metrics

**What to verify**:
- Devices discover each other
- GATT connection established
- Data transfer works bidirectionally
- Metrics show fragments sent/received

---

### Test 10: Transaction Fragmentation

**What it tests**: Large transactions are fragmented correctly

**Steps**:
1. Create a large transaction (e.g., with memo)
2. Use `queueTransaction()` API
3. Monitor metrics for fragment count
4. Verify reassembly on receiver

**Expected**:
- Large transactions split into 512-byte fragments
- Each fragment has proper header (seq, total, tx_id)
- Receiver reassembles successfully

---

## 🐛 Debugging

### View Live Logs

```bash
adb logcat | grep -E "(PolliNet|FFI|BLE)"
```

### Check Native Crashes

```bash
adb logcat | grep -E "(FATAL|backtrace)"
```

### Verify Native Library Loaded

```bash
adb logcat | grep "libpollinet.so"
```

**Expected**: `Load /data/app/.../libpollinet.so ... ok`

### Check BLE Service Status

```bash
adb shell dumpsys activity services xyz.pollinet.sdk.BleService | grep "ServiceRecord"
```

### Check GATT Server Registration

```bash
adb logcat | grep "successfully registered service"
```

---

## ✅ Test Results Template

Use this checklist to track your testing:

- [ ] Test 1: FFI Communication (SDK Version)
- [ ] Test 2: SDK Initialization & Metrics
- [ ] Test 3: Transaction Builder (Rust FFI)
- [ ] Test 4: BLE Transport Layer
- [ ] Test 5: BLE Scanning
- [ ] Test 6: BLE Advertising
- [ ] Test 7: Transaction Builder UI
- [ ] Test 8: Keystore Manager (Signing)
- [ ] Test 9: Two-Device BLE Mesh (E2E)
- [ ] Test 10: Transaction Fragmentation

---

## 🚨 Common Issues

### Issue: "BLE service not available"
**Solution**: Ensure BLE permissions are granted. Check Diagnostics tab → Permissions status.

### Issue: FFI test fails with "SDK init failed"
**Possible causes**:
- RPC URL unreachable (check network)
- Invalid Rust configuration
- Native library not loaded

**Debug**: Check `adb logcat | grep "pollinet"`

### Issue: BLE scanning doesn't find devices
**Possible causes**:
- Location permission not granted (Android 10-11)
- Bluetooth is off
- No devices nearby are advertising

### Issue: Transaction builder fails
**Possible causes**:
- Invalid Solana address format
- RPC endpoint down
- Network connectivity issues

---

## 📊 Performance Metrics

Monitor these in the **Diagnostics** tab:

- **Fragments Buffered**: Should stay low (<10) under normal operation
- **Transactions Complete**: Increments when full transactions are reassembled
- **Reassembly Failures**: Should remain 0 (indicates dropped fragments)
- **Last Error**: Empty string = no errors

---

## 🎯 Next Steps After Testing

Based on test results:
1. **All tests pass?** → Proceed to Solana Mobile Wallet Adapter integration
2. **FFI tests fail?** → Debug Rust/JNI bindings
3. **BLE tests fail?** → Check permissions and Android Bluetooth stack
4. **UI tests fail?** → Review Compose state management

Report any issues with specific test logs and device information.

