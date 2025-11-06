# MWA Nonce Account Creation - Complete Implementation Guide

## Overview

This document describes the complete implementation of creating durable nonce accounts using Mobile Wallet Adapter (MWA) for PolliNet's offline transaction bundles.

## Problem Statement

PolliNet requires durable nonce accounts for offline/mesh transaction creation. However:
- Creating nonce accounts requires on-chain transactions
- These transactions need to be signed by the wallet holder
- The transactions must be co-signed by an ephemeral nonce keypair
- Traditional approaches required storing private keys, which violates Solana Mobile's security model

## Solution Architecture

### Multi-Layer Implementation

**Rust Core (`src/transaction/mod.rs`)**
```rust
pub async fn create_unsigned_nonce_transactions(
    &self,
    count: usize,
    payer_pubkey_str: &str,
) -> Result<Vec<UnsignedNonceTransaction>, TransactionError>
```
- Generates N ephemeral nonce account keypairs
- Creates unsigned nonce account creation transactions
- Returns transactions + nonce keypairs for local signing

**Rust FFI (`src/ffi/android.rs`)**
```rust
Java_xyz_pollinet_sdk_PolliNetFFI_createUnsignedNonceTransactions
Java_xyz_pollinet_sdk_PolliNetFFI_cacheNonceAccounts
```
- Bridges Rust core to Kotlin/Android
- Handles JSON serialization/deserialization
- Manages secure storage integration

**Kotlin SDK (`PolliNetSDK.kt`)**
```kotlin
suspend fun createUnsignedNonceTransactions(count: Int, payerPubkey: String)
suspend fun cacheNonceAccounts(nonceAccounts: List<String>)
```
- High-level async API for Android apps
- Type-safe data classes
- Automatic error handling

**Android UI (`MwaTransactionDemo.kt`)**
- Complete user workflow implementation
- MWA integration with proper error handling
- Progress tracking and user feedback

## Complete Workflow

### Step 1: Create Unsigned Transactions (Rust)

```rust
// In TransactionService
for i in 0..count {
    // Generate ephemeral nonce keypair
    let nonce_keypair = Keypair::new();
    let nonce_pubkey = nonce_keypair.pubkey();
    
    // Get rent exemption and blockhash from RPC
    let rent_exemption = client.get_minimum_balance_for_rent_exemption(
        solana_sdk::nonce::State::size()
    )?;
    let recent_blockhash = client.get_latest_blockhash()?;
    
    // Create nonce account instructions
    let create_nonce_instructions = system_instruction::create_nonce_account(
        &payer_pubkey,         // funding account (payer)
        &nonce_pubkey,         // nonce account
        &payer_pubkey,         // authority (set to payer)
        rent_exemption,        // lamports
    );
    
    // Create unsigned transaction
    let mut tx = Transaction::new_with_payer(
        &create_nonce_instructions,
        Some(&payer_pubkey),
    );
    tx.message.recent_blockhash = recent_blockhash;
    
    // Serialize and return
    result.push(UnsignedNonceTransaction {
        unsigned_transaction_base64: base64::encode(&tx_bytes),
        nonce_keypair_base64: base64::encode(&nonce_keypair.to_bytes()),
        nonce_pubkey: nonce_pubkey.to_string(),
    });
}
```

### Step 2: Sign with MWA (Kotlin)

```kotlin
// In MwaTransactionDemo.kt
val unsignedNonceTxs = sdk.createUnsignedNonceTransactions(
    count = 5,
    payerPubkey = authorizedPubkey
).getOrThrow()

val noncePublicKeys = mutableListOf<String>()

for ((index, nonceTx) in unsignedNonceTxs.withIndex()) {
    // Send to MWA for signing
    val signedBytes = mwaClient.signAndSendTransaction(
        sender = activityResultSender,
        unsignedTransactionBase64 = nonceTx.unsignedTransactionBase64
    )
    
    // Transaction submitted successfully
    noncePublicKeys.add(nonceTx.noncePubkey)
}
```

### Step 3: Cache Nonce Data (Rust + Storage)

```kotlin
// In MwaTransactionDemo.kt
val cachedCount = sdk.cacheNonceAccounts(noncePublicKeys).getOrThrow()
```

```rust
// In FFI android.rs
let mut bundle = secure_storage.load_bundle()
    .unwrap_or_else(|_| OfflineTransactionBundle::new());

for nonce_account in &request.nonce_accounts {
    let cached_nonce = transport
        .transaction_service()
        .prepare_offline_nonce_data(nonce_account)
        .await?;
    
    bundle.add_nonce(cached_nonce);
}

secure_storage.save_bundle(&bundle)?;
```

## Data Flow

```
┌─────────────────────────────────────────────────────────────────┐
│ User clicks "Prepare Bundle" in MwaTransactionDemo.kt          │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ Kotlin: sdk.createUnsignedNonceTransactions(5, payerPubkey)    │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ FFI: Java_xyz_pollinet_sdk_PolliNetFFI_createUnsignedNonce...  │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ Rust: TransactionService::create_unsigned_nonce_transactions   │
│ - Generate 5 nonce keypairs                                    │
│ - Create 5 unsigned transactions                               │
│ - Return [UnsignedNonceTransaction]                            │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ Kotlin: Loop through transactions                              │
│   - mwaClient.signAndSendTransaction(unsignedTx)               │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ MWA: User approves in wallet app (Solflare, Phantom, etc.)    │
│ - Transaction gets co-signed with payer + nonce keypairs       │
│ - Transaction submitted to Solana network                      │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ Kotlin: sdk.cacheNonceAccounts([noncePubkey1, ...])           │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ Rust: Fetch nonce data from blockchain                        │
│ - Load existing bundle from secure storage                     │
│ - Add new nonces to bundle                                     │
│ - Save bundle: /data/user/0/xyz.pollinet.android/files/...    │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ ✅ Success! 5 nonce accounts ready for offline transactions    │
└─────────────────────────────────────────────────────────────────┘
```

## Key Security Features

### 1. No Private Key Exposure
- Ephemeral nonce keypairs are generated in Rust
- Nonce keypair bytes are returned to Kotlin but NOT stored permanently
- Payer's private key NEVER leaves the wallet's secure hardware (Seed Vault)

### 2. Co-Signing Model
Each nonce creation transaction requires TWO signatures:
1. **Nonce keypair signature**: Ephemeral, generated by PolliNet
2. **Payer signature**: Secured by MWA/Seed Vault

### 3. Secure Storage
- Nonce account DATA (not keys) is cached after creation
- Stored in Android's private app storage
- Rust `SecureStorage` abstraction ensures proper isolation

## Transaction Structure

```rust
// Nonce account creation transaction
Transaction {
    message: {
        instructions: [
            // 1. Create account (system program)
            CreateAccount {
                from: payer_pubkey,
                to: nonce_pubkey,
                lamports: rent_exemption,
                space: State::size(),
                owner: system_program,
            },
            // 2. Initialize nonce account
            InitializeNonceAccount {
                nonce_account: nonce_pubkey,
                authority: payer_pubkey,
            }
        ],
        recent_blockhash: recent_blockhash,
        signers: [nonce_pubkey, payer_pubkey],  // Both must sign!
    }
}
```

## Error Handling

### Rust Errors
```rust
pub enum TransactionError {
    RpcClient(String),          // RPC connection issues
    InvalidPublicKey(String),   // Invalid base58 pubkey
    Serialization(String),      // Bincode errors
    // ...
}
```

### Kotlin Exceptions
```kotlin
try {
    val transactions = sdk.createUnsignedNonceTransactions(5, pubkey).getOrThrow()
} catch (e: Exception) {
    // Handle: network errors, RPC errors, invalid parameters
}
```

### MWA Exceptions
```kotlin
try {
    mwaClient.signAndSendTransaction(sender, unsignedTx)
} catch (e: MwaException) {
    // User rejected, no wallet found, signing failed
}
```

## Cost Analysis

Creating 5 nonce accounts on Solana devnet:
- **Rent-exempt minimum per account**: ~0.00144768 SOL (1,447,680 lamports)
- **Total for 5 accounts**: ~0.00723840 SOL
- **Transaction fees**: ~0.000005 SOL per transaction × 5 = ~0.000025 SOL
- **Grand total**: ~0.00726340 SOL (~$0.0007 USD at $0.10/SOL)

## Testing Workflow

### Prerequisites
1. Solana Mobile-compatible wallet installed (Solflare, Phantom, or fakewallet)
2. Test account funded with devnet SOL (use faucet: https://faucet.solana.com)
3. Android device with BLE capabilities

### Test Steps

1. **Launch PolliNet Android app**
   ```bash
   adb install -r app-debug.apk
   adb shell am start -n xyz.pollinet.android/.MainActivity
   ```

2. **Navigate to "Diagnostics" tab → "MWA Transaction Demo"**

3. **Click "Connect Wallet"**
   - Wallet app launches
   - Approve authorization
   - Your public key appears in the UI

4. **Click "Prepare Bundle"**
   - Status: "Creating nonce account transactions..."
   - Status: "Sending 5 transactions to wallet for signing..."
   - Wallet prompts for each transaction (or batch)
   - Approve all 5 transactions
   - Status: "Caching 5 nonce accounts..."
   - Status: "✅ Successfully prepared offline bundle with 5 nonce accounts!"

5. **Verify Storage**
   ```bash
   adb shell run-as xyz.pollinet.android ls files/
   # Should see: nonce_bundle.json
   
   adb shell run-as xyz.pollinet.android cat files/nonce_bundle.json
   # Should see JSON with 5 nonce entries
   ```

6. **Create Offline Transaction**
   - Now you can create transactions without internet!
   - Nonces are automatically consumed and refreshed

## Monitoring & Logs

### Rust Logs (via logcat)
```bash
adb logcat -s "PolliNet-Rust"
```

Example output:
```
I PolliNet-Rust: 🎯 FFI createUnsignedNonceTransactions called
I PolliNet-Rust: Creating 5 unsigned nonce account transactions
I PolliNet-Rust: Rent exemption for nonce account: 1447680 lamports
I PolliNet-Rust: Transaction 1/5: Nonce account FxY8...
I PolliNet-Rust: ✅ Created 5 unsigned nonce transactions
```

### Android Logs
```bash
adb logcat -s "MwaTransactionDemo"
```

Example output:
```
D MwaTransactionDemo: Creating nonce account transactions...
D MwaTransactionDemo: Signing transaction 1/5...
D MwaTransactionDemo: Transaction 1 submitted. Waiting for confirmation...
D MwaTransactionDemo: ✅ Successfully prepared offline bundle with 5 nonce accounts!
```

## Future Enhancements

1. **Batch Signing**: If wallet supports batch operations, sign all 5 at once
2. **Progress Persistence**: Save progress if user cancels mid-way
3. **Automatic Refresh**: Refresh used nonces in background
4. **Cost Estimation**: Show user estimated cost before signing
5. **Nonce Verification**: Verify nonce validity before caching

## Troubleshooting

### "No MWA-compatible wallet found"
- Install a Solana Mobile wallet (Solflare, Phantom, or fakewallet for testing)

### "Failed to create any nonce accounts. User may have rejected."
- User must approve ALL transactions in the wallet
- Check if wallet has sufficient SOL balance

### "Failed to cache nonce accounts"
- Ensure storage directory is initialized in SDK config
- Check logcat for Rust storage errors

### Transactions stuck pending
- Solana devnet can be slow or congested
- Wait a few seconds between transactions
- Check transaction status on Solana Explorer

## References

- [Solana Durable Nonces](https://docs.solana.com/offline-signing/durable-nonce)
- [Solana Mobile MWA Docs](https://docs.solanamobile.com/android-native/using_mobile_wallet_adapter)
- [PolliNet Architecture](./ARCHITECTURE.md)
- [MWA Integration Progress](./MWA_INTEGRATION_PROGRESS.md)

## Summary

This implementation successfully enables:
✅ Secure nonce account creation via MWA
✅ No private key exposure
✅ Persistent storage for offline use
✅ Complete end-to-end workflow
✅ Proper error handling at all layers
✅ User-friendly progress feedback

The nonce accounts can now be used for creating offline transactions in mesh networks, fulfilling PolliNet's core mission of enabling Solana transactions in disconnected environments.

