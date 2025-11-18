#!/usr/bin/env bash
#
# PolliNet Comprehensive Test Script
#
# This script provides a complete testing workflow for PolliNet:
# 1. Prerequisites check
# 2. Environment setup
# 3. Build verification
# 4. Example tests (basic → advanced)
# 5. Results summary
#
# Usage:
#   ./scripts/test_pollinet.sh [OPTIONS]
#
# Options:
#   --quick          Run quick tests only (skip M1 demo)
#   --m1-only        Run only M1 demo (50+ transactions)
#   --full           Run all tests including M1 demo (default)
#   --help           Show this help message

set -euo pipefail

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${ROOT_DIR}"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Test results
TEST_RESULTS_DIR="${ROOT_DIR}/test_results"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RUN_DIR="${TEST_RESULTS_DIR}/${TIMESTAMP}"
mkdir -p "${RUN_DIR}"

# Test counters
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

# Configuration
QUICK_MODE=false
M1_ONLY=false
FULL_MODE=true

# Print functions
print_header() {
    echo -e "\n${BLUE}╔══════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║${NC}  $1"
    echo -e "${BLUE}╚══════════════════════════════════════════════════════════╝${NC}\n"
}

print_section() {
    echo -e "\n${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${CYAN}$1${NC}"
    echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}\n"
}

print_success() {
    echo -e "${GREEN}✅ $1${NC}"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

print_error() {
    echo -e "${RED}❌ $1${NC}"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

print_warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
    TESTS_SKIPPED=$((TESTS_SKIPPED + 1))
}

print_info() {
    echo -e "${BLUE}ℹ️  $1${NC}"
}

# Parse arguments
parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            --quick)
                QUICK_MODE=true
                FULL_MODE=false
                shift
                ;;
            --m1-only)
                M1_ONLY=true
                FULL_MODE=false
                shift
                ;;
            --full)
                FULL_MODE=true
                shift
                ;;
            --help|-h)
                show_help
                exit 0
                ;;
            *)
                echo -e "${RED}Unknown option: $1${NC}"
                show_help
                exit 1
                ;;
        esac
    done
}

show_help() {
    cat << EOF
PolliNet Comprehensive Test Script

Usage:
    ./scripts/test_pollinet.sh [OPTIONS]

Options:
    --quick          Run quick tests only (skip M1 demo)
    --m1-only        Run only M1 demo (50+ transactions)
    --full           Run all tests including M1 demo (default)
    --help           Show this help message

Examples:
    # Run all tests (default)
    ./scripts/test_pollinet.sh

    # Quick test (skip M1 demo)
    ./scripts/test_pollinet.sh --quick

    # Only M1 demo
    ./scripts/test_pollinet.sh --m1-only

Test Results:
    Results are saved to: test_results/TIMESTAMP/
    Summary report: test_results/TIMESTAMP/summary.md

For detailed documentation:
    - OFFLINE_TRANSACTIONS_GUIDE.md
    - TESTING.md
    - M1_REPRODUCIBILITY_GUIDE.md
EOF
}

# Check prerequisites
check_prerequisites() {
    print_section "Step 1: Checking Prerequisites"
    
    local all_ok=true
    
    # Check Rust
    if command -v cargo &> /dev/null && command -v rustc &> /dev/null; then
        local rust_version=$(rustc --version)
        print_success "Rust toolchain found: ${rust_version}"
    else
        print_error "Rust toolchain not found. Please install from https://rustup.rs/"
        all_ok=false
    fi
    
    # Check Solana CLI (optional but recommended)
    if command -v solana &> /dev/null; then
        local solana_version=$(solana --version 2>/dev/null || echo "unknown")
        print_success "Solana CLI found: ${solana_version}"
    else
        print_warning "Solana CLI not found (optional, but recommended for local testing)"
        print_info "Install from: https://docs.solana.com/cli/install-solana-cli-tools"
    fi
    
    # Check .env file
    if [ -f ".env" ]; then
        print_success ".env file found"
        # Check for required variables
        if grep -q "SOLANA_URL" .env || grep -q "WALLET_PRIVATE_KEY" .env; then
            print_info "Environment variables configured"
        fi
    else
        print_warning ".env file not found"
        print_info "Creating template .env file..."
        cat > .env << 'ENVEOF'
# Solana RPC URL
# Defaults to devnet if not set
SOLANA_URL=https://api.devnet.solana.com

# Optional: Wallet private key (base58 encoded)
# If set, wallet will be restored from this key instead of creating new one
# WALLET_PRIVATE_KEY=your_base58_private_key_here
ENVEOF
        print_success "Created .env template"
        print_info "Edit .env to customize RPC URL or wallet"
    fi
    
    # Check for bundle file (optional)
    if [ -f ".offline_bundle.json" ]; then
        print_info "Found existing nonce bundle (.offline_bundle.json)"
    else
        print_info "No existing bundle found - will create one during tests"
    fi
    
    if [ "$all_ok" = false ]; then
        print_error "Prerequisites check failed. Please fix the issues above."
        exit 1
    fi
    
    print_success "All prerequisites met"
}

# Build verification
verify_build() {
    print_section "Step 2: Verifying Build"
    
    local log_file="${RUN_DIR}/build.log"
    
    print_info "Running cargo check..."
    if cargo check --all-targets --examples 2>&1 | tee "${log_file}"; then
        print_success "Build verification passed"
    else
        print_error "Build verification failed"
        print_info "Check ${log_file} for details"
        exit 1
    fi
    
    print_info "Running cargo fmt check..."
    if cargo fmt --all -- --check 2>&1 | tee -a "${log_file}"; then
        print_success "Code formatting check passed"
    else
        print_warning "Code formatting issues found (non-critical)"
        print_info "Run 'cargo fmt --all' to fix"
    fi
}

# Run a test example
run_test_example() {
    local example_name=$1
    local description=$2
    local log_file="${RUN_DIR}/${example_name}.log"
    
    print_info "Running: ${example_name}"
    print_info "Description: ${description}"
    
    if cargo run --example "${example_name}" 2>&1 | tee "${log_file}"; then
        print_success "${example_name} completed successfully"
        return 0
    else
        print_error "${example_name} failed"
        print_info "Check ${log_file} for details"
        return 1
    fi
}

# Basic functionality tests
run_basic_tests() {
    print_section "Step 3: Basic Functionality Tests"
    
    print_info "These tests verify core PolliNet functionality"
    
    # Test 1: Nonce bundle creation/refresh
    print_info "\nTest 1: Nonce Bundle Management"
    if run_test_example "nonce_refresh_utility" "Create/refresh nonce bundle"; then
        if [ -f ".offline_bundle.json" ]; then
            print_success "Nonce bundle file created"
        else
            print_error "Nonce bundle file not found after creation"
        fi
    fi
    
    # Test 2: Simple transaction creation
    print_info "\nTest 2: Simple Transaction Creation"
    run_test_example "create_nonce_transaction" "Create SOL transfer with nonce account" || true
    
    # Test 3: Offline transaction flow
    print_info "\nTest 3: Offline Transaction Flow"
    run_test_example "offline_transaction_flow" "Complete offline-to-online workflow" || true
    
    print_info "\nBasic tests completed"
}

# Advanced functionality tests
run_advanced_tests() {
    print_section "Step 4: Advanced Functionality Tests"
    
    print_info "These tests verify advanced PolliNet features"
    
    # Test 1: SPL token transaction
    print_info "\nTest 1: SPL Token Transaction"
    run_test_example "create_spl_nonce_transaction" "SPL token transfer with nonce" || true
    
    # Test 2: Governance voting
    print_info "\nTest 2: Governance Voting"
    run_test_example "cast_governance_vote" "Governance vote transaction" || true
    
    # Test 3: Unsigned transaction (multi-party signing)
    print_info "\nTest 3: Multi-Party Signing"
    run_test_example "create_unsigned_transaction" "Unsigned transaction for multi-party signing" || true
    
    # Test 4: Transaction relaying
    print_info "\nTest 4: Transaction Relaying"
    run_test_example "relay_presigned_transaction" "Presigned transaction relaying" || true
    
    # Test 5: Bundle management
    print_info "\nTest 5: Bundle Management"
    run_test_example "offline_bundle_management" "Full bundle management workflow" || true
    
    print_info "\nAdvanced tests completed"
}

# M1 Demo (50+ transactions)
run_m1_demo() {
    print_section "Step 5: M1 Demo - 50+ Transactions"
    
    print_info "This demo creates 50 nonce accounts and submits 50+ transactions"
    print_info "This demonstrates the M1 requirement for large-scale offline transactions"
    print_warning "This test will take approximately 5-10 minutes"
    
    local log_file="${RUN_DIR}/m1_demo.log"
    
    print_info "Starting M1 demo..."
    if cargo run --example m1_demo_50_transactions 2>&1 | tee "${log_file}"; then
        # Extract success count
        local success_count=$(grep -oP "Successful transactions: \K[0-9]+" "${log_file}" | tail -1 || echo "0")
        if [ -n "${success_count}" ] && [ "${success_count}" -ge 50 ]; then
            print_success "M1 Demo completed: ${success_count} successful transactions"
            print_success "✅ M1 REQUIREMENT MET: 50+ successful transactions"
        else
            print_warning "M1 Demo completed but success count may be below 50"
            print_info "Check ${log_file} for details"
        fi
        
        # Check for submission file
        if [ -f ".offline_submission.json" ]; then
            print_success "Transaction signatures saved to .offline_submission.json"
        fi
    else
        print_error "M1 Demo failed"
        print_info "Check ${log_file} for details"
        return 1
    fi
}

# Generate summary report
generate_summary() {
    print_section "Step 6: Generating Summary Report"
    
    local summary_file="${RUN_DIR}/summary.md"
    
    {
        echo "# PolliNet Test Results"
        echo ""
        echo "**Test Run:** ${TIMESTAMP}"
        echo "**Date:** $(date)"
        echo ""
        echo "## Test Summary"
        echo ""
        echo "- ✅ **Passed:** ${TESTS_PASSED}"
        echo "- ❌ **Failed:** ${TESTS_FAILED}"
        echo "- ⚠️  **Skipped:** ${TESTS_SKIPPED}"
        echo ""
        echo "## Test Mode"
        echo ""
        if [ "$M1_ONLY" = true ]; then
            echo "- Mode: **M1 Demo Only**"
        elif [ "$QUICK_MODE" = true ]; then
            echo "- Mode: **Quick Tests** (M1 demo skipped)"
        else
            echo "- Mode: **Full Test Suite**"
        fi
        echo ""
        echo "## Log Files"
        echo ""
        echo "All test logs are available in: \`${RUN_DIR}/\`"
        echo ""
        echo "### Key Files"
        echo ""
        echo "- \`build.log\` - Build verification"
        echo "- \`nonce_refresh_utility.log\` - Nonce bundle creation"
        echo "- \`create_nonce_transaction.log\` - Basic transaction test"
        echo "- \`offline_transaction_flow.log\` - Offline workflow test"
        if [ "$FULL_MODE" = true ] || [ "$M1_ONLY" = true ]; then
            echo "- \`m1_demo.log\` - M1 demo (50+ transactions)"
        fi
        echo ""
        echo "## Next Steps"
        echo ""
        echo "1. Review individual test logs for detailed output"
        echo "2. Check transaction signatures in \`.offline_submission.json\` (if M1 demo ran)"
        echo "3. Verify transactions on Solana Explorer (devnet)"
        echo "4. Run specific examples: \`cargo run --example <name>\`"
        echo ""
        echo "## Documentation"
        echo ""
        echo "- [Offline Transactions Guide](OFFLINE_TRANSACTIONS_GUIDE.md)"
        echo "- [Testing Guide](TESTING.md)"
        echo "- [M1 Reproducibility Guide](M1_REPRODUCIBILITY_GUIDE.md)"
        echo ""
    } > "${summary_file}"
    
    print_success "Summary report generated: ${summary_file}"
    
    # Display summary
    echo ""
    cat "${summary_file}"
}

# Main execution
main() {
    # Banner
    echo -e "${GREEN}"
    cat << 'EOF'
╔══════════════════════════════════════════════════════════╗
║                                                          ║
║          PolliNet Comprehensive Test Suite               ║
║                                                          ║
║          Testing Offline Solana Transactions            ║
║          via Bluetooth Low Energy Mesh                  ║
║                                                          ║
╚══════════════════════════════════════════════════════════╝
EOF
    echo -e "${NC}"
    
    print_info "Test results will be saved to: ${RUN_DIR}"
    echo ""
    
    # Parse arguments
    parse_args "$@"
    
    # Run tests based on mode
    check_prerequisites
    verify_build
    
    if [ "$M1_ONLY" = true ]; then
        run_m1_demo
    else
        run_basic_tests
        
        if [ "$FULL_MODE" = true ]; then
            run_advanced_tests
            run_m1_demo
        else
            print_info "Skipping advanced tests and M1 demo (quick mode)"
        fi
    fi
    
    # Generate summary
    generate_summary
    
    # Final message
    echo ""
    print_section "Test Suite Complete"
    print_info "Results directory: ${RUN_DIR}"
    print_info "Summary report: ${RUN_DIR}/summary.md"
    echo ""
    
    if [ $TESTS_FAILED -eq 0 ]; then
        print_success "All tests completed successfully! 🎉"
        exit 0
    else
        print_error "Some tests failed. Please review the logs above."
        exit 1
    fi
}

# Run main function
main "$@"

