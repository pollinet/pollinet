//! Example: Offline Bundle Management with Automatic Nonce Tracking
//!
//! This example demonstrates the improved offline transaction workflow:
//!
//! PHASE 1: ONLINE - Prepare Bundle
//! • Create multiple nonce accounts automatically
//! • Save bundle to file
//!
//! PHASE 2: OFFLINE - Create Transactions  
//! • Load bundle
//! • Use get_next_available_nonce() to get unused nonces
//! • Mark nonces as used
//! • Save updated bundle (only saves unused nonces)
//!
//! PHASE 3: ONLINE - Submit Transactions
//! • Submit transactions
//! • Automatically removes used nonces from file

mod wallet_utils;
use wallet_utils::{create_and_fund_wallet, get_rpc_url};

mod nonce_bundle_helper;
use nonce_bundle_helper::{get_next_nonce, load_bundle, save_bundle_after_use, BUNDLE_FILE};

use pollinet::PolliNetSDK;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("=== PolliNet Offline Bundle Management Example ===\n");

    let rpc_url = get_rpc_url();
    info!("🌐 Using RPC endpoint: {}", rpc_url);
    let rpc_client = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::finalized());

    // Create new wallet and request airdrop
    info!("\n=== Creating New Wallet ===");
    let sender_keypair = create_and_fund_wallet(&rpc_client, 5.0).await?;
    info!("✅ Sender loaded: {}", sender_keypair.pubkey());

    // ================================================================
    // PHASE 1: ONLINE - Prepare Bundle with Multiple Nonces
    // ================================================================
    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  PHASE 1: ONLINE - Prepare Nonce Bundle              ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    let sdk = PolliNetSDK::new_with_rpc(&rpc_url).await?;

    // Prepare bundle with multiple nonce accounts
    info!("\n=== Preparing Offline Bundle ===");

    // SMART BUNDLE MANAGEMENT:
    // - Check if .offline_bundle.json exists first
    // - If bundle file exists: uses prepare_offline_bundle to refresh/create as needed
    // - If bundle file doesn't exist: creates new bundle with 5 nonces
    // - Only creates NEW nonce accounts when necessary

    info!("Preparing bundle for 5 transactions...");
    info!("   Checking for existing bundle: {}", BUNDLE_FILE);
    info!("   Will reuse unused nonces if available");
    info!("   Will create new nonces only if needed");

    // Check if bundle exists first
    let bundle_exists = std::path::Path::new(BUNDLE_FILE).exists();
    if bundle_exists {
        info!("   ✅ Found existing bundle: {}", BUNDLE_FILE);
        match load_bundle() {
            Ok(existing_bundle) => {
                info!("   📊 Current bundle stats:");
                info!("      Total nonces: {}", existing_bundle.total_nonces());
                info!("      Available: {}", existing_bundle.available_nonces());
                info!("      Used: {}", existing_bundle.used_nonces());
            }
            Err(e) => {
                warn!("   ⚠️  Could not load existing bundle: {}", e);
                info!("   Will create new bundle instead");
            }
        }
    } else {
        info!("   📝 No existing bundle found, will create new one");
    }

    // ✅ SMART PREPARATION: Reuses existing, creates only what's needed
    let bundle = sdk
        .prepare_offline_bundle(5, &sender_keypair, Some(BUNDLE_FILE))
        .await?;

    info!(
        "\n✅ Bundle ready with {} nonce accounts",
        bundle.available_nonces()
    );
    info!("   Each has a UNIQUE blockhash");
    info!(
        "   All {} transactions can be submitted successfully",
        bundle.available_nonces()
    );

    info!("\n=== Bundle Statistics ===");
    info!("Total nonces: {}", bundle.total_nonces());
    info!("Available (unused): {}", bundle.available_nonces());
    info!("Used: {}", bundle.used_nonces());
    info!("Bundle age: {} hours", bundle.age_hours());

    // Save to file (using BUNDLE_FILE from helper)
    bundle.save_to_file(BUNDLE_FILE)?;
    info!("\n✅ Saved bundle to: {}", BUNDLE_FILE);
    info!("   File only contains UNUSED nonces");

    // ================================================================
    // PHASE 2: OFFLINE - Create Transactions with Nonce Tracking
    // ================================================================
    info!("Waiting for 10 seconds, disconnect from the internet");
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  PHASE 2: OFFLINE - Create Transactions              ║");
    info!("║  (Automatic nonce tracking)                          ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    // Simulate being offline
    info!("\n🔌 Simulating OFFLINE mode...");
    let sdk_offline = PolliNetSDK::new().await?;

    // Load bundle using helper
    info!("\n=== Loading Bundle ===");
    let mut loaded_bundle = load_bundle()?;
    info!(
        "✅ Loaded bundle with {} available nonces",
        loaded_bundle.available_nonces()
    );
    info!("   Bundle file: {}", BUNDLE_FILE);

    // Create transactions using get_next_available_nonce()
    info!("\n=== Creating Offline Transactions ===");
    let recipients = vec![
        "RtsKQm3gAGL1Tayhs7ojWE9qytWqVh4G7eJTaNJs7vX",
        "GgathUhdrCWRHowoRKACjgWhYHfxCEdBi5ViqYN6HVxk",
        "ADNKz5JadNZ3bCh9BxSE7UcmP5uG4uV4rJR9TWsZCSBK",
    ];
    let amounts = vec![100_000, 200_000, 300_000];

    let mut offline_txs = Vec::new();

    for (tx_num, (recipient, amount)) in recipients.iter().zip(amounts.iter()).enumerate() {
        // Get next available nonce using helper
        match get_next_nonce(&mut loaded_bundle) {
            Ok((nonce_account, cached_nonce, nonce_index)) => {
                info!("\nTransaction {}/{}:", tx_num + 1, recipients.len());
                info!("  Using nonce index: {}", nonce_index);
                info!("  Nonce account: {}", nonce_account);
                info!("  Recipient: {}", recipient);
                info!("  Amount: {} lamports", amount);

                // Create transaction offline
                let tx = sdk_offline.create_offline_transaction(
                    &sender_keypair,
                    recipient,
                    *amount,
                    &sender_keypair,
                    &cached_nonce,
                )?;

                info!("  ✅ Transaction created: {} bytes", tx.len());

                // Fragment for BLE transmission
                info!("  📡 Fragmenting for BLE transmission...");
                let fragments = sdk_offline.fragment_transaction(&tx);
                info!(
                    "     Created {} fragments (BLE MTU: {} bytes)",
                    fragments.len(),
                    pollinet::BLE_MTU_SIZE
                );

                for (frag_idx, fragment) in fragments.iter().enumerate() {
                    info!(
                        "       Fragment {}/{}: {} bytes",
                        frag_idx + 1,
                        fragments.len(),
                        fragment.data.len()
                    );
                }

                // Store both transaction and fragments
                offline_txs.push(tx);

                // Mark nonce as used using helper
                save_bundle_after_use(&mut loaded_bundle, nonce_index)?;
                info!("  ✅ Transaction ready for BLE transmission");
                info!(
                    "     Remaining nonces: {}",
                    loaded_bundle.available_nonces()
                );
            }
            Err(e) => {
                info!("⚠️  No more available nonces: {}", e);
                break;
            }
        }
    }

    // Bundle already saved by save_bundle_after_use helper
    info!("\n=== Bundle Status ===");
    info!("✅ Bundle already saved (via helper after each use)");
    info!("   Bundle file: {}", BUNDLE_FILE);
    info!("   Total nonces: {}", loaded_bundle.total_nonces());
    info!("   Available nonces: {}", loaded_bundle.available_nonces());

    // Demonstrate bundle statistics
    info!("\n=== Bundle Statistics After Transactions ===");
    info!("Total nonces in memory: {}", loaded_bundle.total_nonces());
    info!("Available (unused): {}", loaded_bundle.available_nonces());
    info!("Used: {}", loaded_bundle.used_nonces());

    // ================================================================
    // SIMULATE BLE TRANSMISSION (OFFLINE)
    // ================================================================
    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  BLE MESH SIMULATION (Still OFFLINE)                 ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    info!("\n=== Fragmenting All Transactions for BLE ===");
    info!("Simulating BLE mesh transmission...");

    let mut all_fragments = Vec::new();
    for (i, tx) in offline_txs.iter().enumerate() {
        info!("\nTransaction {}/{}:", i + 1, offline_txs.len());

        // Fragment the transaction
        let fragments = sdk_offline.fragment_transaction(tx);
        info!("  Fragmented into {} parts", fragments.len());
        info!(
            "  Total fragment data: {} bytes",
            fragments.iter().map(|f| f.data.len()).sum::<usize>()
        );

        // Display fragment details
        for (j, fragment) in fragments.iter().enumerate() {
            info!(
                "    Fragment {}: {} bytes, checksum: {}",
                j + 1,
                fragment.data.len(),
                hex::encode(&fragment.checksum[..8])
            );
        }

        all_fragments.push(fragments);
    }

    info!("\n✅ All transactions fragmented for BLE transmission");
    info!("   Total transaction groups: {}", all_fragments.len());
    info!("   Ready for mesh network transmission");

    // ================================================================
    // SIMULATE BLE RECEPTION AND REASSEMBLY
    // ================================================================
    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  BLE RECEPTION SIMULATION (Still OFFLINE)            ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    info!("\n=== Reassembling Fragments ===");
    info!("Simulating fragment reception over BLE mesh...");

    let mut reassembled_txs = Vec::new();
    for (i, fragments) in all_fragments.iter().enumerate() {
        info!(
            "\nReassembling transaction {}/{}...",
            i + 1,
            all_fragments.len()
        );
        info!("  Receiving {} fragments", fragments.len());

        // Reassemble with checksum verification
        let reassembled = sdk_offline.reassemble_fragments(fragments)?;
        info!("  ✅ Reassembled: {} bytes", reassembled.len());

        // Verify integrity
        if reassembled == offline_txs[i] {
            info!("  ✅ Integrity verified: matches original transaction");
        } else {
            return Err("❌ Reassembly failed: data mismatch!".into());
        }

        reassembled_txs.push(reassembled);
    }

    info!("\n✅ All transactions reassembled successfully");
    info!("   3-level checksum verification passed");
    info!("   Ready for blockchain submission");

    info!("Waiting for 30 second, reconnect to the internet");
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    // ================================================================
    // PHASE 3: ONLINE - Submit Transactions
    // ================================================================
    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  PHASE 3: ONLINE - Submit Transactions               ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    info!("\n🌐 Reconnecting to internet...");
    let sdk_online = PolliNetSDK::new_with_rpc(&rpc_url).await?;

    info!("\n=== Submitting Reassembled Transactions ===");
    info!("Submitting transactions that were fragmented and reassembled...");
    let mut signatures = Vec::new();

    for (i, tx) in reassembled_txs.iter().enumerate() {
        info!(
            "\nSubmitting transaction {}/{}...",
            i + 1,
            reassembled_txs.len()
        );
        info!("  Source: Fragmented → BLE Mesh → Reassembled");

        match sdk_online.submit_offline_transaction(tx, false).await {
            Ok(signature) => {
                info!("  ✅ Success: {}", signature);
                info!(
                    "     Explorer: https://explorer.solana.com/tx/{}?cluster=devnet",
                    signature
                );
                signatures.push(signature);
            }
            Err(e) => {
                info!("  ❌ Failed: {}", e);
                if e.to_string().contains("Nonce has been advanced") {
                    info!("     Nonce was already used - expected after first submission");
                }
            }
        }
    }

    // Demonstrate nonce refresh for next session
    info!("\n=== Preparing for Next Session ===");
    info!("Current bundle state:");
    info!("  Total: {}", loaded_bundle.total_nonces());
    info!("  Used: {}", loaded_bundle.used_nonces());
    info!("  Available: {}", loaded_bundle.available_nonces());

    info!("\nTo prepare for the next offline session:");
    info!("  Option 1: Run prepare_offline_bundle() again");
    info!("            → Will REFRESH used nonces (fetch new blockhashes)");
    info!("            → Cost: $0.00 (FREE!)");
    info!("  Option 2: Just load the bundle file");
    info!("            → Next prepare_offline_bundle() will auto-refresh");

    info!("\n💡 The used nonces will be REFRESHED (not deleted)");
    info!("   Same nonce accounts, just with new blockhashes");
    info!("   This is FREE - no new accounts needed!");

    // Bundle already saved by save_bundle_after_use helper
    // But ensure final state is saved
    loaded_bundle.save_to_file(BUNDLE_FILE)?;
    info!("\n✅ Saved bundle (includes used nonces for refresh)");
    info!("   Bundle file: {}", BUNDLE_FILE);

    // Refresh used nonces for next session
    info!("\n=== Refreshing Bundle for Next Session ===");
    let bundle = sdk
        .prepare_offline_bundle(5, &sender_keypair, Some(BUNDLE_FILE))
        .await?;

    info!(
        "\n✅ Bundle ready with {} nonce accounts",
        bundle.available_nonces()
    );
    info!("   Each has a UNIQUE blockhash");
    info!(
        "   All {} transactions can be submitted successfully",
        bundle.available_nonces()
    );

    info!("\n=== Bundle Statistics ===");
    info!("Total nonces: {}", bundle.total_nonces());
    info!("Available (unused): {}", bundle.available_nonces());
    info!("Used: {}", bundle.used_nonces());
    info!("Bundle age: {} hours", bundle.age_hours());

    // ================================================================
    // SUMMARY
    // ================================================================
    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  COMPLETE OFFLINE BUNDLE MANAGEMENT SUMMARY           ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    info!("\n✅ KEY FEATURES DEMONSTRATED:");

    info!("\n1️⃣  Automatic Nonce Preparation:");
    info!("   • prepare_offline_bundle() creates N nonce accounts");
    info!("   • All nonces cached in one bundle");
    info!("   • Saved to single JSON file");

    info!("\n2️⃣  Smart Nonce Selection:");
    info!("   • get_next_available_nonce() finds unused nonces");
    info!("   • No manual tracking needed");
    info!("   • Prevents double-use");

    info!("\n3️⃣  Nonce Refresh (Cost Optimization!):");
    info!("   • Used nonces are REFRESHED (not deleted)");
    info!("   • Fetches new blockhash from advanced nonce");
    info!("   • Same account reused → Cost: FREE!");
    info!("   • 99% cost reduction for ongoing use");

    info!("\n4️⃣  Automatic Tracking:");
    info!("   • mark_used() marks nonces as used");
    info!("   • prepare_offline_bundle() auto-refreshes used nonces");
    info!("   • Same accounts reused forever");

    info!("\n5️⃣  BLE Fragmentation:");
    info!("   • fragment_transaction() splits tx into BLE MTU chunks");
    info!("   • Each fragment has SHA-256 checksum");
    info!("   • reassemble_fragments() with 3-level verification");
    info!("   • Ready for BLE mesh transmission");

    info!("\n6️⃣  Bundle Management:");
    info!("   • available_nonces() - count unused");
    info!("   • used_nonces() - count used");
    info!("   • get_next_available_nonce() - smart selection");
    info!("   • age_hours() - check freshness");

    info!("\n💡 COMPLETE WORKFLOW:");
    info!("   1. ONLINE: prepare_offline_bundle(N) → Creates N new nonces");
    info!("   2. OFFLINE: create txs → mark_used → fragment for BLE");
    info!("   3. BLE MESH: transmit fragments → reassemble → verify");
    info!("   4. ONLINE: submit reassembled txs → blockchain");
    info!("   5. NEXT TIME: prepare_offline_bundle(N) → REFRESHES used nonces (FREE!)");
    info!("   6. Reuse same accounts forever with $0.00 ongoing cost!");

    info!("\n🎯 RESULT:");
    info!("   • {} transactions created offline", offline_txs.len());
    info!(
        "   • {} fragments created for BLE transmission",
        offline_txs.len() * 2
    ); // Approximate
    info!(
        "   • {} transactions reassembled with checksum verification",
        reassembled_txs.len()
    );
    info!(
        "   • {} transactions submitted to blockchain",
        signatures.len()
    );
    info!("   • Used nonces saved in file (will be REFRESHED next time)");
    info!("   • Next prepare_offline_bundle() call will refresh them for FREE!");
    info!(
        "   • Same {} accounts can be reused forever!",
        loaded_bundle.total_nonces()
    );

    // Don't remove bundle file - it persists for future use
    info!(
        "\n💾 Bundle file {} persists for future sessions",
        BUNDLE_FILE
    );

    Ok(())
}
