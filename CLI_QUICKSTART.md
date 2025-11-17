# PolliNet CLI Quickstart

PolliNet ships a lightweight CLI wrapper that stitches together the existing Rust examples so you can reproduce the full offline → BLE relay → on-chain flow without memorising individual `cargo run --example ...` invocations.

## Prerequisites

1. Install Rust (nightly not required) and the Solana CLI dependencies described in `README.md`.
2. Ensure you have a funded devnet keypair in `~/.config/solana/id.json` – the examples use it as the sender/fee payer.
3. From the repo root run:
   ```bash
   cargo fetch
   ```

## CLI Usage

All commands live in `scripts/pollinet_cli.sh`. Each run captures logs under `cli_logs/` so you can review artefacts or attach them to bug reports.

```bash
./scripts/pollinet_cli.sh help
```

### 1. Prepare Nonce Data & Pre-Sign Bundles

This creates/refreshes nonce accounts, generates offline bundles, and exercises the fragmentation logic.

```bash
./scripts/pollinet_cli.sh prepare
```

Outputs:
- `offline_bundle.json` (updated)
- Example BLE logs under `ble_mesh_logs/`
- CLI log file: `cli_logs/<timestamp>_prepare_offline_bundle.log`

### 2. Relay a Presigned Transaction via BLE (Simulation on Desktop)

```bash
./scripts/pollinet_cli.sh relay
```

This runs the `relay_presigned_transaction` example which compresses, fragments, and “broadcasts” over the simulated BLE adapter. Use the log to verify checksum/fragment counts.

### 3. Submit Offline Transactions to Solana

```bash
./scripts/pollinet_cli.sh submit
```

The `offline_transaction_flow` example decompresses offline payloads, verifies nonces, and submits them to devnet. Successful submissions are echoed in the log with Explorer URLs.

### Optional: Refresh Nonces Only

If you simply need to refresh cached nonce data without creating transactions:

```bash
./scripts/pollinet_cli.sh refresh-nonces
```

## Automation via `generate_m1_demo.sh`

For CI-style verification (formatting, clippy, tests, plus the CLI flows), run:

```bash
./generate_m1_demo.sh
```

The script produces:
- `demo_artifacts/<timestamp>/...` – build/test/CLI logs.
- `M1_DEMO_RESULTS.md` – summary with links to the log files and any captured Solana signatures.

Use the CLI logs to trace each stage if reviewers need to audit the execution trace. The CLI commands themselves are safe to re-run; they automatically refresh nonce data and overwrite summary files where necessary.*** End Patch```} to=functions.apply_patch ***!

