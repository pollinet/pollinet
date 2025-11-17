//! Example: SPL Token Transfer with Durable Nonce
//!
//! This example demonstrates creating presigned SPL token transactions with nonce accounts.
//!
//! Flow:
//! 1. Load sender keypair from private key
//! 2. Check sender balance
//! 3. Find or create nonce account
//! 4. Create presigned SPL token transfer with nonce
//! 5. Compress and fragment for BLE transmission
//! 6. Reassemble, decompress, and submit to blockchain
//!
//! Prerequisites:
//! - Pre-funded wallet with SOL (for fees)
//! - SPL token accounts for sender and recipient
//! - Token balance in sender's token account

mod wallet_utils;
use wallet_utils::create_and_fund_wallet;

use chrono;
use pollinet::nonce;
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

    info!("=== PolliNet SPL Token Nonce Transaction Example ===\n");

    // 1. Initialize the SDK and RPC client
    let rpc_url = "https://api.devnet.solana.com";
    let sdk = PolliNetSDK::new_with_rpc(rpc_url).await?;
    let rpc_client =
        RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::finalized());
    info!("✅ SDK initialized with RPC client: {}", rpc_url);

    // 2. Create new wallet and request airdrop
    info!("\n=== Creating New Wallet ===");
    let sender_keypair = create_and_fund_wallet(&rpc_client, 5.0).await?;
    info!("✅ Sender loaded: {}", sender_keypair.pubkey());

    // 3. Set up nonce account (hardcoded for this example)
    info!("\n=== Setting Up Nonce Account ===");
    let nonce_account = "ADNKz5JadNZ3bCh9BxSE7UcmP5uG4uV4rJR9TWsZCSBK";
    info!("Using nonce account: {}", nonce_account);
    info!("   Nonce authority: {} (sender)", sender_keypair.pubkey());

    // 4. Set SPL token transfer parameters
    info!("\n=== SPL Token Transfer Parameters ===");

    // Wallet pubkeys and mint address (ATAs will be derived automatically)
    let sender_wallet = sender_keypair.pubkey().to_string();
    let recipient_wallet = "RtsKQm3gAGL1Tayhs7ojWE9qytWqVh4G7eJTaNJs7vX".to_string();
    let mint_address = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU".to_string(); // Your token mint
    let token_amount = 1_000_000; // Amount depends on token decimals (e.g., 1 USDC = 1,000,000)

    info!("Sender wallet: {}", sender_wallet);
    info!("Recipient wallet: {}", recipient_wallet);
    info!("Token mint: {}", mint_address);
    info!("Token amount: {} (smallest unit)", token_amount);
    info!("ATAs will be automatically derived from wallets + mint");

    // 5. Create the presigned SPL token transfer with nonce
    info!("\n=== Creating SPL Token Transfer ===");
    info!("Creating presigned SPL token transfer with nonce account...");
    info!("Associated Token Accounts will be derived automatically");

    let compressed_tx = sdk
        .create_spl_transaction(
            &sender_wallet,
            &sender_keypair,
            &recipient_wallet,
            &mint_address,
            token_amount,
            &nonce_account,
            &sender_keypair, // Sender is the nonce authority
        )
        .await?;

    info!("✅ SPL transaction created and signed");
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
            "   Fragment {}/{}: {} bytes (checksum: {})",
            i + 1,
            fragments.len(),
            fragment.data.len(),
            hex::encode(&fragment.checksum[..8])
        );
    }

    // 8. Display transaction details
    info!("\n=== Transaction Ready ===");
    info!("✅ SPL token transfer is ready for BLE transmission!");
    info!("✅ Transaction has a longer lifetime due to nonce account");
    info!("✅ Can be submitted to Solana at any time (until nonce advances)");

    // 9. Simulate receiving fragments
    info!("\n=== Simulating Fragment Reception ===");
    info!("In a real scenario, these fragments would be received over BLE mesh...");
    info!("Reassembling {} fragments...", fragments.len());

    let reassembled_tx = sdk.reassemble_fragments(&fragments)?;
    info!("✅ Fragments reassembled successfully");
    info!("   Reassembled size: {} bytes", reassembled_tx.len());

    if reassembled_tx == compressed_tx {
        info!("✅ Reassembly verification passed!");
    } else {
        return Err("Reassembly failed: data mismatch".into());
    }

    // 10. Optional: Wait to demonstrate nonce durability
    info!("\n=== Waiting Period (Optional) ===");
    info!("Waiting for 1 minute to demonstrate nonce transaction durability...");

    let total_minutes = 1;
    for remaining_minutes in (1..=total_minutes).rev() {
        let current_time = chrono::Local::now();
        info!(
            "⏳ {} minute(s) remaining | Current time: {}",
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

    // 11. Submit to Solana blockchain
    info!("\n=== Submitting SPL Transfer to Solana ===");
    info!("Decompressing and submitting transaction to blockchain...");

    let signature = sdk.submit_transaction_to_solana(&reassembled_tx).await?;
    info!("✅ SPL token transfer submitted successfully!");
    info!("   Transaction signature: {}", signature);
    info!(
        "   View on Explorer: https://explorer.solana.com/tx/{}?cluster=devnet",
        signature
    );

    // 12. Broadcast confirmation
    info!("\n=== Broadcasting Confirmation ===");
    sdk.broadcast_confirmation(&signature).await?;
    info!("✅ Confirmation broadcasted to network");

    // 13. Summary
    info!("\n=== Complete SPL Transfer Summary ===");
    info!("✅ 1. Loaded sender from private key");
    info!("✅ 2. Verified sender balance");
    info!("✅ 3. Set up nonce account: {}", nonce_account);
    info!("✅ 4. Created presigned SPL token transfer with durable nonce");
    info!(
        "✅ 5. Compressed transaction: {} bytes",
        compressed_tx.len()
    );
    info!(
        "✅ 6. Fragmented into {} BLE-ready fragments",
        fragments.len()
    );
    info!("✅ 7. Reassembled fragments with checksum verification");
    info!("✅ 8. Decompressed and submitted to Solana");
    info!("✅ 9. Broadcasted confirmation: {}", signature);

    info!("\n=== SPL Token Transfer Notes ===");
    info!("• Transaction type: SPL Token Transfer (not native SOL)");
    info!("• Token accounts used: sender → recipient token accounts");
    info!("• Nonce account: {}", nonce_account);
    info!("• Sender signs as both token owner and nonce authority");
    info!("• Instructions: [1] Advance nonce, [2] SPL token transfer");
    info!("• Transaction remains valid until nonce is advanced");

    Ok(())
}
