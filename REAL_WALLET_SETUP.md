# Real Wallet Integration for Testing

## ✅ Status: CONFIGURED

The PolliNet Android app now uses your **real funded wallet** for all offline bundle demos and tests.

---

## 🔑 Wallet Configuration

### Private Key (Base58)
```
5zRwe731N375MpGuQvQoUjSMUpoXNLqsGWE9J8SoqHKfivhUpNxwt3o9Gdu6jjCby4dJRCGBA6HdBzrhvLVhUaqu
```

### Where It's Used
1. **Offline Bundle Demo** (Diagnostics Screen)
   - Step 1: Prepare Offline Bundle
   - Step 2: Create Offline Transaction
   - Step 3: Submit Transaction

2. **File Location**
   - `pollinet-android/app/src/main/java/xyz/pollinet/android/ui/DiagnosticsScreen.kt`
   - Lines: 591, 665 (in `OfflineBundleDemo` composable)

---

## 📱 How to Test

### 1. Open the App
- App should already be installed on your device
- Launch: PolliNet Android

### 2. Go to Diagnostics Tab
- Tap "Diagnostics" in bottom navigation
- Scroll to "🚀 Offline Bundle Demo (Core PolliNet)"

### 3. Run the Complete Flow

#### Step 1: Prepare Bundle
**Tap**: "1️⃣ Prepare Offline Bundle (3 nonces)"

**What happens:**
- Creates 3 nonce accounts on Solana devnet
- Uses your **real wallet** (costs ~$0.60 in devnet SOL)
- Caches nonce data for offline use

**Expected logs:**
```
[HH:MM:SS] 📦 Step 1: Preparing Offline Bundle...
[HH:MM:SS]    Creating 3 nonce accounts for offline use
[HH:MM:SS]    Cost: 3 × $0.20 = $0.60 (first time)
[HH:MM:SS] ✓ Bundle prepared!
[HH:MM:SS]   Total nonces: 3
[HH:MM:SS]   Available: 3
[HH:MM:SS]   Ready for offline transaction creation!
```

#### Step 2: Create Offline Transaction
**Tap**: "2️⃣ Create Transaction (Offline)"

**What happens:**
- Creates a signed transaction **completely offline**
- NO internet/RPC calls required
- Uses cached nonce from Step 1
- Signs with your real wallet

**Expected logs:**
```
[HH:MM:SS] 📴 Step 2: Creating Transaction OFFLINE...
[HH:MM:SS]    NO INTERNET REQUIRED!
[HH:MM:SS]    Using cached nonce data
[HH:MM:SS] ✓ Transaction created OFFLINE!
[HH:MM:SS]   Size: XXX chars (base64)
[HH:MM:SS]   Ready for BLE transmission
[HH:MM:SS]   Can submit when back online
```

#### Step 3: Submit Transaction
**Tap**: "3️⃣ Submit Transaction (Online)"

**What happens:**
- Submits the offline-created transaction to Solana
- Verifies nonce is still valid
- **Actually sends 0.001 SOL** (1,000,000 lamports) to test recipient

**Expected logs (SUCCESS):**
```
[HH:MM:SS] 🌐 Step 3: Submitting Transaction...
[HH:MM:SS]    Back online - submitting to blockchain
[HH:MM:SS] ✓ Transaction submitted!
[HH:MM:SS]   Signature: 5Xy7z8A9B1C2D3E4...
[HH:MM:SS]   🎉 Complete offline → online flow!
```

**OR (if insufficient balance):**
```
[HH:MM:SS] ✗ Submit failed: insufficient funds for transaction
```

---

## 🔧 Base58 Decoding

The app includes a custom `decodeBase58()` function to convert your private key from base58 to bytes:

```kotlin
private fun decodeBase58(input: String): ByteArray {
    val ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    // ... decoding logic ...
}
```

**Used in:**
- `OfflineBundleDemo` composable (line 592, 666)
- Converts base58 private key → 64-byte keypair

---

## 💰 Wallet Requirements

### For Full Testing:
1. **Devnet SOL Balance**: ~1 SOL recommended
   - Nonce creation: ~0.60 SOL (3 accounts × 0.2 SOL)
   - Transaction fees: ~0.00001 SOL per transaction
   - Test transfers: 0.001 SOL per transaction

2. **Get Devnet SOL**:
   ```bash
   # Using Solana CLI
   solana airdrop 2 <YOUR_PUBLIC_KEY> --url devnet
   
   # Or use the web faucet
   # https://faucet.solana.com/
   ```

---

## 🔐 Security Notes

### ⚠️ IMPORTANT
- This private key is **hardcoded in the app source code**
- **ONLY use for testing on devnet**
- **NEVER use this wallet on mainnet**
- **NEVER put real funds on this wallet**

### For Production:
- Remove hardcoded private keys
- Integrate Solana Mobile Wallet Adapter (M10)
- Use Android Keystore for local key management
- Request user authorization for transactions

---

## 🧪 What This Proves

### Core PolliNet Capabilities:
1. ✅ **Offline Transaction Creation**
   - Step 2 works with **zero network calls**
   - Transaction signed locally using cached nonce

2. ✅ **Smart Bundle Management**
   - Nonce accounts created once
   - Can be reused across multiple transactions
   - Automatic refresh when nonces are consumed

3. ✅ **BLE/Mesh Ready**
   - Compressed transactions (base64 encoded)
   - Small payloads for BLE transmission
   - Ready for fragmentation and mesh propagation

---

## 📊 Testing Checklist

- [ ] App launches successfully
- [ ] Navigate to Diagnostics tab
- [ ] Tap "Prepare Offline Bundle" - succeeds
- [ ] See bundle status card appear
- [ ] Tap "Create Transaction (Offline)" - succeeds instantly
- [ ] See offline transaction ready card appear
- [ ] Tap "Submit Transaction" - check logs for result
- [ ] Transaction either succeeds or shows balance error
- [ ] Check "Test Logs" section for detailed output
- [ ] Tap "Reset Demo" and run again

---

## 🐛 Troubleshooting

### "✗ Bundle failed: ..."
- **Check**: Wallet has devnet SOL (~1 SOL)
- **Check**: Internet connection for RPC calls
- **Solution**: Get devnet SOL from faucet

### "✗ Transaction failed: invalid blockhash"
- **Reason**: Cached nonce was used by another transaction
- **Solution**: Run Step 1 again to get fresh nonces

### "✗ Submit failed: insufficient funds"
- **Reason**: Wallet doesn't have enough SOL for transfer
- **Solution**: This is expected if wallet is empty - Step 2 still proves offline creation works!

---

## 📖 Related Files

- **`DiagnosticsScreen.kt`** - UI implementation
- **`PolliNetSDK.kt`** - Kotlin wrappers for offline bundle functions
- **`src/ffi/android.rs`** - Rust FFI bindings
- **`OFFLINE_BUNDLE_DEMO_COMPLETE.md`** - Full feature documentation

---

**Last Updated**: 2025-11-05  
**Wallet Type**: Devnet Test Wallet (Base58-encoded)  
**Status**: ✅ **CONFIGURED & READY FOR TESTING**

