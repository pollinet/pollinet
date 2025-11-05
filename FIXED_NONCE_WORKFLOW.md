# Fixed Nonce Management Workflow

## 🐛 Problem Identified

The user correctly identified critical issues with the nonce management:

1. **`prepare_offline_bundle` was creating NEW nonces** instead of refreshing used ones
2. **`create_offline_transaction` didn't mark nonces as used** in storage
3. **Kotlin was sending nonce data** instead of Rust managing it from storage

## ✅ Solution Implemented

### Complete Workflow (Correct Implementation)

```
┌─────────────────────────────────────────────────────────────┐
│  1. PREPARE OFFLINE BUNDLE (First Time)                     │
├─────────────────────────────────────────────────────────────┤
│  User: Taps "Prepare Bundle (3 nonces)"                     │
│  Rust: Check storage → No bundle found                      │
│  Rust: Create 3 NEW nonce accounts → Cost: $0.60           │
│  Rust: Save bundle to storage                               │
│        {                                                     │
│          nonceCaches: [                                      │
│            { nonceAccount: "...", used: false },           │
│            { nonceAccount: "...", used: false },           │
│            { nonceAccount: "...", used: false }            │
│          ]                                                   │
│        }                                                     │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│  2. CREATE OFFLINE TRANSACTION                               │
├─────────────────────────────────────────────────────────────┤
│  User: Taps "Create Transaction (Offline)"                  │
│  Rust: Load bundle from storage                              │
│  Rust: Pick first unused nonce (nonce #1)                   │
│  Rust: Mark nonce #1 as used = true                         │
│  Rust: Save updated bundle                                   │
│  Rust: Create transaction with nonce #1                     │
│  Rust: Return transaction (NOT bundle)                      │
│                                                               │
│  Storage now:                                                │
│        {                                                     │
│          nonceCaches: [                                      │
│            { nonceAccount: "...", used: true },  ← USED!   │
│            { nonceAccount: "...", used: false },           │
│            { nonceAccount: "...", used: false }            │
│          ]                                                   │
│        }                                                     │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│  3. CREATE ANOTHER TRANSACTION                               │
├─────────────────────────────────────────────────────────────┤
│  User: Taps "Create Transaction" again                      │
│  Rust: Load bundle                                           │
│  Rust: Pick next unused nonce (nonce #2)                    │
│  Rust: Mark nonce #2 as used                                │
│  Rust: Save bundle                                           │
│  Rust: Return transaction                                    │
│                                                               │
│  Storage now:                                                │
│        {                                                     │
│          nonceCaches: [                                      │
│            { used: true },  ← nonce #1 used                │
│            { used: true },  ← nonce #2 used                │
│            { used: false }  ← nonce #3 available           │
│          ]                                                   │
│        }                                                     │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│  4. PREPARE BUNDLE (After Submitting Transactions)          │
├─────────────────────────────────────────────────────────────┤
│  User: Submits transactions, nonces get advanced on-chain  │
│  User: Taps "Prepare Bundle (3 nonces)" again               │
│  Rust: Load existing bundle from storage                    │
│  Rust: See 2 nonces marked as used                          │
│  Rust: Refresh those 2 nonces (fetch new blockhash)        │
│        → Cost: $0.00 FREE! Just an RPC call                │
│  Rust: Mark refreshed nonces as used = false               │
│  Rust: Save updated bundle                                   │
│                                                               │
│  Result: All 3 nonces available again (2 refreshed, 1 never used)
│  Cost: $0.00 vs $0.40 to create 2 new nonces!              │
└─────────────────────────────────────────────────────────────┘
```

## 📝 Key Changes

### 1. Rust FFI (`src/ffi/android.rs`)

#### `prepare_offline_bundle`:
```rust
// OLD (WRONG): Always created new nonces
let bundle = prepare_offline_bundle(count, keypair, None).await?;

// NEW (CORRECT): Loads existing, passes to service for refresh
let existing = storage.load_bundle()?;
// Save to temp file so service can load it
let bundle = prepare_offline_bundle(count, keypair, Some(temp_path)).await?;
// Service sees existing bundle, refreshes used nonces (FREE!)
storage.save_bundle(&bundle)?;
```

#### `create_offline_transaction`:
```rust
// OLD (WRONG): Kotlin sent nonce data
let nonce = request.cached_nonce.to_transaction_type();

// NEW (CORRECT): Load from storage, pick nonce, mark as used
let mut bundle = storage.load_bundle()?.ok_or("No bundle found")?;
let nonce_to_use = bundle.nonce_caches.iter_mut()
    .find(|n| !n.used)
    .ok_or("No available nonces")?;

let cached_nonce = nonce_to_use.clone();
nonce_to_use.used = true;  // ← MARK AS USED!

storage.save_bundle(&bundle)?;  // ← SAVE IMMEDIATELY!

// Now create transaction with the selected nonce
create_offline_transaction(..., &cached_nonce)?;
```

### 2. Kotlin SDK (`PolliNetSDK.kt`)

```kotlin
// OLD (WRONG): Required nonce parameter
suspend fun createOfflineTransaction(
    ...
    cachedNonce: CachedNonceData  // ← Had to pass nonce
)

// NEW (CORRECT): No nonce parameter
suspend fun createOfflineTransaction(
    ...
    // Nonce automatically picked from storage!
)
```

### 3. UI (`DiagnosticsScreen.kt`)

```kotlin
// OLD (WRONG): Had to pick nonce from bundle
bundle?.nonceCaches?.firstOrNull()?.let { nonce ->
    sdk.createOfflineTransaction(..., cachedNonce = nonce)
}

// NEW (CORRECT): No nonce needed
sdk.createOfflineTransaction(
    senderKeypair = keypairBytes,
    nonceAuthorityKeypair = keypairBytes,
    recipient = "...",
    amount = 1_000_000
    // Rust picks nonce automatically from storage!
)
```

## 🎯 Benefits of Fixed Workflow

### Cost Optimization
```
WITHOUT FIX (Old Way):
- Prepare bundle (3 nonces): $0.60
- Use all 3 nonces
- Prepare bundle again: $0.60  ← Creates NEW nonces!
- Total for 6 transactions: $1.20

WITH FIX (Correct Way):
- Prepare bundle (3 nonces): $0.60
- Use all 3 nonces
- Prepare bundle again: $0.00  ← Refreshes used nonces!
- Total for 6 transactions: $0.60
- SAVINGS: 50%! 💰
```

### Correct Nonce Lifecycle
```
1. CREATE → used = false
2. USE IN TX → used = true (marked immediately)
3. SUBMIT TX → nonce advances on-chain
4. REFRESH → fetch new blockhash (FREE), used = false
5. REPEAT → Can use same nonce account again!
```

### Automatic Storage Management
- ✅ Kotlin doesn't manage nonces at all
- ✅ Rust handles everything in storage
- ✅ No nonce data sent over FFI boundary
- ✅ Immediate persistence after marking as used
- ✅ Correct refresh behavior (not creating new)

## 📋 Testing the Fix

### Test 1: First Time Bundle Creation
```bash
# Logcat output to verify:
📦 Preparing offline bundle for 3 transactions
📂 No existing bundle found - will create new one
✅ Bundle prepared with 3 total nonces (3 available)
💾 Bundle saved to secure storage
   Total nonces: 3, Available: 3, Used: 0
```

### Test 2: Create Transaction (Marks as Used)
```bash
# Logcat output to verify:
📴 Creating OFFLINE transaction
📂 Loaded bundle: 3 total nonces, 3 available
📌 Using nonce account: <account>
✅ Marked nonce as used
💾 Bundle saved with updated nonce status
   Available nonces remaining: 2
```

### Test 3: Create Another Transaction
```bash
# Logcat output to verify:
📴 Creating OFFLINE transaction
📂 Loaded bundle: 3 total nonces, 2 available
📌 Using nonce account: <different account>
✅ Marked nonce as used
💾 Bundle saved
   Available nonces remaining: 1
```

### Test 4: Refresh After Submission
```bash
# Logcat output to verify:
📦 Preparing offline bundle for 3 transactions
📂 Found existing bundle with 3 nonces (available: 1, used: 2)
💾 Saved existing bundle to temp file
♻️  Refreshing 2 used nonce accounts (advanced)...
✅ Refreshed 2 nonce accounts (FREE!)
💾 Bundle saved to secure storage
   Total nonces: 3, Available: 3, Used: 0  ← All available again!
```

## 🔍 What to Watch in Logs

### Correct Behavior:
- ✅ "Found existing bundle" on second prepare call
- ✅ "Refreshing X used nonce accounts" (not creating new)
- ✅ "Marked nonce as used" immediately after picking
- ✅ "Bundle saved" after every transaction creation
- ✅ "Available nonces remaining: X" decreasing correctly

### Incorrect Behavior (OLD):
- ❌ Always "Creating new bundle"
- ❌ "Creating X NEW nonce accounts" on every prepare
- ❌ No "marked as used" messages
- ❌ Bundle not saving after transaction creation

## 📊 Summary

| Action | Old Behavior | New Behavior |
|--------|-------------|--------------|
| **Prepare (first)** | Create 3 nonces ($0.60) | Create 3 nonces ($0.60) |
| **Create tx** | Use nonce from Kotlin | Load from storage, mark used, save |
| **Create tx again** | Use nonce from Kotlin | Pick next unused, mark used, save |
| **Prepare (second)** | Create 3 MORE nonces ($0.60) | Refresh 2 used nonces ($0.00) |
| **Total cost** | $1.20 for 6 tx | $0.60 for infinite tx |
| **Storage updates** | None | After every tx creation |
| **Nonce tracking** | Kotlin tracks | Rust tracks in storage |

## ✅ Verification Commands

```bash
# Watch logs during testing:
adb logcat -s "PolliNet-Rust:D" | grep -E "(📦|📴|📂|✅|💾|♻️|📌)"

# Look for these patterns:
# 1. First prepare: "No existing bundle found"
# 2. Create tx: "Marked nonce as used"
# 3. Second prepare: "Refreshing X used nonce accounts"
```

The workflow is now **correct and cost-optimized**! 🎉

