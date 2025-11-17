#!/usr/bin/env bash
# Lightweight CLI wrapper around the existing PolliNet Rust examples.
# Provides a single entry-point for preparing nonce bundles, relaying
# transactions via BLE (or simulation), and submitting them on-chain.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_ROOT="${ROOT_DIR}/cli_logs"
mkdir -p "${LOG_ROOT}"

timestamp() {
  date +"%Y-%m-%d %H:%M:%S"
}

log_section() {
  local msg="$1"
  echo ""
  echo "[$(timestamp)] ${msg}"
  echo "------------------------------------------------------------"
}

run_example() {
  local label="$1"
  shift
  local log_file="${LOG_ROOT}/$(date +%Y%m%d_%H%M%S)_${label}.log"
  log_section "Running ${label}"
  (
    cd "${ROOT_DIR}"
    cargo run --example "$@" 2>&1
  ) | tee "${log_file}"
  echo "Logs saved to ${log_file}"
}

show_help() {
  cat <<EOF
PolliNet CLI Wrapper
Usage: $0 <command>

Commands:
  prepare         Prepare/refresh nonce bundles and create offline payloads
  relay           Process a presigned transaction and relay it via BLE (simulation on desktop)
  submit          Submit offline-created transactions to Solana devnet
  refresh-nonces  Refresh cached nonce data without creating transactions
  help            Show this message

Logs for each run are stored in ${LOG_ROOT}.
EOF
}

cmd="${1:-help}"
shift || true

case "${cmd}" in
  prepare)
    run_example "prepare_offline_bundle" offline_bundle_management
    ;;
  relay)
    run_example "relay_presigned_transaction" relay_presigned_transaction
    ;;
  submit)
    run_example "offline_transaction_flow" offline_transaction_flow
    ;;
  refresh-nonces)
    run_example "nonce_refresh_utility" nonce_refresh_utility
    ;;
  help|--help|-h)
    show_help
    ;;
  *)
    echo "Unknown command: ${cmd}"
    echo ""
    show_help
    exit 1
    ;;
esac

