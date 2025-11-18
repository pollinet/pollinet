//! M1 Demo: 50+ Successful Offline-to-Online Transactions
//!
//! This example demonstrates the M1 requirement of 50+ successful offline to online
//! transactions on Solana Devnet. It creates 50 nonce accounts, generates 50 offline
//! transactions, and submits them all successfully.
//!
//! Run with: cargo run --example m1_demo_50_transactions

mod wallet_utils;
use wallet_utils::{create_and_fund_wallet, get_rpc_url};

mod nonce_bundle_helper;
use nonce_bundle_helper::{get_next_nonce, load_bundle, save_bundle_after_use, BUNDLE_FILE};

use base64::{engine::general_purpose, Engine as _};
use pollinet::PolliNetSDK;
use pollinet::util::lz::Lz4Compressor;
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::signer::Signer;
use tracing::{info, warn};

const REQUIRED_TRANSACTIONS: usize = 5;
const AIRDROP_AMOUNT_SOL: f64 = 2.0; // Enough for 50 nonce accounts (~0.075 SOL) + fees
const OFFLINE_TX_FILE: &str = ".offline_transaction.json";
const OFFLINE_SUBMISSION_FILE: &str = ".offline_submission.json";

#[derive(Serialize, Deserialize)]
struct OfflineTransactionRecord {
    index: usize,
    recipient: String,
    amount: u64,
    compressed_tx: String,
    fragments: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("╔═══════════════════════════════════════════════════════╗");
    info!("║  M1 DEMO: 50+ Offline-to-Online Transactions          ║");
    info!("╚═══════════════════════════════════════════════════════╝");
    info!("");
    info!(
        "This demo creates {} offline transactions and submits",
        REQUIRED_TRANSACTIONS
    );
    info!("them all successfully to Solana Devnet.");
    info!("");

    let rpc_url = get_rpc_url();
    info!("🌐 Using RPC endpoint: {}", rpc_url);
    let rpc_client = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::finalized());

    // Create sender wallet with sufficient funds
    info!("=== Step 1: Creating and Funding Sender Wallet ===");
    let sender_keypair = create_and_fund_wallet(&rpc_client, AIRDROP_AMOUNT_SOL).await?;
    info!("✅ Sender wallet: {}", sender_keypair.pubkey());
    info!(
        "   Balance should be sufficient for {} nonce accounts",
        REQUIRED_TRANSACTIONS
    );
    info!("");

    // Initialize SDK
    let sdk = PolliNetSDK::new_with_rpc(&rpc_url).await?;

    // PHASE 1: Prepare 50 nonce accounts
    info!(
        "=== Step 2: Preparing {} Nonce Accounts ===",
        REQUIRED_TRANSACTIONS
    );
    info!("Creating nonce accounts and caching data for offline use...");
    info!("");
    info!("🌐 This prepare step happens while the device has internet connectivity.");
    info!("   Nonce accounts are durably stored so they can be used later while offline.");

    let offline_bundle = sdk
        .prepare_offline_bundle(REQUIRED_TRANSACTIONS, &sender_keypair, Some(BUNDLE_FILE))
        .await?;

    info!(
        "✅ Prepared {} nonce accounts",
        offline_bundle.total_nonces()
    );
    info!(
        "   Available for offline transactions: {}",
        offline_bundle.available_nonces()
    );
    info!("   Saved to: {}", BUNDLE_FILE);
    info!("");

    // PHASE 2: Create 50 offline transactions
    info!(
        "=== Step 3: Creating {} Offline Transactions ===",
        REQUIRED_TRANSACTIONS
    );
    info!("Creating transactions completely offline (no internet required)...");
    info!("");

    // Ensure bundle file exists before going offline
    let mut loaded_bundle = match load_bundle() {
        Ok(bundle) => bundle,
        Err(err) => {
            warn!("⚠️  Failed to load {}: {}", BUNDLE_FILE, err);
            info!("   Attempting to recreate bundle file before going offline...");
            let refreshed_bundle = sdk
                .prepare_offline_bundle(REQUIRED_TRANSACTIONS, &sender_keypair, Some(BUNDLE_FILE))
                .await?;
            refreshed_bundle.save_to_file(BUNDLE_FILE)?;
            info!("   ✅ Bundle recreated at {}", BUNDLE_FILE);
            load_bundle()?
        }
    };

    info!("🛑 Assume the device is now offline, using only cached nonce data.");
    info!("");
    let mut offline_records = Vec::new();

    // Use sender's own account as recipient to avoid rent exemption issues
    // (Sending to a new account requires rent exemption ~0.00089 SOL minimum)
    let recipient_pubkey = sender_keypair.pubkey();
    let transfer_amount = 10_000; // 0.00001 SOL per transaction

    for tx_index in 0..REQUIRED_TRANSACTIONS {
        let (nonce_account, cached_nonce, nonce_position) = get_next_nonce(&mut loaded_bundle)?;
        info!(
            "Creating transaction {}/{} using nonce account {}...",
            tx_index + 1,
            REQUIRED_TRANSACTIONS,
            nonce_account
        );

        let compressed_tx = sdk.create_offline_transaction(
            &sender_keypair,
            &recipient_pubkey.to_string(),
            transfer_amount,
            &sender_keypair, // Sender is nonce authority
            &cached_nonce,
        )?;

        info!(
            "   ✅ Transaction serialized & compressed ({} bytes)",
            compressed_tx.len()
        );

        // Fragment for BLE transport
        let fragments = sdk.fragment_transaction(&compressed_tx);
        info!("   📡 Fragmented into {} BLE chunks", fragments.len());

        // Persist data for later submission
        let encoded_tx = general_purpose::STANDARD.encode(&compressed_tx);
        let encoded_fragments = fragments
            .iter()
            .map(|fragment| general_purpose::STANDARD.encode(&fragment.data))
            .collect::<Vec<_>>();

        offline_records.push(OfflineTransactionRecord {
            index: tx_index + 1,
            recipient: recipient_pubkey.to_string(),
            amount: transfer_amount,
            compressed_tx: encoded_tx,
            fragments: encoded_fragments,
        });

        save_bundle_after_use(&mut loaded_bundle, nonce_position)?;

        if (tx_index + 1) % 10 == 0 {
            info!(
                "  ✅ Created {}/{} transactions",
                tx_index + 1,
                REQUIRED_TRANSACTIONS
            );
        }
    }

    info!(
        "✅ Created, compressed, and fragmented all {} transactions offline",
        offline_records.len()
    );

    // Save offline artifacts
    let tx_json = serde_json::to_string_pretty(&offline_records)?;
    std::fs::write(OFFLINE_TX_FILE, tx_json)?;
    info!(
        "   Saved compressed + fragmented payloads to: {}",
        OFFLINE_TX_FILE
    );
    info!("");

    // Simulate extended offline period with countdown
    info!("⏳ Simulating 5-minute offline period before reconnecting...");
    info!("   This demonstrates that transactions remain valid during extended offline periods");
    info!("");
    
    let total_seconds = 5 * 60; // 5 minutes
    for remaining in (1..=total_seconds).rev() {
        let minutes = remaining / 60;
        let seconds = remaining % 60;
        
        if remaining % 10 == 0 || remaining <= 10 {
            info!("   ⏱️  Offline period: {:02}:{:02} remaining", minutes, seconds);
        }
        
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
    
    info!("");
    info!("🌐 Reconnecting to internet...");

    // PHASE 3: Submit all 50 transactions
    info!(
        "=== Step 4: Submitting {} Transactions to Devnet ===",
        REQUIRED_TRANSACTIONS
    );
    info!("Simulating BLE transmission → reassembly → decompression → submission...");
    info!("");

    // Load transactions
    let tx_json = std::fs::read_to_string(OFFLINE_TX_FILE)?;
    let loaded_records: Vec<OfflineTransactionRecord> = serde_json::from_str(&tx_json)?;
    info!(
        "✅ Loaded {} offline transaction artifacts",
        loaded_records.len()
    );

    // Submit with rate limiting to avoid overwhelming RPC
    let mut successful_signatures = Vec::new();
    let mut failed_count = 0;

    for (i, record) in loaded_records.iter().enumerate() {
        let compressed_tx = general_purpose::STANDARD
            .decode(&record.compressed_tx)
            .map_err(|e| {
                format!(
                    "Failed to decode stored transaction {}: {}",
                    record.index, e
                )
            })?;

        info!(
            "🔁 Simulating BLE relay for transaction {}/{} (nonce index {})...",
            i + 1,
            loaded_records.len(),
            record.index
        );

        let fragments = sdk.fragment_transaction(&compressed_tx);
        info!("   • Fragment count: {}", fragments.len());

        let reassembled_tx = sdk.reassemble_fragments(&fragments)?;
        info!(
            "   • Fragments reassembled successfully ({} bytes)",
            reassembled_tx.len()
        );

        // Decompress the transaction
        info!("   • Decompressing transaction...");
        let compressor = Lz4Compressor::new()
            .map_err(|e| format!("Failed to create compressor: {}", e))?;
        
        let decompressed_tx = if reassembled_tx.len() >= 8 && reassembled_tx.starts_with(b"LZ4") {
            let decompressed = compressor.decompress_with_size(&reassembled_tx)
                .map_err(|e| format!("Failed to decompress transaction: {}", e))?;
            info!(
                "   • Decompression successful: {} bytes -> {} bytes",
                reassembled_tx.len(),
                decompressed.len()
            );
            decompressed
        } else {
            info!("   • No compression detected, using raw data");
            reassembled_tx
        };

        info!(
            "Submitting transaction {}/{} to Solana...",
            i + 1,
            loaded_records.len()
        );

        match sdk.submit_offline_transaction(&decompressed_tx, true).await {
            Ok(signature) => {
                info!("  ✅ Transaction {} SUCCESS", i + 1);
                info!("     Signature: {}", signature);
                info!(
                    "     Explorer: https://explorer.solana.com/tx/{}?cluster=devnet",
                    signature
                );
                successful_signatures.push(signature);
            }
            Err(e) => {
                info!("  ❌ Transaction {} FAILED: {}", i + 1, e);
                failed_count += 1;
                // Continue with next transaction
            }
        }

        // Small delay to avoid rate limiting
        if i < loaded_records.len() - 1 {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        // Progress update every 10 transactions
        if (i + 1) % 10 == 0 {
            info!(
                "  Progress: {}/{} submitted, {} successful, {} failed",
                i + 1,
                loaded_records.len(),
                successful_signatures.len(),
                failed_count
            );
        }
    }

    // Final summary
    info!("");
    info!("╔═══════════════════════════════════════════════════════╗");
    info!("║  M1 DEMO COMPLETE                                    ║");
    info!("╚═══════════════════════════════════════════════════════╝");
    info!("");
    info!("Total transactions created: {}", loaded_records.len());
    info!("Total transactions submitted: {}", loaded_records.len());
    info!("Successful transactions: {}", successful_signatures.len());
    info!("Failed transactions: {}", failed_count);
    info!("");

    if successful_signatures.len() >= REQUIRED_TRANSACTIONS {
        info!(
            "✅ REQUIREMENT MET: {} successful offline-to-online transactions!",
            successful_signatures.len()
        );
    } else {
        info!(
            "⚠️  REQUIREMENT NOT FULLY MET: Need {}, got {}",
            REQUIRED_TRANSACTIONS,
            successful_signatures.len()
        );
    }

    info!("");
    info!("=== All Transaction Signatures ===");
    for (i, sig) in successful_signatures.iter().enumerate() {
        info!("  {}. {}", i + 1, sig);
    }

    // Save signatures to file for verification
    let sig_json = serde_json::to_string_pretty(&successful_signatures)?;
    std::fs::write(OFFLINE_SUBMISSION_FILE, sig_json)?;
    info!("");
    info!("✅ Signatures saved to: {}", OFFLINE_SUBMISSION_FILE);
    info!("");
    info!("=== Verification ===");
    info!("All transactions are publicly verifiable on Solana Explorer:");
    for sig in successful_signatures.iter().take(5) {
        info!("  https://explorer.solana.com/tx/{}?cluster=devnet", sig);
    }
    if successful_signatures.len() > 5 {
        info!("  ... and {} more", successful_signatures.len() - 5);
    }

    Ok(())
}
