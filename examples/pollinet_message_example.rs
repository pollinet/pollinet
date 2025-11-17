//! Example of creating and transmitting transactions
//!
//! ⚠️  Desktop/Linux BLE networking is simulation-only. Use the Android PolliNet
//! app for production BLE relays.

use pollinet::PolliNetSDK;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    
    // Initialize SDK
    let sdk = PolliNetSDK::new().await?;
    sdk.start_ble_networking().await?;
    println!("⚠️ Desktop BLE adapter is simulation-only. Android handles production relays.");
    
    // Create keypairs for the example
    let sender_keypair = solana_sdk::signature::Keypair::new();
    let nonce_authority_keypair = solana_sdk::signature::Keypair::new();
    
    // Create transaction
    let compressed_tx = sdk.create_transaction(
        "11111111111111111111111111111112",           // Sender
        &sender_keypair,                              // Sender keypair
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", // Recipient
        100_000_000_000,                              // Amount (100 SOL)
        "11111111111111111111111111111113",           // Nonce account
        &nonce_authority_keypair,                     // Nonce authority keypair
    ).await?;
    
    println!("Created transaction:");
    println!("  Size: {} bytes", compressed_tx.len());
    
    // Fragment the transaction
    println!("\nFragmenting transaction for BLE transmission...");
    let fragments = sdk.fragment_transaction(&compressed_tx);
    println!("Transaction fragmented into {} pieces", fragments.len());
    
    // Relay over BLE mesh
    println!("Relaying transaction over BLE mesh...");
    sdk.relay_transaction(fragments).await?;
    println!("Transaction relayed successfully!");
    
    Ok(())
}
