# PolliNet Scripts

This directory contains utility scripts for testing and running PolliNet.

## Available Scripts

### `test_pollinet.sh` - Comprehensive Test Suite

**Main testing script** for PolliNet. Runs all tests and generates reports.

```bash
# Run all tests
./scripts/test_pollinet.sh

# Quick test (skip M1 demo)
./scripts/test_pollinet.sh --quick

# M1 demo only
./scripts/test_pollinet.sh --m1-only

# Help
./scripts/test_pollinet.sh --help
```

**What it does:**
- Checks prerequisites (Rust, Solana CLI, .env)
- Verifies build
- Runs basic functionality tests
- Runs advanced functionality tests
- Runs M1 demo (50+ transactions)
- Generates summary report

**Output:** `test_results/TIMESTAMP/`

### `pollinet_cli.sh` - CLI Wrapper

Lightweight wrapper around Rust examples for quick CLI access.

```bash
# Prepare nonce bundle
./scripts/pollinet_cli.sh prepare

# Relay transaction
./scripts/pollinet_cli.sh relay

# Submit transactions
./scripts/pollinet_cli.sh submit

# Refresh nonces
./scripts/pollinet_cli.sh refresh-nonces
```

**Output:** `cli_logs/`

## Quick Reference

| Script | Purpose | Duration |
|--------|---------|----------|
| `test_pollinet.sh` | Full test suite | ~10-15 min |
| `test_pollinet.sh --quick` | Quick tests | ~2-3 min |
| `test_pollinet.sh --m1-only` | M1 demo only | ~5-10 min |
| `pollinet_cli.sh` | CLI wrapper | Varies |

## See Also

- [TESTING.md](../TESTING.md) - Detailed testing guide
- [OFFLINE_TRANSACTIONS_GUIDE.md](../OFFLINE_TRANSACTIONS_GUIDE.md) - Complete guide including CLI usage and examples

