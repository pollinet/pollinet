//! Example: Complete Offline Transaction Flow
//!
//! This example demonstrates the complete workflow for offline transactions:
//!
//! PHASE 1: ONLINE - Prepare for offline use
//! 1. Create multiple nonce accounts
//! 2. Fetch and cache nonce data
//! 3. Save to file for offline use
//!
//! PHASE 2: OFFLINE - Create transactions
//! 4. Load cached nonce data from file
//! 5. Create transactions completely offline (NO internet)
//! 6. Save transactions for later submission
//!
//! PHASE 3: ONLINE - Submit transactions
//! 7. Load offline-created transactions
//! 8. Submit to Solana blockchain
//! 9. Broadcast confirmations
//!
//! This demonstrates true offline capability for PolliNet

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
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("=== PolliNet Offline Transaction Example ===\n");
    info!("This example demonstrates creating transactions COMPLETELY OFFLINE");
    info!("No internet connection is required for transaction creation!");

    let rpc_url = get_rpc_url();
    info!("🌐 Using RPC endpoint: {}", rpc_url);
    let rpc_client = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::finalized());

    // Create new wallet and request airdrop
    info!("\n=== Creating New Wallet ===");
    let sender_keypair = create_and_fund_wallet(&rpc_client, 5.0).await?;
    info!("✅ Sender loaded: {}", sender_keypair.pubkey());

    // ================================================================
    // PHASE 1: ONLINE - Prepare for Offline Use
    // ================================================================
    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  PHASE 1: ONLINE - Prepare for Offline Use           ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    let sdk = PolliNetSDK::new_with_rpc(&rpc_url).await?;

    // Load bundle from .offline_bundle.json
    info!("\n=== Loading Nonce Bundle ===");
    info!("Loading nonce accounts from {}", BUNDLE_FILE);

    let mut bundle = load_bundle()?;
    info!(
        "✅ Loaded {} nonce accounts for offline use",
        bundle.available_nonces()
    );

    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  PHASE 1 COMPLETE!                                    ║");
    info!("║  You can now disconnect from the internet            ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    // ================================================================
    // PHASE 2: OFFLINE - Create Transactions
    // ================================================================
    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  PHASE 2: OFFLINE - Create Transactions              ║");
    info!("║  (No internet connection required!)                  ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    // Simulate being offline by creating new SDK without RPC
    info!("\n🔌 Simulating OFFLINE mode (no RPC client)...");
    let sdk_offline = PolliNetSDK::new().await?;

    // Load cached nonce data from file
    info!("\n=== Loading Cached Nonce Data ===");
    let mut loaded_bundle = load_bundle()?;
    info!(
        "✅ Loaded {} cached nonce accounts",
        loaded_bundle.available_nonces()
    );
    info!("   Bundle age: {} hours", loaded_bundle.age_hours());
    info!("   Bundle file: {}", BUNDLE_FILE);

    // Create multiple transactions offline
    info!("\n=== Creating Transactions OFFLINE ===");
    let recipients = vec![
        "RtsKQm3gAGL1Tayhs7ojWE9qytWqVh4G7eJTaNJs7vX",
        "GgathUhdrCWRHowoRKACjgWhYHfxCEdBi5ViqYN6HVxk",
        "ADNKz5JadNZ3bCh9BxSE7UcmP5uG4uV4rJR9TWsZCSBK",
    ];
    let amounts = vec![100_000, 200_000, 300_000]; // Different amounts

    let mut offline_txs = Vec::new();

    let mut transaction_count = 0;
    for (recipient, amount) in recipients.iter().zip(amounts.iter()) {
        if let Some((index, cached_nonce)) = loaded_bundle.get_next_available_nonce() {
            transaction_count += 1;
            info!(
                "Creating offline transaction {}/{}...",
                transaction_count,
                recipients.len()
            );
            info!("  Recipient: {}", recipient);
            info!("  Amount: {} lamports", amount);

            let tx = sdk_offline.create_offline_transaction(
                &sender_keypair,
                recipient,
                *amount,
                &sender_keypair, // Sender is nonce authority
                cached_nonce,
            )?;

            offline_txs.push(tx);
            info!(
                "  ✅ Transaction created: {} bytes",
                offline_txs[transaction_count - 1].len()
            );
            info!("     NO INTERNET REQUIRED!");

            // Mark nonce as used
            loaded_bundle.mark_used(index)?;
        } else {
            info!("⚠️  No more available nonces!");
            break;
        }
    }

    // Save updated bundle
    loaded_bundle.save_to_file(BUNDLE_FILE)?;
    info!("✅ Saved updated bundle to {}", BUNDLE_FILE);

    info!(
        "\n✅ Created {} transactions completely OFFLINE",
        offline_txs.len()
    );

    // Save offline transactions to file
    info!("\n=== Saving Offline Transactions ===");
    let tx_file = "offline_transactions.json";
    let tx_json = serde_json::to_string_pretty(&offline_txs)?;
    std::fs::write(tx_file, tx_json)?;
    info!(
        "✅ Saved {} transactions to: {}",
        offline_txs.len(),
        tx_file
    );

    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  PHASE 2 COMPLETE!                                    ║");
    info!(
        "║  {} transactions created offline                     ║",
        offline_txs.len()
    );
    info!("║  You can now go back online to submit them           ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    // ================================================================
    // PHASE 3: ONLINE - Submit Transactions
    // ================================================================
    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  PHASE 3: ONLINE - Submit Transactions               ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    // Reconnect to internet
    info!("\n🌐 Reconnecting to internet...");
    let sdk_online = PolliNetSDK::new_with_rpc(&rpc_url).await?;

    // Load offline transactions
    info!("\n=== Loading Offline Transactions ===");
    let tx_json = std::fs::read_to_string(tx_file)?;
    let loaded_txs: Vec<Vec<u8>> = serde_json::from_str(&tx_json)?;
    info!("✅ Loaded {} transactions from file", loaded_txs.len());

    // Submit transactions
    info!("\n=== Submitting Offline Transactions ===");
    let mut signatures = Vec::new();

    for (i, tx) in loaded_txs.iter().enumerate() {
        info!("\nSubmitting transaction {}/{}...", i + 1, loaded_txs.len());
        info!("  Size: {} bytes", tx.len());

        match sdk_online.submit_offline_transaction(tx, true).await {
            Ok(signature) => {
                info!("  ✅ Transaction submitted successfully!");
                info!("     Signature: {}", signature);
                info!(
                    "     Explorer: https://explorer.solana.com/tx/{}?cluster=devnet",
                    signature
                );
                signatures.push(signature);
            }
            Err(e) => {
                info!("  ❌ Transaction failed: {}", e);
                if e.to_string().contains("Nonce has been advanced") {
                    info!("     Nonce was advanced between transactions");
                    info!("     Only the first transaction in a batch will succeed");
                    info!("     This is expected behavior with shared nonce accounts");
                }
            }
        }
    }

    // Summary
    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  COMPLETE OFFLINE TRANSACTION FLOW SUMMARY            ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    info!("\n✅ PHASE 1 (ONLINE):");
    info!(
        "   • Loaded {} nonce accounts from bundle",
        loaded_bundle.total_nonces()
    );
    info!("   • Bundle file: {}", BUNDLE_FILE);

    info!("\n✅ PHASE 2 (OFFLINE):");
    info!("   • Loaded cached nonce data");
    info!(
        "   • Created {} transactions WITHOUT internet",
        loaded_txs.len()
    );
    info!("   • Saved transactions to file");

    info!("\n✅ PHASE 3 (ONLINE):");
    info!(
        "   • Submitted {} transaction(s) successfully",
        signatures.len()
    );
    info!("   • Signatures:");
    for (i, sig) in signatures.iter().enumerate() {
        info!("     {}. {}", i + 1, sig);
    }

    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  KEY FEATURES DEMONSTRATED                            ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    info!("\n🔑 True Offline Capability:");
    info!("   • Transactions created with ZERO internet access");
    info!("   • All data from cached nonce accounts");
    info!("   • Private keys never transmitted");

    info!("\n🔑 Multiple Transactions:");
    info!("   • One nonce account = one offline transaction");
    info!("   • Prepare N nonce accounts for N transactions");
    info!("   • Cost: ~0.0015 SOL per nonce account");

    info!("\n🔑 BLE Mesh Ready:");
    info!("   • Offline transactions can be fragmented");
    info!("   • Transmitted over BLE mesh");
    info!("   • Submitted by any online device");

    info!("\n🔑 Security:");
    info!("   • Nonce verification before submission");
    info!("   • Checksum validation");
    info!("   • Private keys stay on offline device");

    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  EXAMPLE COMPLETE                                     ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    // Cleanup
    // Bundle file persists for future use
    std::fs::remove_file(tx_file).ok();

    Ok(())
}
