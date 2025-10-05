//! Example of creating and transmitting transactions

use pollinet::PolliNetSDK;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    
    // Initialize SDK
    let sdk = PolliNetSDK::new().await?;
    sdk.start_ble_networking().await?;
    
    // Create transaction
    let compressed_tx = sdk.create_transaction(
        "11111111111111111111111111111112",           // Sender
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", // Recipient
        100_000_000_000,                              // Amount (100 SOL)
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
