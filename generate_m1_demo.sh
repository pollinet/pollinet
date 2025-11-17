#!/bin/bash
# generate_m1_demo.sh

echo "PolliNet M1 Demonstration - $(date)" > M1_DEMO_RESULTS.md
echo "================================" >> M1_DEMO_RESULTS.md
echo "" >> M1_DEMO_RESULTS.md
echo "## Transaction Signatures" >> M1_DEMO_RESULTS.md
echo "" >> M1_DEMO_RESULTS.md

# Run different examples and capture signatures
for i in {1..5}; do
  echo "Running transaction $i..."
  cargo run --example create_nonce_transaction 2>&1 | \
    grep "Signature:" | \
    awk '{print "- https://solscan.io/tx/"$3"?cluster=devnet"}' >> M1_DEMO_RESULTS.md
  sleep 5  # Avoid rate limiting
done

echo "" >> M1_DEMO_RESULTS.md
echo "Total successful transactions: $(grep -c 'explorer.solana.com' M1_DEMO_RESULTS.md)" >> M1_DEMO_RESULTS.md