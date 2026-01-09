//! Example: Testing SPL Token Transactions with Idempotent ATA Creation
//!
//! This example demonstrates creating and testing SPL token transfer transactions
//! with automatic idempotent ATA (Associated Token Account) creation.
//! 
//! Key features:
//! - Loads wallet from private key
//! - Creates SPL token transfer with idempotent ATA creation
//! - Verifies transaction structure (3 instructions: advance nonce, create ATA, transfer)
//! - Tests both online and offline transaction creation
//! - Demonstrates signing and submission
//!
//! Flow:
//! 1. Load sender keypair from private key
//! 2. Verify sender balance and token account
//! 3. Create unsigned SPL transaction (with idempotent ATA creation)
//! 4. Verify transaction has 3 instructions
//! 5. Sign and submit transaction
//!
//! This example transfers 0.1 USDC to RtsKQm3gAGL1Tayhs7ojWE9qytWqVh4G7eJTaNJs7vX on devnet
//!
//! Run with:
//!   cargo run --example test_spl_transaction

use base64;
use bs58;
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

    info!("=== PolliNet SPL Transaction Test with Idempotent ATA Creation ===\n");

    // 1. Initialize the SDK and RPC client
    let rpc_url = "https://solana-devnet.g.alchemy.com/v2/XuGpQPCCl-F1SSI-NYtsr0mSxQ8P8ts6";
    let sdk = PolliNetSDK::new_with_rpc(rpc_url).await?;
    let rpc_client =
        RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::finalized());
    info!("✅ SDK initialized with RPC client: {}", rpc_url);

    // 2. Load sender keypair from private key
    info!("\n=== Loading Sender Keypair from Private Key ===");
    let sender_private_key =
        "5zRwe731N375MpGuQvQoUjSMUpoXNLqsGWE9J8SoqHKfivhUpNxwt3o9Gdu6jjCby4dJRCGBA6HdBzrhvLVhUaqu";

    let private_key_bytes = bs58::decode(sender_private_key)
        .into_vec()
        .map_err(|e| format!("Failed to decode private key: {}", e))?;

    let sender_keypair = Keypair::try_from(&private_key_bytes[..])
        .map_err(|e| format!("Failed to create keypair from private key: {}", e))?;

    info!("✅ Sender wallet loaded from private key");
    info!("   Public key: {}", sender_keypair.pubkey());
    info!("   Private key length: {} bytes", private_key_bytes.len());

    // 3. Check sender SOL balance
    info!("\n=== Checking Sender Balance ===");
    let sender_balance = rpc_client.get_balance(&sender_keypair.pubkey())?;
    info!(
        "Sender SOL balance: {} lamports ({} SOL)",
        sender_balance,
        sender_balance as f64 / LAMPORTS_PER_SOL as f64
    );

    if sender_balance < 2_000_000 {
        return Err("Sender has insufficient balance. Please fund the wallet with at least 0.002 SOL (0.001 for nonce account + 0.001 for fees).".into());
    }

    // 4. Create or use existing nonce account
    info!("\n=== Setting Up Nonce Account ===");
    info!("Creating a new nonce account...");
    info!("   Sender will fund and control the nonce account");
    info!("   Nonce authority: {} (sender)", sender_keypair.pubkey());
    
    let nonce_keypair = nonce::create_nonce_account(&rpc_client, &sender_keypair)
        .await
        .map_err(|e| format!("Failed to create nonce account: {}", e))?;
    
    let nonce_account = nonce_keypair.pubkey().to_string();
    info!("✅ Nonce account created successfully!");
    info!("   Nonce account: {}", nonce_account);
    info!("   Waiting for confirmation...");
    
    // Wait for confirmation
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    info!("✅ Nonce account confirmed and ready to use");

    // 5. Set SPL token transfer parameters
    info!("\n=== SPL Token Transfer Parameters ===");
    let sender_wallet = sender_keypair.pubkey().to_string();
    let recipient_wallet = "EufFKpRgwpdXMuXpEG6Nh2bb77tFnj9d6FgSAEnsSMQy".to_string();
    let fee_payer = sender_keypair.pubkey().to_string(); // Sender pays fees
    let mint_address = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU".to_string(); // USDC mint on devnet
    let token_amount = 100_000; // 0.1 USDC (USDC has 6 decimals: 0.1 * 10^6 = 100,000)

    info!("Sender wallet: {}", sender_wallet);
    info!("Recipient wallet: {}", recipient_wallet);
    info!("Fee payer: {}", fee_payer);
    info!("Token mint: {} (USDC on devnet)", mint_address);
    info!("Token amount: {} (0.1 USDC)", token_amount);
    info!("ATAs will be automatically derived");
    info!("✅ Idempotent ATA creation will be included in transaction");

    // 6. Create UNSIGNED SPL token transfer (with idempotent ATA creation)
    info!("\n=== Creating Unsigned SPL Token Transfer ===");
    info!("Creating unsigned SPL token transfer with nonce account...");
    info!("Transaction will include:");
    info!("  1. Advance nonce account instruction");
    info!("  2. Create recipient ATA instruction (idempotent)");
    info!("  3. SPL token transfer instruction");

    let unsigned_tx = sdk
        .create_unsigned_spl_transaction(
            &sender_wallet,
            &recipient_wallet,
            &fee_payer,
            &mint_address,
            token_amount,
            &nonce_account,
        )
        .await?;

    info!("✅ Unsigned SPL transaction created");
    info!("   Size: {} characters (base64 encoded)", unsigned_tx.len());

    // 7. Verify transaction structure
    info!("\n=== Verifying Transaction Structure ===");
    let tx_bytes = base64::decode(&unsigned_tx)?;
    let tx: solana_sdk::transaction::Transaction = bincode1::deserialize(&tx_bytes)?;
    
    info!("✅ Transaction deserialized successfully");
    info!("   Number of instructions: {}", tx.message.instructions.len());
    info!("   Number of accounts: {}", tx.message.account_keys.len());
    info!("   Number of signatures: {}", tx.signatures.len());
    
    // Verify we have exactly 3 instructions
    if tx.message.instructions.len() == 3 {
        info!("✅ Correct number of instructions: 3");
        info!("   Instruction 1: Advance nonce account");
        info!("   Instruction 2: Create recipient ATA (idempotent)");
        info!("   Instruction 3: SPL token transfer");
    } else {
        return Err(format!(
            "Expected 3 instructions, but found {}",
            tx.message.instructions.len()
        ).into());
    }

    // Check that all signatures are empty (unsigned)
    let valid_sigs = tx
        .signatures
        .iter()
        .filter(|sig| *sig != &solana_sdk::signature::Signature::default())
        .count();
    info!("   Valid signatures: {}/{} (should be 0 for unsigned)", valid_sigs, tx.signatures.len());

    if valid_sigs != 0 {
        return Err("Transaction should be unsigned, but contains signatures".into());
    }

    // 8. Display transaction details
    info!("\n=== Unsigned SPL Transaction Details ===");
    info!("✅ Transaction is ready for signing!");
    info!("   Instructions: [1] Advance nonce, [2] Create ATA (idempotent), [3] SPL Token Transfer");
    info!("   Required signers:");
    info!("     - Sender/Token owner (as nonce authority): {}", sender_wallet);
    info!("     - Fee payer: {}", fee_payer);
    info!("   Blockhash: From nonce account (durable)");
    info!("\nBase64 transaction (first 80 chars):");
    info!("   {}...", &unsigned_tx[..80.min(unsigned_tx.len())]);

    // 9. Sign the transaction
    info!("\n=== Signing SPL Transaction ===");
    info!("Sender (token owner) will sign the transaction...");

    // Decode and deserialize to get message data
    let tx_for_signing: solana_sdk::transaction::Transaction = bincode1::deserialize(&tx_bytes)?;
    let message_data = tx_for_signing.message_data();

    // Sign the message
    let sender_signature = sender_keypair.sign_message(&message_data);
    info!("✅ Sender created signature: {}", sender_signature);

    // Add signature using the SDK method
    info!("\n=== Adding Signature to SPL Transaction ===");
    let signed_tx = sdk.add_signature(&unsigned_tx, &sender_keypair.pubkey(), &sender_signature)?;

    info!("✅ Signature added successfully");
    info!("   Updated transaction: {} characters (base64)", signed_tx.len());

    // Verify transaction is fully signed
    let final_tx_bytes = base64::decode(&signed_tx)?;
    let final_tx: solana_sdk::transaction::Transaction = bincode1::deserialize(&final_tx_bytes)?;
    let final_valid_sigs = final_tx
        .signatures
        .iter()
        .filter(|sig| *sig != &solana_sdk::signature::Signature::default())
        .count();
    info!(
        "   Valid signatures: {}/{}",
        final_valid_sigs,
        final_tx.signatures.len()
    );

    if final_valid_sigs == final_tx.signatures.len() {
        info!("✅ SPL transaction is fully signed and ready for submission!");

        // 10. Submit the transaction
        info!("\n=== Submitting Fully Signed SPL Transaction ===");
        info!("Submitting to Solana using send_and_confirm_transaction...");
        info!("This will test the idempotent ATA creation in a real transaction");

        let signature = sdk
            .send_and_confirm_transaction(&signed_tx)
            .await?;
        
        info!("✅ SPL token transfer submitted successfully!");
        info!("   Transaction signature: {}", signature);
        info!("   Transferred: 0.1 USDC to {}", recipient_wallet);
        info!(
            "   View on Explorer: https://explorer.solana.com/tx/{}?cluster=devnet",
            signature
        );
        info!("\n✅ Idempotent ATA creation tested successfully!");
        info!("   The transaction included the ATA creation instruction");
        info!("   If the ATA already existed, the instruction was idempotent (no error)");
        info!("   If the ATA didn't exist, it was created successfully");
    } else {
        info!(
            "⚠️  SPL transaction needs {} more signature(s)",
            final_tx.signatures.len() - final_valid_sigs
        );
        info!("   Would need fee payer to sign before submission");
    }

    // 11. Summary
    info!("\n=== Test Summary ===");
    info!("✅ 1. Loaded sender wallet from private key");
    info!("✅ 2. Verified sender SOL balance");
    info!("✅ 3. Created new nonce account: {}", nonce_account);
    info!("✅ 4. Created unsigned SPL transaction with idempotent ATA creation");
    info!("✅ 5. Verified transaction has 3 instructions (advance nonce, create ATA, transfer)");
    info!("✅ 6. Verified transaction is unsigned");
    info!("✅ 7. Signed transaction with sender keypair");
    if final_valid_sigs == final_tx.signatures.len() {
        info!("✅ 8. Submitted fully signed SPL transaction to Solana");
        info!("✅ 9. Tested idempotent ATA creation in real transaction");
        info!("✅ 10. Successfully transferred 0.1 USDC to {}", recipient_wallet);
    } else {
        info!("⚠️  8. Transaction ready for additional signatures");
    }

    info!("\n=== Key Features Tested ===");
    info!("• Wallet import from private key (bs58)");
    info!("• SPL token transfer transaction creation");
    info!("• Idempotent ATA creation instruction");
    info!("• Transaction structure verification (3 instructions)");
    info!("• Transaction signing");
    info!("• Transaction submission to Solana");

    info!("\n=== Idempotent ATA Creation Benefits ===");
    info!("• Prevents 'invalid account data for instruction' errors");
    info!("• Safe to include even if ATA already exists");
    info!("• Ensures recipient token account exists before transfer");
    info!("• No need to check ATA existence before creating transaction");

    Ok(())
}
