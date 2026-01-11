//! Example: Creating Unsigned SPL Token Transactions for Multi-Party Signing
//!
//! This example demonstrates creating unsigned SPL token transfer transactions that can be signed later
//! or by multiple parties. Useful for:
//! - Multi-sig SPL token wallets
//! - Separate fee payer scenarios for token transfers
//! - Offline token transaction creation
//! - Token payment services
//!
//! Flow:
//! 1. Load sender keypair
//! 2. Find or create nonce account
//! 3. Create UNSIGNED SPL token transfer
//! 4. Add sender signature
//! 5. Submit to Solana (if fully signed)

use base64;
use bs58;
use chrono;
use pollinet::PolliNetSDK;
use pollinet::nonce;
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

    info!("=== PolliNet Unsigned SPL Token Transaction Example ===\n");

    // 1. Initialize the SDK and RPC client
    let rpc_url = "https://solana-devnet.g.alchemy.com/v2/XuGpQPCCl-F1SSI-NYtsr0mSxQ8P8ts6";
    let sdk = PolliNetSDK::new_with_rpc(rpc_url).await?;
    let rpc_client =
        RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::finalized());
    info!("✅ SDK initialized with RPC client: {}", rpc_url);

    // 2. Load sender keypair from private key
    info!("\n=== Loading Sender Keypair ===");
    let sender_private_key =
        "5zRwe731N375MpGuQvQoUjSMUpoXNLqsGWE9J8SoqHKfivhUpNxwt3o9Gdu6jjCby4dJRCGBA6HdBzrhvLVhUaqu";

    let private_key_bytes = bs58::decode(sender_private_key)
        .into_vec()
        .map_err(|e| format!("Failed to decode private key: {}", e))?;

    let sender_keypair = Keypair::try_from(&private_key_bytes[..])
        .map_err(|e| format!("Failed to create keypair from private key: {}", e))?;

    info!("✅ Sender loaded: {}", sender_keypair.pubkey());

    // 3. Check sender balance
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

    // 4. Set up nonce account
    info!("\n=== Setting Up Nonce Account ===");
    let nonce_account = "ADNKz5JadNZ3bCh9BxSE7UcmP5uG4uV4rJR9TWsZCSBK";
    info!("Using nonce account: {}", nonce_account);
    info!("   Nonce authority: {} (sender)", sender_keypair.pubkey());

    // 5. Set SPL token transfer parameters
    info!("\n=== SPL Token Transfer Parameters ===");
    let sender_wallet = sender_keypair.pubkey().to_string();
    let recipient_wallet = "RtsKQm3gAGL1Tayhs7ojWE9qytWqVh4G7eJTaNJs7vX".to_string();
    let fee_payer = sender_keypair.pubkey().to_string(); // Sender pays fees (can be different)
    let mint_address = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU".to_string(); // Token mint
    let token_amount = 500_000; // Amount in token's smallest unit (e.g., 0.5 USDC = 500,000)

    info!("Sender wallet: {}", sender_wallet);
    info!("Recipient wallet: {}", recipient_wallet);
    info!("Fee payer: {}", fee_payer);
    info!("Token mint: {}", mint_address);
    info!("Token amount: {} (smallest unit)", token_amount);
    info!("ATAs will be automatically derived");

    // 6. Create UNSIGNED SPL token transfer
    info!("\n=== Creating Unsigned SPL Token Transfer ===");
    info!("Creating unsigned SPL token transfer with nonce account...");
    info!("Transaction will NOT be signed or compressed");
    info!("ATAs will be derived from wallets + mint");

    let unsigned_tx = sdk
        .create_unsigned_spl_transaction(
            &sender_wallet,
            &recipient_wallet,
            &fee_payer,
            &mint_address,
            token_amount,
            Some(&nonce_account),
            None, // nonce_data - will fetch from nonce_account
        )
        .await?;

    info!("✅ Unsigned SPL transaction created");
    info!("   Size: {} characters (base64 encoded)", unsigned_tx.len());
    info!("   Format: base64(bincode(Transaction))");
    info!("   Ready for signing");

    // 7. Display transaction details
    info!("\n=== Unsigned SPL Transaction Details ===");
    info!("✅ Transaction is ready for signing!");
    info!("   Instructions: [1] Advance nonce, [2] SPL Token Transfer");
    info!("   Required signers:");
    info!("     - Sender/Token owner (as nonce authority): {}", sender_wallet);
    info!("     - Fee payer: {}", fee_payer);
    info!("   Blockhash: From nonce account (durable)");
    info!("\nBase64 transaction (first 60 chars):");
    info!("   {}...", &unsigned_tx[..60.min(unsigned_tx.len())]);

    // 8. Demonstrate signing the transaction
    info!("\n=== Demonstrating Signature Addition ===");

    // Simulate signing by sender/token owner
    info!("\n=== Signing SPL Transaction ===");
    info!("Sender (token owner) will sign the transaction...");

    // Decode and deserialize to get message data
    let tx_bytes = base64::decode(&unsigned_tx)?;
    let tx_for_signing: solana_sdk::transaction::Transaction = bincode1::deserialize(&tx_bytes)?;
    let message_data = tx_for_signing.message_data();

    // Sign the message
    let sender_signature = sender_keypair.sign_message(&message_data);
    info!("✅ Sender created signature: {}", sender_signature);

    // Add signature using the SDK method
    info!("\n=== Adding Signature to SPL Transaction ===");
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
        info!("✅ SPL transaction is fully signed and ready for submission!");

        // Demonstrate submission with countdown
        info!("\n=== Waiting Period (Optional) ===");
        info!("Waiting for 2 minutes to demonstrate nonce transaction durability...");

        let total_minutes = 1;
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
        info!("✅ Wait complete | Time: {}", final_time.format("%Y-%m-%d %H:%M:%S"));

        // Submit the SPL transaction
        info!("\n=== Submitting Fully Signed SPL Transaction ===");
        info!("Submitting to Solana using submit_transaction...");

        let signature = sdk
            .submit_transaction(partially_signed_tx.as_str())
            .await?;
        info!("✅ SPL token transfer submitted successfully!");
        info!("   Transaction signature: {}", signature);
        info!(
            "   View on Explorer: https://explorer.solana.com/tx/{}?cluster=devnet",
            signature
        );
    } else {
        info!(
            "⚠️  SPL transaction needs {} more signature(s)",
            final_tx.signatures.len() - valid_sigs
        );
        info!("   Would need fee payer to sign before submission");
    }

    info!("\n=== Next Steps for Multi-Party SPL Signing ===");
    info!("1. Create unsigned SPL transaction (already base64)");
    info!("2. Send to token owner via BLE/internet");
    info!("3. Owner signs using add_signature()");
    info!("4. If fee payer differs, send to fee payer");
    info!("5. Fee payer signs using add_signature()");
    info!("6. Submit when fully signed using submit_transaction()");

    // 9. Summary
    info!("\n=== Complete Unsigned SPL Transaction Summary ===");
    info!("✅ 1. Loaded sender from private key");
    info!("✅ 2. Verified sender balance");
    info!("✅ 3. Set up nonce account");
    info!(
        "✅ 4. Created UNSIGNED SPL transaction: {} chars (base64)",
        unsigned_tx.len()
    );
    info!("✅ 5. ATAs automatically derived from wallets + mint");
    info!("✅ 6. Demonstrated signature addition with add_signature()");
    info!(
        "✅ 7. Verified signature count: {}/{}",
        valid_sigs,
        final_tx.signatures.len()
    );
    if valid_sigs == final_tx.signatures.len() {
        info!("✅ 8. Submitted fully signed SPL transaction to Solana");
    } else {
        info!("⚠️  8. Transaction ready for additional signatures");
    }

    info!("\n=== Implementation Notes ===");
    info!("• Transaction type: SPL Token Transfer (unsigned)");
    info!("• Transaction returns as base64 encoded string");
    info!("• add_signature() takes and returns base64");
    info!("• submit_transaction() takes base64 string or raw bytes");
    info!("• ATAs derived automatically from wallets + mint");
    info!("• Sender is used as nonce authority AND token owner");
    info!("• If sender = fee payer: Only one signature needed");
    info!("• If sender ≠ fee payer: Two signatures needed");
    info!("• add_signature() intelligently handles dual roles");
    info!("• Instructions: [1] Advance nonce, [2] SPL token transfer");

    info!("\n=== Use Cases ===");
    info!("• Multi-sig token wallets: Collect signatures from multiple parties");
    info!("• Token payment services: Third party pays transaction fees");
    info!("• Offline token transfers: Create online, sign offline later");
    info!("• DeFi applications: Create transactions for users to approve");
    info!("• Hardware wallet integration: Send to hardware wallet for signing");

    Ok(())
}

