# 📖 PolliNet Offline Transactions Guide

## 🌐 PolliNet: Enabling Offline Solana Transactions Through Mesh Networking

### Vision: Universal Wallet Integration

PolliNet is designed to be integrated into major Solana wallet applications, transforming every device into a potential relay node for offline transactions. When integrated into wallets such as:

- **Solana Mobile** (Saga phones and future devices)
- **Phantom Wallet** (Mobile and desktop)
- **Jupiter Wallet** (Mobile)
- **Solflare** (Mobile and web)
- **And other Solana wallet providers**

Every device running these wallets becomes an automatic participant in the PolliNet mesh network. This creates a **decentralized, opportunistic relay infrastructure** where:

1. **Users create transactions offline** using nonce accounts
2. **Transactions propagate automatically** via Bluetooth Low Energy (BLE) mesh
3. **Any connected device** can relay transactions to the blockchain
4. **No central coordination** is required—the network is truly decentralized

### The Network Effect

As more wallets integrate PolliNet, the mesh network becomes exponentially more powerful:

```
10 wallets → 10 potential relay nodes
100 wallets → 100 potential relay nodes
1,000 wallets → 1,000 potential relay nodes
```

Each additional device increases network coverage, redundancy, and reliability. Transactions can hop between devices until they reach one with internet connectivity.

### Platform Implementation

**Production BLE Implementation:**
- **Android SDK** – The primary, production-ready implementation
  - Full BLE GATT support
  - Foreground service for reliable operation
  - Optimized for battery efficiency
  - Production-hardened for real-world deployment

**Development & Testing:**
- **Linux/macOS Simulation** – For development and testing only
  - Simulates BLE mesh behavior
  - Useful for local debugging and CI/CD testing
  - **Not intended for production use**
  - Examples demonstrate concepts but use simulated BLE

**Important:** The examples in this guide run on Linux/macOS using simulation mode. For production deployments, the Android SDK provides the full BLE mesh networking capabilities.

---

## 🎯 Core Concept: Nonce Accounts = Offline Transaction Capacity

### Understanding the Relationship

**The number of nonce accounts you create is exactly equal to the number of offline transactions you can perform before you need internet access again.**

#### Why?

Each Solana transaction requires a **unique, unadvanced nonce** to be valid. When you create a transaction offline:

1. You use a nonce account with a cached blockhash
2. That specific nonce can only be used **once**
3. After submission, the nonce is "advanced" and cannot be reused
4. **You need a new nonce account for the next transaction**

#### Example:

```
If you create 5 nonce accounts:
├── Nonce Account 1 → Used for Transaction 1
├── Nonce Account 2 → Used for Transaction 2
├── Nonce Account 3 → Used for Transaction 3
├── Nonce Account 4 → Used for Transaction 4
└── Nonce Account 5 → Used for Transaction 5

After Transaction 5 → All nonces used → Need internet to refresh
```

**Important:** Once you've used all nonce accounts, you **must** reconnect to the internet to refresh them before creating more offline transactions.

---

## 💰 Cost Optimization: Nonce Caching and Reuse

### Why We Cache and Reuse Nonces

Creating a new nonce account costs approximately **~0.0015 SOL** (~$0.25 at current prices). This adds up quickly if you create new accounts for every transaction.

#### Cost Breakdown:

| Approach | First 10 Transactions | Next 10 Transactions | Total (20 tx) |
|----------|----------------------|---------------------|---------------|
| **New nonce each time** | 0.015 SOL (~$2.50) | 0.015 SOL (~$2.50) | 0.030 SOL (~$5.00) |
| **Reuse & refresh** | 0.0075 SOL (~$1.25) | **$0.00** (FREE!) | **0.0075 SOL (~$1.25)** |

### How Nonce Refresh Works

1. **Initial Creation:** Create N nonce accounts once (one-time cost: ~0.0015 SOL each)
2. **Use Offline:** Each transaction uses one nonce account
3. **Refresh Online:** When you reconnect, refresh used nonces by fetching their **advanced blockhash** (FREE - no transaction needed!)
4. **Reuse Forever:** Same nonce accounts can be refreshed indefinitely

#### Example Workflow:

```
Session 1 (Online):
  → Create 5 nonce accounts (cost: ~0.0075 SOL)
  → Save to .offline_bundle.json

Session 2 (Offline):
  → Use all 5 nonces for transactions
  → Mark as "used" in bundle

Session 3 (Online):
  → Refresh used nonces (FREE - just fetch new blockhash!)
  → Reuse same 5 accounts forever
```

**Result:** 99% cost reduction for ongoing use!

---

## 🚀 Running Examples: Step-by-Step Guide

### Prerequisites

**⚠️ Important Note:** The examples in this guide run in **simulation mode** on Linux/macOS. They demonstrate PolliNet concepts but use simulated BLE mesh networking. For production deployments with real BLE mesh networking, use the **Android SDK**.

1. **Environment Setup:**
   ```bash
   # Create .env file in project root
   cat > .env << EOF
   SOLANA_URL=https://api.devnet.solana.com
   # Optional: Use your wallet private key (base58 encoded)
   # WALLET_PRIVATE_KEY=your_base58_private_key_here
   EOF
   ```

2. **Rust Toolchain:**
   ```bash
   # Verify Rust is installed
   rustc --version
   cargo --version
   ```

3. **Solana CLI (Optional, for local validator):**
   ```bash
   # If using local validator instead of devnet
   solana-test-validator
   # Then set SOLANA_URL=http://127.0.0.1:8899 in .env
   ```

---

### Recommended Example Order

#### 🎬 Quick Start (Automated)

Use the CLI wrapper script (`scripts/pollinet_cli.sh`) for streamlined workflows. All commands capture logs under `cli_logs/` for review.

**CLI Commands:**

```bash
# 1. Prepare nonce bundle (creates/refreshes nonce accounts)
./scripts/pollinet_cli.sh prepare
# Outputs: .offline_bundle.json, logs in cli_logs/

# 2. Relay transaction via BLE (simulation on desktop)
./scripts/pollinet_cli.sh relay
# Compresses, fragments, and broadcasts over simulated BLE adapter

# 3. Submit offline transactions to Solana
./scripts/pollinet_cli.sh submit
# Decompresses payloads, verifies nonces, submits to devnet

# 4. Refresh nonces only (without creating transactions)
./scripts/pollinet_cli.sh refresh-nonces
```

**For comprehensive testing including M1 demo:**
```bash
./scripts/test_pollinet.sh --m1-only
```

---

#### 📝 Manual Step-by-Step

##### **Step 1: Prepare Nonce Bundle (REQUIRED FIRST)**

Before any offline transactions, you must create a nonce bundle:

```bash
cargo run --example nonce_refresh_utility
```

**What it does:**
- Creates `.offline_bundle.json` if it doesn't exist
- Creates N nonce accounts (default: 5)
- Caches blockhash and account data for offline use
- Refreshes used nonces if bundle already exists

**Output:**
```
✅ Bundle created successfully!
   Total nonces: 5
   Available nonces: 5
```

**Important:** This is the **ONLY** example you must run before others!

---

##### **Step 2: Choose Your Example**

Based on what you want to test:

###### **Option A: Simple Offline Transaction Flow**
```bash
cargo run --example offline_transaction_flow
```
**Demonstrates:**
- Loading nonce bundle offline
- Creating transactions without internet
- Saving transactions for later submission
- Submitting when back online

###### **Option B: Complete Bundle Management**
```bash
cargo run --example offline_bundle_management
```
**Demonstrates:**
- Full 3-phase workflow (online → offline → online)
- Automatic nonce tracking and reuse
- BLE fragmentation and reassembly
- Cost optimization through refresh

###### **Option C: BLE Mesh Sender/Receiver (Simulation)**
```bash
# Terminal 1: Receiver (waiting for connections)
cargo run --example offline_transaction_receiver

# Terminal 2: Sender (connects and sends)
cargo run --example offline_transaction_sender
```
**Demonstrates:**
- BLE mesh network communication (simulated on Linux/macOS)
- Real-time transaction relay
- Multi-device coordination

**Note:** These examples use simulated BLE mesh networking. For production BLE mesh networking, use the Android SDK which provides full GATT-based BLE mesh capabilities.

###### **Option D: Specific Transaction Types**

```bash
# SOL transfer with nonce
cargo run --example create_nonce_transaction

# SPL token transfer
cargo run --example create_spl_nonce_transaction

# Governance voting
cargo run --example cast_governance_vote

# Unsigned transactions (multi-party signing)
cargo run --example create_unsigned_transaction
```

###### **Option E: M1 Demo (50+ Transactions)**
```bash
cargo run --example m1_demo_50_transactions
```
**Demonstrates:**
- Creating 50 nonce accounts
- Performing 50+ offline transactions
- Successful batch submission

---

##### **Step 3: Refresh Nonces (When Needed)**

After using nonces, refresh them for reuse:

```bash
cargo run --example nonce_refresh_utility
```

**What it does:**
- Detects used nonces in `.offline_bundle.json`
- Refreshes them by fetching new blockhashes (FREE!)
- Makes them available again for offline use

**When to run:**
- After creating transactions offline
- Before starting a new offline session
- When you see "No available nonces" errors

---

## 🔄 Complete Workflow Example

Here's a realistic workflow:

```bash
# 1. Initial Setup (Online - One Time)
cargo run --example nonce_refresh_utility
# Creates 5 nonce accounts → Can do 5 offline transactions

# 2. Go Offline and Create Transactions
cargo run --example offline_transaction_flow
# Uses 3 nonces → 3 transactions created offline
# Remaining: 2 nonces available

# 3. Create More Transactions (Still Offline)
# (You could create 2 more transactions before needing refresh)

# 4. Come Back Online and Refresh
cargo run --example nonce_refresh_utility
# Refreshes 3 used nonces (FREE!)
# Now have 5 available nonces again

# 5. Repeat Steps 2-4 Forever
# Same 5 nonce accounts, refreshed for free each time!
```

---

## 📊 Understanding Bundle File

The `.offline_bundle.json` file contains:

```json
{
  "created_at": "2024-01-15T10:30:00Z",
  "nonces": [
    {
      "nonce_account": "Account1...",
      "authority": "Authority1...",
      "blockhash": "Blockhash1...",
      "used": false  // ← Available for offline transaction
    },
    {
      "nonce_account": "Account2...",
      "authority": "Authority2...",
      "blockhash": "Blockhash2...",
      "used": true   // ← Already used, needs refresh
    }
  ]
}
```

**Key Points:**
- `used: false` → Available for offline transactions
- `used: true` → Needs refresh (free) before reuse
- File persists between sessions
- Same accounts are reused forever (cost optimization)

---

## ⚠️ Common Issues and Solutions

### Issue: "No available nonces in bundle"

**Solution:**
```bash
# Refresh used nonces
cargo run --example nonce_refresh_utility
```

### Issue: "Bundle file not found"

**Solution:**
```bash
# Create bundle first
cargo run --example nonce_refresh_utility
```

### Issue: "Insufficient funds for airdrop"

**Solution:**
- Use `SOLANA_URL=http://127.0.0.1:8899` for local validator
- Local validators provide free airdrops
- Or use your funded devnet wallet via `WALLET_PRIVATE_KEY` in `.env`

### Issue: "Nonce has been advanced"

**Explanation:**
- This means the nonce was already used
- This is expected if you submit the same transaction twice
- Each nonce can only be used once per transaction

---

## 💡 Best Practices

1. **Always create bundle first:** Run `nonce_refresh_utility` before offline work
2. **Check available nonces:** Examples show bundle stats before starting
3. **Refresh proactively:** Run refresh before starting new offline session
4. **Monitor balance:** Ensure wallet has enough SOL for fees (~0.0015 SOL per nonce)
5. **Use same wallet:** Keep `WALLET_PRIVATE_KEY` in `.env` for consistency
6. **Start small:** Test with 5 nonces before creating 50+

---

## 🎓 Summary

| Concept | Explanation |
|---------|-------------|
| **Nonce Account** | One account = one offline transaction capacity |
| **Bundle File** | `.offline_bundle.json` stores all nonce data |
| **Used Nonces** | Cannot be reused until refreshed |
| **Refresh** | Free operation to make used nonces available again |
| **Reuse** | Same accounts can be refreshed forever (cost optimization) |

**Key Formula:**
```
Number of Nonce Accounts = Number of Offline Transactions Before Internet Needed
```

**Remember:**
- Create once (costs ~0.0015 SOL each)
- Use offline (one per transaction)
- Refresh online (FREE!)
- Reuse forever (99% cost savings!)

---

## 📚 Additional Resources

### Official Documentation

- **[GitHub Repository](https://github.com/pollinet/pollinet)** – Source code, issues, and contributions
- **[Whitepaper](https://pollinet.github.io/pollinet/)** – Detailed technical architecture and design rationale
- **[M1 Reproducibility Guide](./M1_REPRODUCIBILITY_GUIDE.md)** – M1 demo instructions
- **[Main README](./README.md)** – Project overview and getting started

### Integration Resources

For wallet developers interested in integrating PolliNet:

- **Android SDK** – Production-ready BLE implementation
- **Rust Core Library** – Cross-platform transaction handling
- **Example Implementations** – Reference implementations for common use cases
- **API Documentation** – Comprehensive SDK documentation

### Community & Support

- **Issues & Bug Reports** – [GitHub Issues](https://github.com/pollinet/pollinet/issues)
- **Contributions** – [Contributing Guidelines](https://github.com/pollinet/pollinet/blob/main/CONTRIBUTING.md)
- **Discussions** – [GitHub Discussions](https://github.com/pollinet/pollinet/discussions)

---

## 🔗 Quick Links

- **Repository:** https://github.com/pollinet/pollinet
- **Whitepaper:** https://pollinet.github.io/pollinet/
- **Documentation:** See Additional Resources above

---

**Happy Offline Transacting! 🚀**

*PolliNet: Decentralized Solana transaction propagation over Bluetooth Low Energy mesh networks.*

