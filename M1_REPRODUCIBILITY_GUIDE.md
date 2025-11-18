# M1 Reproducibility Guide

This guide enables independent external developers to reproduce the M1 requirement: **50+ successful offline-to-online transactions on Solana Devnet**.

## Prerequisites

1. **Rust Toolchain** (1.70+)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Solana CLI** (for devnet access)
   ```bash
   sh -c "$(curl -sSfL https://release.solana.com/stable/install)"
   ```

3. **Environment Setup**
   ```bash
   # Create .env file in project root
   cat > .env << EOF
   SOLANA_URL=https://api.devnet.solana.com
   EOF
   ```

## Quick Reproduction

### Option 1: Automated Test Suite (Recommended)

```bash
# Run M1 demo only (50+ transactions)
./scripts/test_pollinet.sh --m1-only
```

This will:
- ✅ Check prerequisites
- ✅ Verify build
- ✅ Create 50 nonce accounts
- ✅ Generate 50 offline transactions
- ✅ Submit all 50 transactions to devnet
- ✅ Verify 50+ successful transactions
- ✅ Generate summary report

**Duration:** ~5-10 minutes  
**Output:** `test_results/TIMESTAMP/m1_demo.log` and `test_results/TIMESTAMP/summary.md`

### Option 2: Manual Execution

```bash
# Step 1: Prepare 50 nonce accounts
cargo run --example nonce_refresh_utility

# Step 2: Run M1 demo
cargo run --example m1_demo_50_transactions
```

## Expected Results

### Success Criteria

✅ **50+ nonce accounts created**  
✅ **50 offline transactions generated**  
✅ **50+ transactions successfully submitted to devnet**  
✅ **All transaction signatures saved to `.offline_submission.json`**

### Verification

1. **Check transaction count:**
   ```bash
   grep -c "Successfully submitted" test_results/*/m1_demo.log
   # Should show 50+
   ```

2. **Check signatures file:**
   ```bash
   cat .offline_submission.json | jq '.signatures | length'
   # Should be 50+
   ```

3. **Verify on Solana Explorer:**
   - Open `.offline_submission.json`
   - Copy any signature
   - Visit: `https://explorer.solana.com/tx/<signature>?cluster=devnet`
   - Verify transaction status: "Success"

## Troubleshooting

### Issue: Insufficient Funds

**Solution:**
- The demo automatically requests airdrops
- If airdrop fails, manually fund wallet:
  ```bash
  solana airdrop 10 <YOUR_WALLET_ADDRESS> --url devnet
  ```

### Issue: RPC Rate Limiting

**Solution:**
- Use local validator instead:
  ```bash
  solana-test-validator
  # Then set SOLANA_URL=http://127.0.0.1:8899 in .env
  ```

### Issue: Build Errors

**Solution:**
```bash
cargo clean
cargo build --all-targets --examples
```

## Independent Verification

For external developers to independently verify:

1. **Clone the repository:**
   ```bash
   git clone <repository-url>
   cd pollinet
   ```

2. **Follow Quick Reproduction steps above**

3. **Verify results:**
   - Check `test_results/` directory for logs
   - Verify 50+ transaction signatures
   - Confirm transactions on Solana Explorer

## Documentation

- **[Offline Transactions Guide](./OFFLINE_TRANSACTIONS_GUIDE.md)** - Complete guide to offline transactions
- **[Testing Guide](./TESTING.md)** - Comprehensive testing documentation
- **[README](./README.md)** - Project overview

## Support

If you encounter issues reproducing the M1 demo:

1. Check logs in `test_results/TIMESTAMP/`
2. Review [TESTING.md](./TESTING.md) troubleshooting section
3. Verify environment setup matches prerequisites
4. Check Solana devnet status

---

**M1 Requirement:** Demonstrate at least fifty (50+) successful offline to online transactions on Solana Devnet, reproducible by three (3) or more independent external developers.

