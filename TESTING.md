# PolliNet Testing Guide

This guide explains how to test PolliNet using the comprehensive test script.

## Quick Start

### Run All Tests

```bash
./scripts/test_pollinet.sh
```

This will:
1. Check prerequisites (Rust, Solana CLI, .env file)
2. Verify the build
3. Run basic functionality tests
4. Run advanced functionality tests
5. Run M1 demo (50+ transactions)
6. Generate a summary report

### Quick Test (Skip M1 Demo)

```bash
./scripts/test_pollinet.sh --quick
```

Useful for faster iteration when you don't need to test the full M1 requirement.

### M1 Demo Only

```bash
./scripts/test_pollinet.sh --m1-only
```

Runs only the M1 demo (50+ transactions). This takes approximately 5-10 minutes.

## Test Modes

### Full Mode (Default)
- ✅ Prerequisites check
- ✅ Build verification
- ✅ Basic functionality tests
- ✅ Advanced functionality tests
- ✅ M1 demo (50+ transactions)

**Duration:** ~10-15 minutes

### Quick Mode (`--quick`)
- ✅ Prerequisites check
- ✅ Build verification
- ✅ Basic functionality tests
- ❌ Advanced tests (skipped)
- ❌ M1 demo (skipped)

**Duration:** ~2-3 minutes

### M1 Only Mode (`--m1-only`)
- ✅ Prerequisites check
- ✅ Build verification
- ❌ Basic tests (skipped)
- ❌ Advanced tests (skipped)
- ✅ M1 demo (50+ transactions)

**Duration:** ~5-10 minutes

## Test Results

All test results are saved to `test_results/TIMESTAMP/`:

```
test_results/
└── 20241118_120000/
    ├── summary.md          # Test summary report
    ├── build.log           # Build verification
    ├── nonce_refresh_utility.log
    ├── create_nonce_transaction.log
    ├── offline_transaction_flow.log
    ├── create_spl_nonce_transaction.log
    ├── cast_governance_vote.log
    ├── create_unsigned_transaction.log
    ├── relay_presigned_transaction.log
    ├── offline_bundle_management.log
    └── m1_demo.log          # M1 demo results
```

## Prerequisites

Before running tests, ensure you have:

1. **Rust Toolchain**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Solana CLI** (optional but recommended)
   ```bash
   sh -c "$(curl -sSfL https://release.solana.com/stable/install)"
   ```

3. **.env File** (auto-created if missing)
   ```bash
   # .env
   SOLANA_URL=https://api.devnet.solana.com
   # WALLET_PRIVATE_KEY=your_key_here  # Optional
   ```

## What Gets Tested

### Basic Functionality Tests

1. **Nonce Bundle Management**
   - Creates/refreshes nonce bundle
   - Verifies bundle file creation

2. **Simple Transaction Creation**
   - Creates SOL transfer with nonce account
   - Verifies transaction serialization

3. **Offline Transaction Flow**
   - Complete offline-to-online workflow
   - Transaction creation, compression, fragmentation
   - Reassembly and submission

### Advanced Functionality Tests

1. **SPL Token Transaction**
   - SPL token transfer with nonce account
   - Associated token account handling

2. **Governance Voting**
   - Creates governance vote transaction
   - Uses nonce account for durability

3. **Multi-Party Signing**
   - Creates unsigned transaction
   - Demonstrates signature addition

4. **Transaction Relaying**
   - Presigned transaction relaying
   - BLE mesh simulation

5. **Bundle Management**
   - Full bundle management workflow
   - Nonce usage tracking

### M1 Demo

- Creates 50 nonce accounts
- Generates 50 offline transactions
- Simulates 5-minute offline period
- Submits all transactions successfully
- Verifies 50+ successful transactions

## Troubleshooting

### Build Errors

If you encounter build errors:

```bash
# Clean and rebuild
cargo clean
cargo build --all-targets --examples
```

### Test Failures

1. **Check individual test logs** in `test_results/TIMESTAMP/`
2. **Verify .env configuration** (RPC URL, wallet key)
3. **Check Solana network connectivity**
4. **Ensure sufficient wallet balance** (tests request airdrops automatically)

### M1 Demo Failures

The M1 demo requires:
- Sufficient wallet balance (~10 SOL for 50 nonce accounts)
- Stable RPC connection
- ~5-10 minutes runtime

If it fails:
- Check `test_results/TIMESTAMP/m1_demo.log`
- Verify RPC endpoint is accessible
- Ensure wallet has sufficient funds

## Manual Testing

You can also run individual examples manually:

```bash
# Basic examples
cargo run --example nonce_refresh_utility
cargo run --example create_nonce_transaction
cargo run --example offline_transaction_flow

# Advanced examples
cargo run --example create_spl_nonce_transaction
cargo run --example cast_governance_vote
cargo run --example create_unsigned_transaction

# M1 demo
cargo run --example m1_demo_50_transactions
```

## Continuous Integration

For CI/CD pipelines:

```bash
# Quick validation
./scripts/test_pollinet.sh --quick

# Full validation
./scripts/test_pollinet.sh --full
```

## Related Documentation

- [Offline Transactions Guide](OFFLINE_TRANSACTIONS_GUIDE.md) - Complete guide including CLI usage and examples
- [M1 Reproducibility Guide](M1_REPRODUCIBILITY_GUIDE.md) - M1 demo reproduction steps
- [README](README.md) - Project overview

## Support

If you encounter issues:

1. Check the test logs in `test_results/`
2. Review the documentation files above
3. Verify your environment setup
4. Check Solana network status

For more information, see the [PolliNet documentation](https://pollinet.github.io/pollinet/).

