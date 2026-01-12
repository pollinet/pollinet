//! Example: Creating Unsigned Transactions for Multi-Party Signing
//!
//! This example demonstrates creating unsigned transactions that can be signed later
//! or by multiple parties. Useful for:
//! - Multi-sig wallets
//! - Separate fee payer scenarios
//! - Offline transaction creation
//! - Transaction relay services
//!
//! Flow:
//! 1. Load sender keypair
//! 2. Find or create nonce account
//! 3. Create UNSIGNED transaction
//! 4. Return uncompressed transaction bytes
//! 5. (Later) Sign and submit when ready

mod wallet_utils;
use wallet_utils::{create_and_fund_wallet, get_rpc_url};

mod nonce_bundle_helper;
use nonce_bundle_helper::{get_next_nonce, load_bundle, save_bundle_after_use};

use base64;
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

    info!("=== PolliNet Unsigned Transaction Example ===\n");

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

    // 3. Load nonce from bundle
    info!("\n=== Loading Nonce from Bundle ===");
    let mut bundle = load_bundle()?;
    let (nonce_account, cached_nonce, nonce_index) = get_next_nonce(&mut bundle)?;

    info!("✅ Loaded nonce from bundle: {}", nonce_account);
    info!("   Nonce authority: {}", cached_nonce.authority);
    info!("   Blockhash: {}", cached_nonce.blockhash);

    // 4. Set transaction parameters
    info!("\n=== Transaction Parameters ===");
    let sender = sender_keypair.pubkey().to_string();
    let recipient = "RtsKQm3gAGL1Tayhs7ojWE9qytWqVh4G7eJTaNJs7vX".to_string();
    let fee_payer = sender_keypair.pubkey().to_string(); // Sender pays fees (can be different)
    let amount = 1_000_000; // 0.001 SOL

    info!("Sender: {}", sender);
    info!("Recipient: {}", recipient);
    info!("Fee payer: {}", fee_payer);
    info!(
        "Amount: {} lamports ({} SOL)",
        amount,
        amount as f64 / 1_000_000_000.0
    );
    info!("Nonce account: {}", nonce_account);

    // 5. Create UNSIGNED transaction
    info!("\n=== Creating Unsigned Transaction ===");
    info!("Creating unsigned transaction with nonce account...");
    info!("Transaction will NOT be signed or compressed");

    let unsigned_tx = sdk
        .create_unsigned_transaction(&sender, &recipient, &fee_payer, amount, Some(&nonce_account), None)
        .await?;

    // Mark nonce as used after creating transaction
    save_bundle_after_use(&mut bundle, nonce_index)?;

    info!("✅ Unsigned transaction created");
    info!("   Size: {} characters (base64 encoded)", unsigned_tx.len());
    info!("   Format: base64(bincode(Transaction))");
    info!("   Ready for signing");

    // 6. Display transaction details
    info!("\n=== Unsigned Transaction Details ===");
    info!("✅ Transaction is ready for signing!");
    info!("   Instructions: [1] Advance nonce, [2] Transfer");
    info!("   Required signers:");
    info!("     - Sender (as nonce authority): {}", sender);
    info!("     - Fee payer: {}", fee_payer);
    info!("   Blockhash: From nonce account (durable)");
    info!("\nBase64 transaction (first 60 chars):");
    info!("   {}...", &unsigned_tx[..60.min(unsigned_tx.len())]);

    // 7. Demonstrate signing the transaction
    info!("\n=== Demonstrating Signature Addition ===");

    // Simulate signing by sender
    info!("\n=== Signing Transaction ===");
    info!("Sender will sign the transaction...");

    // Decode and deserialize to get message data
    use solana_sdk::signature::Signer;
    let tx_bytes = base64::decode(&unsigned_tx)?;
    let tx_for_signing: solana_sdk::transaction::Transaction = bincode1::deserialize(&tx_bytes)?;
    let message_data = tx_for_signing.message_data();

    // Sign the message
    let sender_signature = sender_keypair.sign_message(&message_data);
    info!("✅ Sender created signature: {}", sender_signature);

    // Add signature using the SDK method
    info!("\n=== Adding Signature to Transaction ===");
    let partially_signed_tx =
        sdk.add_signature(&unsigned_tx, &sender_keypair.pubkey(), &sender_signature)?;

    info!("✅ Signature added successfully");
    info!(
        "   Updated transaction: {} characters (base64)",
        partially_signed_tx.len()
    );

    // Check if transaction is fully signed
    let final_tx_bytes = base64::decode(&partially_signed_tx)?;
    let final_tx: solana_sdk::transaction::Transaction = bincode1::deserialize(&final_tx_bytes)?;
    let valid_sigs = final_tx
        .signatures
        .iter()
        .filter(|sig| *sig != &solana_sdk::signature::Signature::default())
        .count();
    info!(
        "   Valid signatures: {}/{}",
        valid_sigs,
        final_tx.signatures.len()
    );

    if valid_sigs == final_tx.signatures.len() {
        info!("✅ Transaction is fully signed and ready for submission!");

        // Demonstrate submission
        info!("\n=== Submitting Fully Signed Transaction ===");
        info!("Submitting to Solana using submit_transaction...");

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

        let signature = sdk
            .submit_transaction(partially_signed_tx.as_str())
            .await?;
        info!("✅ Transaction submitted successfully!");
        info!("   Transaction signature: {}", signature);
        info!(
            "   View on Explorer: https://explorer.solana.com/tx/{}?cluster=devnet",
            signature
        );
    } else {
        info!(
            "⚠️  Transaction needs {} more signature(s)",
            final_tx.signatures.len() - valid_sigs
        );
        info!("   Would need fee payer to sign before submission");
    }

    info!("\n=== Next Steps for Multi-Party Signing ===");
    info!("1. Serialize to base64 for transmission");
    info!("2. Send to signers via BLE/internet");
    info!("3. Each party signs using add_signature()");
    info!("4. Verify all signatures collected");
    info!("5. Submit when fully signed");

    // 8. Summary
    info!("\n=== Complete Unsigned Transaction Summary ===");
    info!("✅ 1. Created new wallet and funded with 5 SOL via airdrop");
    info!("✅ 3. Set up nonce account");
    info!(
        "✅ 4. Created UNSIGNED transaction: {} chars (base64)",
        unsigned_tx.len()
    );
    info!("✅ 5. Demonstrated signature addition with add_signature()");
    info!(
        "✅ 6. Verified signature count: {}/{}",
        valid_sigs,
        final_tx.signatures.len()
    );
    if valid_sigs == final_tx.signatures.len() {
        info!("✅ 7. Submitted fully signed transaction to Solana");
    } else {
        info!("⚠️  7. Transaction ready for additional signatures");
    }

    info!("\n=== Implementation Notes ===");
    info!("• Transaction returns as base64 encoded string");
    info!("• add_signature() takes and returns base64");
    info!("• submit_transaction() takes base64 string or raw bytes");
    info!("• Sender is used as nonce authority");
    info!("• If sender = fee payer: Only one signature needed");
    info!("• If sender ≠ fee payer: Two signatures needed");
    info!("• add_signature() intelligently handles dual roles");
    info!("• Signatures can be added incrementally");

    info!("\n=== Use Cases ===");
    info!("• Multi-sig wallets: Collect signatures from multiple parties");
    info!("• Separate fee payer: Third party pays transaction fees");
    info!("• Offline signing: Create online, sign offline later");
    info!("• Transaction services: Create transactions for users to sign");
    info!("• Hardware wallets: Send to hardware wallet for signing");

    Ok(())
}
