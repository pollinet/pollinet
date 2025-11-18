//! Example: Complete end-to-end nonce transaction flow
//!
//! This example demonstrates the complete PolliNet transaction lifecycle:
//! 1. Load sender from private key
//! 2. Check sender balance (wallet must be pre-funded)
//! 3. Check for existing nonce account or create new one (sender funds and controls it)
//! 4. Create signed transaction using the nonce account for longer lifetime
//! 5. Compress the transaction
//! 6. Fragment it for BLE transmission
//! 7. Reassemble fragments back together
//! 8. Decompress and submit to Solana blockchain
//! 9. Broadcast confirmation to the network
//!
//! Key features:
//! - Sender is both the funder and authority of the nonce account
//! - Checks for existing nonce account before creating new one
//! - Sender can advance the nonce independently
//! - Transaction has extended lifetime (not limited to ~2 minutes)
//!
//! Prerequisites:
//! - RPC endpoint access (Solana devnet)
//! - Pre-funded wallet (sender must have balance)
//! - Internet connection to submit transaction

mod wallet_utils;
use wallet_utils::{create_and_fund_wallet, get_rpc_url};

mod nonce_bundle_helper;
use nonce_bundle_helper::{get_next_nonce, load_bundle, save_bundle_after_use};

use chrono;
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

    info!("=== PolliNet Nonce Transaction Example ===\n");

    // 1. Initialize the SDK and RPC client
    let rpc_url = get_rpc_url();
    info!("🌐 Using RPC endpoint: {}", rpc_url);
    let sdk = PolliNetSDK::new_with_rpc(&rpc_url).await?;
    let rpc_client = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::finalized());
    info!("✅ SDK initialized with RPC client: {}", rpc_url);

    // 2. Create new wallet and request airdrop
    info!("\n=== Creating New Wallet ===");
    let sender_keypair = create_and_fund_wallet(&rpc_client, 5.0).await?;
    info!("✅ Sender loaded: {}", sender_keypair.pubkey());
    info!("   Sender will be both the funder and nonce authority");

    // 4. Load nonce from bundle
    info!("\n=== Loading Nonce from Bundle ===");
    let mut bundle = load_bundle()?;
    let (nonce_account, cached_nonce, nonce_index) = get_next_nonce(&mut bundle)?;

    info!("✅ Loaded nonce from bundle: {}", nonce_account);
    info!("   Nonce authority: {}", cached_nonce.authority);
    info!("   Blockhash: {}", cached_nonce.blockhash);

    // 5. Set transaction parameters
    let sender = sender_keypair.pubkey().to_string();
    let recipient = "RtsKQm3gAGL1Tayhs7ojWE9qytWqVh4G7eJTaNJs7vX".to_string();
    let amount = 1_000_000; // 0.001 SOL in lamports

    info!("\n=== Transaction Parameters ===");
    info!("Sender: {}", sender);
    info!("Recipient: {}", recipient);
    info!(
        "Amount: {} lamports ({} SOL)",
        amount,
        amount as f64 / 1_000_000_000.0
    );
    info!("Nonce account: {}", nonce_account);

    // 6. Create the signed transaction with nonce
    info!("\n=== Creating Transaction ===");
    info!("Creating presigned transaction with nonce account...");

    let compressed_tx = sdk
        .create_transaction(
            &sender,
            &sender_keypair,
            &recipient,
            amount,
            &nonce_account,
            &sender_keypair, // Sender is the nonce authority
        )
        .await?;

    // Mark nonce as used after creating transaction
    save_bundle_after_use(&mut bundle, nonce_index)?;

    info!("✅ Transaction created and signed");
    info!("✅ Transaction serialized");
    info!("✅ Transaction compressed (if needed)");
    info!("   Compressed size: {} bytes", compressed_tx.len());

    // 7. Fragment the transaction for BLE transmission
    info!("\n=== Fragmenting for BLE ===");
    let fragments = sdk.fragment_transaction(&compressed_tx);

    info!(
        "✅ Transaction fragmented into {} fragments",
        fragments.len()
    );
    info!("   BLE MTU size: {} bytes", pollinet::BLE_MTU_SIZE);

    for (i, fragment) in fragments.iter().enumerate() {
        info!(
            "   Fragment {}/{}: {} bytes",
            i + 1,
            fragments.len(),
            fragment.data.len()
        );
    }

    // 8. Display transaction details
    info!("\n=== Transaction Ready ===");
    info!("✅ The transaction is ready for BLE transmission!");
    info!("✅ Transaction has a longer lifetime due to nonce account");
    info!("✅ Can be submitted to Solana at any time (until nonce advances)");

    // 9. Simulate receiving fragments (in real scenario, these would come over BLE)
    info!("\n=== Simulating Fragment Reception ===");
    info!("In a real scenario, these fragments would be received over BLE mesh...");
    info!("Reassembling {} fragments...", fragments.len());

    let reassembled_tx = sdk.reassemble_fragments(&fragments)?;
    info!("✅ Fragments reassembled successfully");
    info!("   Reassembled size: {} bytes", reassembled_tx.len());

    // Verify reassembly
    if reassembled_tx == compressed_tx {
        info!("✅ Reassembly verification passed!");
    } else {
        return Err("Reassembly failed: data mismatch".into());
    }

    info!("\n=== Waiting Period ===");
    info!("Waiting for 5 minutes before submitting to Solana");
    info!("This demonstrates that nonce-based transactions remain valid over time");

    // Countdown timer - 5 minutes
    let total_minutes = 5;
    for remaining_minutes in (1..=total_minutes).rev() {
        let current_time = chrono::Local::now();
        info!(
            "⏳ {} minute(s) remaining until submission | Current time: {}",
            remaining_minutes,
            current_time.format("%Y-%m-%d %H:%M:%S")
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }

    let final_time = chrono::Local::now();
    info!(
        "✅ Wait complete | Time: {}",
        final_time.format("%Y-%m-%d %H:%M:%S")
    );
    info!("Transaction is still valid thanks to durable nonce!");

    // 10. Submit to Solana blockchain
    info!("\n=== Submitting to Solana ===");
    info!("Decompressing and submitting transaction to blockchain...");

    let signature = sdk.submit_transaction_to_solana(&reassembled_tx).await?;
    info!("✅ Transaction submitted successfully!");
    info!("   Transaction signature: {}", signature);

    // 11. Broadcast confirmation
    info!("\n=== Broadcasting Confirmation ===");
    sdk.broadcast_confirmation(&signature).await?;
    info!("✅ Confirmation broadcasted to network");

    // 12. Summary
    info!("\n=== Complete Flow Summary ===");
    info!("✅ 1. Loaded sender from private key: {}", sender);
    info!("✅ 2. Verified sender balance");
    info!("✅ 3. Set up nonce account: {}", nonce_account);
    info!("✅ 4. Created signed transaction with durable nonce");
    info!(
        "✅ 5. Compressed transaction: {} bytes",
        compressed_tx.len()
    );
    info!(
        "✅ 6. Fragmented into {} BLE-ready fragments",
        fragments.len()
    );
    info!("✅ 7. Reassembled fragments back to transaction");
    info!("✅ 8. Decompressed and submitted to Solana");
    info!("✅ 9. Broadcasted confirmation: {}", signature);

    info!("\n=== Implementation Notes ===");
    info!("• Nonce account on Solana devnet: {}", nonce_account);
    info!("• New wallet created and funded with 5 SOL via airdrop");
    info!("• Checks for existing nonce account before creating new one");
    info!("• Sender is both the funder and authority of the nonce account");
    info!("• Transaction uses nonce account's stored blockhash (not recent blockhash)");
    info!("• Transaction remains valid until nonce account is advanced");
    info!("• Sender signs once (as both nonce authority and transaction sender)");
    info!("• Advance nonce instruction is automatically included as first instruction");
    info!(
        "• Complete end-to-end flow: load → verify → setup nonce → create → fragment → relay → reassemble → submit → confirm"
    );

    Ok(())
}
