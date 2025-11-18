//! Example: Relay Presigned Custom Transaction
//!
//! This example demonstrates how to take a presigned transaction,
//! compress it, fragment it, and relay it over the BLE mesh network.
//!
//! Flow:
//! 1. Load sender keypair
//! 2. Create a presigned transaction (can be any custom transaction)
//! 3. Process and relay via SDK (automatic compression + fragmentation)
//! 4. Fragments are relayed over BLE mesh
//!
//! Use Cases:
//! - Relay any custom Solana transaction
//! - Forward transactions from other sources
//! - Mesh network transaction propagation
//! - Store-and-forward scenarios

mod wallet_utils;
use wallet_utils::{create_and_fund_wallet, get_rpc_url};

mod nonce_bundle_helper;
use nonce_bundle_helper::{get_next_nonce, load_bundle, save_bundle_after_use};

use base64;
use pollinet::PolliNetSDK;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::system_instruction;
use solana_sdk::transaction::Transaction;
use std::str::FromStr;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("=== PolliNet Relay Presigned Transaction Example ===\n");

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

    // 4. Create a custom presigned transaction
    info!("\n=== Creating Custom Presigned Transaction ===");
    info!("This could be ANY Solana transaction - we'll create a simple SOL transfer as example");

    // For this example, we'll create a signed transaction using the SDK
    let recipient = "RtsKQm3gAGL1Tayhs7ojWE9qytWqVh4G7eJTaNJs7vX";
    let amount = 500_000; // 0.0005 SOL

    info!("Creating presigned transaction:");
    info!("   From: {}", sender_keypair.pubkey());
    info!("   To: {}", recipient);
    info!("   Amount: {} lamports", amount);

    // Create a signed transaction (this could come from anywhere)
    let compressed_tx = sdk
        .create_transaction(
            &sender_keypair.pubkey().to_string(),
            &sender_keypair,
            recipient,
            amount,
            &nonce_account,
            &sender_keypair,
        )
        .await?;

    info!("✅ Transaction created and signed");
    info!("   Compressed size: {} bytes", compressed_tx.len());

    // For relay, we'll create another simple signed transaction in base64 format
    // In practice, this would come from another node or user
    info!("\n=== Creating Another Transaction (Simulating External Source) ===");
    info!("This simulates receiving a presigned transaction from another party...");

    // Create a simple signed transaction directly
    let nonce_pubkey = Pubkey::from_str(&nonce_account)?;
    let recipient_pubkey = Pubkey::from_str(recipient)?;
    let nonce_account_data = rpc_client.get_account(&nonce_pubkey)?;
    let nonce_state: solana_sdk::nonce::state::Versions =
        bincode1::deserialize(&nonce_account_data.data)?;
    let nonce_data = match nonce_state.state() {
        solana_sdk::nonce::State::Initialized(data) => data.clone(),
        _ => return Err("Nonce not initialized".into()),
    };

    let advance_ix =
        system_instruction::advance_nonce_account(&nonce_pubkey, &sender_keypair.pubkey());
    let transfer_ix =
        system_instruction::transfer(&sender_keypair.pubkey(), &recipient_pubkey, amount);

    let mut custom_tx =
        Transaction::new_with_payer(&[advance_ix, transfer_ix], Some(&sender_keypair.pubkey()));
    custom_tx.message.recent_blockhash = nonce_data.blockhash();
    custom_tx.sign(&[&sender_keypair], nonce_data.blockhash());

    // Serialize and encode to base64
    let tx_bytes = bincode1::serialize(&custom_tx)?;
    let base64_tx = base64::encode(&tx_bytes);

    info!("✅ Presigned transaction ready for relay");
    info!("   Transaction size: {} bytes", tx_bytes.len());
    info!("   Base64 length: {} characters", base64_tx.len());
    info!(
        "   First 60 chars: {}...",
        &base64_tx[..60.min(base64_tx.len())]
    );

    // 6. Process and relay the presigned transaction
    info!("\n=== Processing and Relaying Transaction ===");
    info!("Calling process_and_relay_transaction...");
    info!("This will:");
    info!("  1. Validate the transaction is signed");
    info!("  2. Compress the transaction (if > 100 bytes)");
    info!("  3. Fragment into {} byte chunks", pollinet::BLE_MTU_SIZE);
    info!("  4. Add SHA-256 checksums to fragments");
    info!("  5. Relay fragments over BLE mesh");

    let tx_id = sdk.process_and_relay_transaction(&base64_tx).await?;

    // Mark nonce as used after processing transaction
    save_bundle_after_use(&mut bundle, nonce_index)?;

    info!("✅ Transaction processed and relayed successfully!");
    info!("   Transaction ID: {}", tx_id);
    info!("   Fragments sent over BLE mesh network");

    // 7. Summary
    info!("\n=== Relay Transaction Summary ===");
    info!("✅ 1. Loaded sender keypair");
    info!("✅ 2. Created custom presigned transaction");
    info!("✅ 3. Encoded to base64 format");
    info!("✅ 4. Processed transaction:");
    info!("      - Validated signatures");
    info!("      - Compressed with LZ4");
    info!("      - Fragmented with SHA-256 checksums");
    info!("✅ 5. Relayed fragments over BLE mesh");
    info!("✅ 6. Transaction ID: {}", tx_id);

    info!("\n=== Implementation Notes ===");
    info!("• Accepts any presigned Solana transaction");
    info!("• Transaction must be signed (validated before processing)");
    info!("• Automatically compresses if > 100 bytes");
    info!("• Fragments include SHA-256 checksums");
    info!("• Relays over configured BLE mesh transport");
    info!("• Returns transaction ID for tracking");
    info!("• Can relay SOL transfers, SPL transfers, or any custom instruction");

    info!("\n=== Use Cases ===");
    info!("• Mesh Network Relay: Forward transactions from other nodes");
    info!("• Store-and-Forward: Process and relay cached transactions");
    info!("• Transaction Services: Relay transactions for users");
    info!("• Custom Instructions: Relay any Solana transaction type");
    info!("• Offline Propagation: Relay transactions through mesh network");

    info!("\n=== Integration Example ===");
    info!("// Receive presigned transaction from another node");
    info!("let presigned_tx_base64 = receive_from_peer()?;");
    info!("");
    info!("// Process and relay through mesh network");
    info!("let tx_id = sdk.process_and_relay_transaction(&presigned_tx_base64).await?;");
    info!("");
    info!("// Track relay status");
    info!("println!(\"Relayed transaction: {{}}\", tx_id);");

    Ok(())
}
