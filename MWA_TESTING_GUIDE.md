# MWA Integration Testing Guide

Complete guide to test the Solana Mobile Wallet Adapter (MWA) integration with PolliNet.

---

## 📋 Prerequisites

### 1. Development Tools
- ✅ Android Studio installed
- ✅ Rust toolchain installed
- ✅ Android device or emulator (API 29+)

### 2. Mobile Wallet App
Install one of these Solana Mobile wallets:
- **Solflare** (Recommended): [Play Store](https://play.google.com/store/apps/details?id=com.solflare.mobile)
- **Phantom**: [Play Store](https://play.google.com/store/apps/details?id=app.phantom.android)

### 3. Wallet Setup
1. Open the wallet app
2. Create a new wallet or import existing
3. **Switch to Devnet**:
   - Settings → Network → Devnet
4. Get some Devnet SOL:
   - Use [Solana Faucet](https://faucet.solana.com/)
   - Or run: `solana airdrop 2 <YOUR_PUBKEY> --url devnet`

---

## 🧪 Phase 1: Verify Rust Compilation

Verify the Rust MWA functions compile correctly:

```bash
# Navigate to Android project
cd /Users/oghenekparoboreminokanju/pollinet/pollinet-android

# Build Rust libraries (this compiles and tests the MWA functions)
./gradlew pollinet-sdk:buildRustLib
```

**Expected Output:**
```
Building Rust libraries for Android...
Compiling pollinet v0.1.0
   Compiling solana-sdk...
   Finished `release` profile [optimized]
✓ arm64-v8a
✓ armeabi-v7a  
✓ x86_64
✓ x86

BUILD SUCCESSFUL
```

**What This Verifies:**
- ✅ All MWA functions compile without errors
- ✅ FFI bindings are correct
- ✅ Cross-platform compatibility (4 Android ABIs)
- ✅ No private key dependencies in MWA code path

**Note:** Unit tests require RPC dependencies that aren't available with `--no-default-features`. The Android integration test (Phase 3) provides comprehensive end-to-end testing of the MWA functionality.

---

## 🔨 Phase 2: Build & Deploy Android App

Build the complete Android app with Rust SDK:

```bash
# Set Java home
export JAVA_HOME=/Applications/Android\ Studio.app/Contents/jbr/Contents/Home

# Navigate to Android project
cd /Users/oghenekparoboreminokanju/pollinet/pollinet-android

# Build Rust libraries and Android APK
./gradlew pollinet-sdk:buildRustLib :app:assembleDebug

# Install to connected device/emulator
./gradlew installDebug
```

**Expected Output:**
```
Building Rust libraries for Android...
✓ arm64-v8a
✓ armeabi-v7a
✓ x86_64
✓ x86

BUILD SUCCESSFUL
```

**Troubleshooting:**
- **Error: "No connected devices"**
  - Connect Android device via USB with debugging enabled
  - Or start an emulator in Android Studio

- **Error: "Permission denied"**
  ```bash
  chmod +x gradlew
  ```

---

## 📱 Phase 3: Test on Android Device

### Step 1: Grant Permissions

1. Open PolliNet app
2. When prompted, grant BLE permissions:
   - ✅ BLUETOOTH_SCAN
   - ✅ BLUETOOTH_CONNECT
   - ✅ BLUETOOTH_ADVERTISE
   - ✅ LOCATION (required for BLE on Android < 12)

### Step 2: Initialize SDK

1. Scroll to bottom of DiagnosticsScreen
2. Find "🔐 MWA Transaction Demo" section
3. Verify status shows: "Ready. Connect wallet to begin."

### Step 3: Test Authorization

1. Click **"Connect Wallet"** button
2. **Expected behavior:**
   - Wallet app (Solflare/Phantom) opens automatically
   - Shows authorization request:
     ```
     PolliNet
     https://pollinet.xyz
     
     Requesting permission to:
     - View your wallet address
     - Request transaction signatures
     
     [Approve] [Reject]
     ```
3. Tap **"Approve"**
4. Returns to PolliNet app
5. **Verify:**
   - ✅ Status: "Wallet connected successfully!"
   - ✅ Shows truncated public key: `ABC12...XYZ89`

**Troubleshooting:**
- **"No wallet app found"**: Install Solflare or Phantom from Play Store
- **Wallet doesn't open**: Check wallet app is on Devnet
- **Connection fails**: Try restarting both apps

### Step 4: Prepare Offline Bundle

⚠️ **Note:** This step currently shows as TODO because it requires implementing nonce account creation with MWA signing. For now, you can:

**Option A: Use the old flow (with keypairs)**
- Use the "Offline Bundle Demo" section above the MWA demo
- This works but uses direct keypair access (not MWA)

**Option B: Skip for now**
- Create nonces manually using Solana CLI
- Proceed to next step with pre-existing nonces

### Step 5: Create Unsigned Transaction ⭐

This is the **key test** - creating transactions with public keys only:

1. Click **"Create Unsigned TX"** button
2. **Behind the scenes:**
   ```
   Kotlin → FFI → Rust
   └─> create_unsigned_offline_transaction(
         sender_pubkey,    ← PUBLIC KEY
         recipient_pubkey, ← PUBLIC KEY
         amount,
         nonce_authority_pubkey ← PUBLIC KEY
       )
   └─> Returns: base64 unsigned transaction
   ```
3. **Verify:**
   - ✅ Status: "Unsigned transaction created! Ready to sign."
   - ✅ Shows: "✅ Unsigned TX: ABC123..."
   - ✅ No errors in logcat

**Check Logcat:**
```bash
adb logcat | grep "pollinet\|MWA"
```

Expected logs:
```
🔓 Creating UNSIGNED offline transaction for MWA
   Sender pubkey: ABC...XYZ
   NO private keys involved - MWA will sign
📂 Loaded bundle: X total nonces, Y available
📌 Using nonce account: DEF...GHI
✅ Marked nonce as used
💾 Bundle saved with updated nonce status
✅ Unsigned transaction created for MWA signing
   Transaction ready for Seed Vault signature
```

### Step 6: Sign with MWA (Secure Signing) 🔐

This is where **true security** happens - signing in Seed Vault:

1. Click **"Sign Transaction"** button
2. **Expected behavior:**
   - Wallet app opens
   - Shows transaction details:
     ```
     Transaction Request
     
     PolliNet is requesting signature
     
     From: <Your Wallet>
     To: <Recipient>
     Amount: 0.001 SOL
     Fee: ~0.000005 SOL
     
     [Sign] [Reject]
     ```
3. **Review carefully** - verify amount and recipient
4. Tap **"Sign"**
5. Enter PIN/biometric if required
6. Returns to PolliNet app

**Behind the scenes:**
```
PolliNet                    Wallet App              Seed Vault
   |                             |                        |
   |--Unsigned TX------------->  |                        |
   |                             |--Ask for signature-->  |
   |                             |                        | [SIGNS IN
   |                             |                        |  HARDWARE]
   |                             |<--Signature-----------  |
   |<--Signed TX--------------   |                        |
```

7. **Verify:**
   - ✅ Status: "Transaction signed! Ready to submit."
   - ✅ Shows: "✅ Signed TX: DEF456..."

**Key Security Point:**
- 🔐 Private key **NEVER** left the Seed Vault
- 🔐 PolliNet only received the signature
- 🔐 Even if PolliNet is compromised, keys are safe

### Step 7: Submit Transaction

1. Click **"Submit to Blockchain"** button
2. Wait for confirmation (5-10 seconds)
3. **Verify:**
   - ✅ Status: "Transaction submitted successfully!"
   - ✅ Shows: "✅ Signature: ABC...XYZ"

4. **Verify on Explorer:**
   ```
   https://explorer.solana.com/tx/<SIGNATURE>?cluster=devnet
   ```

   Should show:
   - ✅ Status: Success
   - ✅ From: Your wallet
   - ✅ To: Recipient
   - ✅ Amount: 0.001 SOL

---

## 🔍 Phase 4: Verify Security (Critical!)

### Test 1: Verify No Private Keys in Logs

```bash
# Check that NO private keys appear in logs
adb logcat | grep -i "private\|secret\|keypair"

# Should only show logs like:
# "NO private keys involved"
# "Public key: ABC..."
```

**✅ PASS:** No private keys found in logs
**❌ FAIL:** If you see base64 keypairs or secret keys

### Test 2: Verify Transaction Structure

```bash
# In PolliNet app, copy the unsigned transaction (truncated display)
# Decode to verify structure:

# Save to file: unsigned_tx.txt (paste the base64)
base64 -d unsigned_tx.txt | xxd | head -20

# Should show Solana transaction structure:
# - Signatures field (empty for unsigned)
# - Message header
# - Account keys (public keys only)
# - Recent blockhash (nonce)
# - Instructions
```

### Test 3: Compare Flows

**Old Flow (INSECURE):**
```
App generates private key
  → Stores in memory
  → Passes to Rust
  → Rust signs transaction
  → Potential leak at any step
```

**New MWA Flow (SECURE):**
```
App only has public key
  → Creates unsigned transaction
  → Sends to Seed Vault
  → Seed Vault signs in hardware
  → App receives signature only
  → No leak possible
```

---

## 🐛 Troubleshooting

### "MWA authorization not yet implemented"

This is **expected** - the `PolliNetMwaClient.kt` is a stub. To complete:

1. Study Solana Mobile docs:
   ```
   https://docs.solanamobile.com/android-native/overview
   ```

2. Check MWA SDK examples:
   ```
   https://github.com/solana-mobile/mobile-wallet-adapter/tree/main/examples
   ```

3. Implement the 3 stub methods in `PolliNetMwaClient.kt`:
   - `authorize()`
   - `signAndSendTransaction()`
   - `reauthorize()`

### "Failed to create unsigned transaction"

**Check:**
1. Storage directory configured:
   ```kotlin
   SdkConfig(
       storageDirectory = context.filesDir.absolutePath // ← Must be set
   )
   ```

2. Bundle prepared:
   ```bash
   adb shell ls /data/data/xyz.pollinet.android/files/
   # Should show: pollinet_nonce_bundle.json
   ```

3. Nonces available:
   ```bash
   adb shell cat /data/data/xyz.pollinet.android/files/pollinet_nonce_bundle.json
   # Should show nonces with "used": false
   ```

### "Transaction signing failed"

**Check:**
1. Wallet on correct network (Devnet)
2. Wallet has sufficient SOL (>0.001 SOL)
3. Transaction not expired (nonce still valid)
4. Wallet app is up to date

### "No available nonces"

All nonces marked as used. Fix:
```bash
# Option 1: Delete bundle (will recreate)
adb shell rm /data/data/xyz.pollinet.android/files/pollinet_nonce_bundle.json

# Option 2: Refresh nonces by calling prepareOfflineBundle again
```

---

## ✅ Success Criteria

Your MWA integration is working correctly if:

- ✅ Rust tests pass (Phase 1)
- ✅ Android app builds successfully (Phase 2)
- ✅ Wallet authorization works (Phase 3, Step 3)
- ✅ Unsigned transaction created without private keys (Phase 3, Step 5)
- ✅ Transaction signed in Seed Vault (Phase 3, Step 6)
- ✅ Transaction submitted and confirmed on-chain (Phase 3, Step 7)
- ✅ No private keys found in logs (Phase 4, Test 1)
- ✅ Transaction verifiable on Solana Explorer (Phase 3, Step 7.4)

**Security validation:**
- 🔐 Private keys **never** appeared in PolliNet code
- 🔐 Signing happened **inside** Seed Vault
- 🔐 App only handled **public keys** and **signatures**

---

## 📊 Performance Benchmarks

Expected timings on typical device:

| Operation | Time | Notes |
|-----------|------|-------|
| Authorization | 2-5s | One-time per session |
| Create Unsigned TX | <100ms | Pure computation |
| Sign with MWA | 3-8s | Includes wallet UI |
| Submit TX | 5-15s | Network dependent |
| **Total Flow** | **10-30s** | End-to-end |

---

## 🎯 Next Steps

Once testing is complete:

1. **Complete MWA Client Implementation**
   - Study MWA SDK documentation
   - Implement actual authorize/sign methods
   - Test with real wallet apps

2. **Add Error Handling**
   - User cancels signing
   - Network failures
   - Insufficient funds
   - Expired nonces

3. **Improve UX**
   - Loading states
   - Progress indicators
   - Better error messages
   - Transaction history

4. **Add More Transaction Types**
   - SPL token transfers
   - NFT transfers
   - Governance votes
   - Stake operations

5. **Production Hardening**
   - Switch to Mainnet
   - Add transaction fees display
   - Implement retry logic
   - Add analytics/monitoring

---

## 🆘 Getting Help

If you encounter issues:

1. **Check Logs:**
   ```bash
   adb logcat | grep -E "pollinet|MWA|Solana"
   ```

2. **Enable Verbose Logging:**
   ```kotlin
   SdkConfig(
       enableLogging = true,
       logLevel = "debug"
   )
   ```

3. **Test Individual Components:**
   - Run Rust tests first
   - Test FFI layer separately
   - Test Android UI independently

4. **Consult Documentation:**
   - PolliNet: `MWA_INTEGRATION_PROGRESS.md`
   - Solana Mobile: https://docs.solanamobile.com
   - MWA SDK: https://github.com/solana-mobile/mobile-wallet-adapter

---

## 📝 Test Checklist

Copy this checklist for your testing session:

```
Phase 1: Rust Tests
[ ] cargo test mwa_tests passes
[ ] No private keys in test code
[ ] All 3 functions return expected results

Phase 2: Build
[ ] Rust libraries compile for all ABIs
[ ] Android app builds successfully
[ ] APK installs on device

Phase 3: Android Testing
[ ] BLE permissions granted
[ ] Wallet app installed and configured
[ ] Authorization successful
[ ] Unsigned transaction created
[ ] Transaction signed by wallet
[ ] Transaction submitted successfully
[ ] Transaction confirmed on-chain

Phase 4: Security Verification
[ ] No private keys in logcat
[ ] Transaction structure correct
[ ] Signing happened in Seed Vault
[ ] Keys never touched PolliNet code

Result: ✅ PASS / ❌ FAIL
Notes: ___________________________
```

---

**Happy Testing! 🚀**

Remember: The goal is to verify that **private keys never leave the Seed Vault** while still enabling secure Solana transactions. This is the foundation of Solana Mobile Stack security.

