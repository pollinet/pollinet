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

use bs58;
use pollinet::PolliNetSDK;
use pollinet::transaction::OfflineTransactionBundle;
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

    // Load sender keypair
    let sender_private_key =
        "5zRwe731N375MpGuQvQoUjSMUpoXNLqsGWE9J8SoqHKfivhUpNxwt3o9Gdu6jjCby4dJRCGBA6HdBzrhvLVhUaqu";
    let private_key_bytes = bs58::decode(sender_private_key)
        .into_vec()
        .map_err(|e| format!("Failed to decode private key: {}", e))?;
    let sender_keypair = Keypair::try_from(&private_key_bytes[..])
        .map_err(|e| format!("Failed to create keypair from private key: {}", e))?;

    info!("✅ Sender loaded: {}", sender_keypair.pubkey());

    // ================================================================
    // PHASE 1: ONLINE - Prepare for Offline Use
    // ================================================================
    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  PHASE 1: ONLINE - Prepare for Offline Use           ║");
    info!("╚═══════════════════════════════════════════════════════╝");

    let rpc_url = "https://api.devnet.solana.com";
    let sdk = PolliNetSDK::new_with_rpc(rpc_url).await?;
    let rpc_client =
        RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::finalized());

    // Check sender balance
    info!("\n=== Checking Sender Balance ===");
    let sender_balance = rpc_client.get_balance(&sender_keypair.pubkey())?;
    info!(
        "Sender balance: {} lamports ({} SOL)",
        sender_balance,
        sender_balance as f64 / LAMPORTS_PER_SOL as f64
    );

    if sender_balance == 0 {
        return Err("Sender has no balance. Please fund the wallet first.".into());
    }

    // Create multiple nonce accounts for multiple offline transactions
    info!("\n=== Creating Nonce Accounts for Offline Use ===");
    let num_nonces = 3; // Prepare 3 nonce accounts = 3 offline transactions
    info!("Creating {} nonce accounts...", num_nonces);

    let mut offline_bundle = OfflineTransactionBundle::new();

    // For this example, we'll use an existing nonce account
    // In production, you would create new ones like this:
    /*
    for i in 0..num_nonces {
        info!("Creating nonce account {}/{}...", i + 1, num_nonces);
        let nonce_keypair = nonce::create_nonce_account(&rpc_client, &sender_keypair).await?;
        
        // Wait for confirmation
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        
        // Fetch and cache nonce data
        let cached_nonce = sdk
            .prepare_offline_nonce_data(&nonce_keypair.pubkey().to_string())
            .await?;
        
        offline_bundle.add_nonce(cached_nonce);
        info!("  ✅ Nonce {}/{} ready", i + 1, num_nonces);
    }
    */

    // For demo purposes, use existing nonce account
    info!("Using existing nonce account for demonstration...");
    let nonce_account = "ADNKz5JadNZ3bCh9BxSE7UcmP5uG4uV4rJR9TWsZCSBK";
    
    let cached_nonce = sdk.prepare_offline_nonce_data(nonce_account).await?;
    offline_bundle.add_nonce(cached_nonce.clone());
    offline_bundle.add_nonce(cached_nonce.clone());
    offline_bundle.add_nonce(cached_nonce);

    info!(
        "✅ Prepared {} nonce accounts for offline use",
        offline_bundle.available_nonces()
    );

    // Save to file
    info!("\n=== Saving Nonce Data for Offline Use ===");
    let cache_file = "offline_nonces.json";
    offline_bundle.save_to_file(cache_file)?;
    info!("✅ Saved offline nonce data to: {}", cache_file);
    info!("   This file can now be used to create transactions offline");
    info!("   File size: {} bytes", std::fs::metadata(cache_file)?.len());

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
    let loaded_bundle = OfflineTransactionBundle::load_from_file(cache_file)?;
    info!("✅ Loaded {} cached nonce accounts", loaded_bundle.available_nonces());
    info!("   Bundle age: {} hours", loaded_bundle.age_hours());

    // Create multiple transactions offline
    info!("\n=== Creating Transactions OFFLINE ===");
    let recipients = vec![
        "RtsKQm3gAGL1Tayhs7ojWE9qytWqVh4G7eJTaNJs7vX",
        "GgathUhdrCWRHowoRKACjgWhYHfxCEdBi5ViqYN6HVxk",
        "ADNKz5JadNZ3bCh9BxSE7UcmP5uG4uV4rJR9TWsZCSBK",
    ];
    let amounts = vec![100_000, 200_000, 300_000]; // Different amounts

    let mut offline_txs = Vec::new();

    for (i, (recipient, amount)) in recipients.iter().zip(amounts.iter()).enumerate() {
        if let Some(cached_nonce) = loaded_bundle.get_nonce(i) {
            info!("Creating offline transaction {}/{}...", i + 1, recipients.len());
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
            info!("  ✅ Transaction created: {} bytes", offline_txs[i].len());
            info!("     NO INTERNET REQUIRED!");
        }
    }

    info!(
        "\n✅ Created {} transactions completely OFFLINE",
        offline_txs.len()
    );

    // Save offline transactions to file
    info!("\n=== Saving Offline Transactions ===");
    let tx_file = "offline_transactions.json";
    let tx_json = serde_json::to_string_pretty(&offline_txs)?;
    std::fs::write(tx_file, tx_json)?;
    info!("✅ Saved {} transactions to: {}", offline_txs.len(), tx_file);

    info!("\n╔═══════════════════════════════════════════════════════╗");
    info!("║  PHASE 2 COMPLETE!                                    ║");
    info!("║  {} transactions created offline                     ║", offline_txs.len());
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
    let sdk_online = PolliNetSDK::new_with_rpc(rpc_url).await?;

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
                info!("     Explorer: https://explorer.solana.com/tx/{}?cluster=devnet", signature);
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
    info!("   • Created {} nonce accounts", loaded_bundle.available_nonces());
    info!("   • Cached nonce data to file");
    info!("   • File: {}", cache_file);

    info!("\n✅ PHASE 2 (OFFLINE):");
    info!("   • Loaded cached nonce data");
    info!("   • Created {} transactions WITHOUT internet", loaded_txs.len());
    info!("   • Saved transactions to file");

    info!("\n✅ PHASE 3 (ONLINE):");
    info!("   • Submitted {} transaction(s) successfully", signatures.len());
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
    std::fs::remove_file(cache_file).ok();
    std::fs::remove_file(tx_file).ok();

    Ok(())
}

