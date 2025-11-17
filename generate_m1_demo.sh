#!/usr/bin/env bash
# Comprehensive demo harness: formats, lints, tests, then runs the CLI flows.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ARTIFACT_ROOT="${ROOT_DIR}/demo_artifacts"
RUN_ID="$(date +%Y%m%d_%H%M%S)"
RUN_DIR="${ARTIFACT_ROOT}/${RUN_ID}"
mkdir -p "${RUN_DIR}"

log_and_run() {
  local name="$1"
  shift
  local log_file="${RUN_DIR}/${name}.log"
  echo ""
  echo ">>> ${name}"
  echo "    logging to ${log_file}"
  (
    cd "${ROOT_DIR}"
    "$@"
  ) | tee "${log_file}"
}

echo "PolliNet Demo Run - ${RUN_ID}" > "${ROOT_DIR}/M1_DEMO_RESULTS.md"
echo "==========================================" >> "${ROOT_DIR}/M1_DEMO_RESULTS.md"
echo "" >> "${ROOT_DIR}/M1_DEMO_RESULTS.md"

# Build/Test suite
log_and_run "cargo_fmt" cargo fmt --all
log_and_run "cargo_clippy" cargo clippy --all-targets -- -D warnings
log_and_run "cargo_test" cargo test --all

# CLI flows
log_and_run "cli_prepare" ./scripts/pollinet_cli.sh prepare
log_and_run "cli_relay" ./scripts/pollinet_cli.sh relay
log_and_run "cli_submit" ./scripts/pollinet_cli.sh submit

# Capture any signatures seen during the submit step
SIGNATURES=$(grep -h "Signature:" "${RUN_DIR}/cli_submit.log" || true)

{
  echo "## Build & Test Logs"
  echo "- fmt: ${RUN_DIR}/cargo_fmt.log"
  echo "- clippy: ${RUN_DIR}/cargo_clippy.log"
  echo "- test: ${RUN_DIR}/cargo_test.log"
  echo ""
  echo "## CLI Flow Logs"
  echo "- prepare: ${RUN_DIR}/cli_prepare.log"
  echo "- relay: ${RUN_DIR}/cli_relay.log"
  echo "- submit: ${RUN_DIR}/cli_submit.log"
  echo ""
  echo "## Transaction Signatures"
  if [[ -n "${SIGNATURES}" ]]; then
    echo "${SIGNATURES}" | sed 's/Signature:/- Signature:/'
  else
    echo "- (none captured – check submit log)"
  fi
} >> "${ROOT_DIR}/M1_DEMO_RESULTS.md"

echo ""
echo "Demo complete. Full artifact set in ${RUN_DIR}"
echo "Summary written to M1_DEMO_RESULTS.md"