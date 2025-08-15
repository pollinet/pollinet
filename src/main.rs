//! PolliNet SDK demonstration
//! 
//! Shows how to use the PolliNet SDK for offline Solana transaction propagation

use pollinet::PolliNetSDK;
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    info!("🚀 Starting PolliNet SDK demonstration...");
    
    // Initialize the PolliNet SDK
    let sdk = PolliNetSDK::new().await?;
    info!("✅ PolliNet SDK initialized successfully");
    
    // Start BLE networking
    sdk.start_ble_networking().await?;
    info!("📡 BLE networking started");
    
    // Example 1: Create and relay a transaction
    info!("\n📝 Example 1: Creating and relaying a transaction");
    match create_and_relay_transaction(&sdk).await {
        Ok(_) => info!("✅ Transaction created and relayed successfully"),
        Err(e) => error!("❌ Failed to create and relay transaction: {}", e),
    }
    
    // Example 2: Cast a governance vote
    info!("\n🗳️  Example 2: Casting a governance vote");
    match sdk.cast_vote("proposal_123", 1).await {
        Ok(_) => info!("✅ Vote cast successfully"),
        Err(e) => error!("❌ Failed to cast vote: {}", e),
    }
    
    // Example 3: Submit transaction when online
    info!("\n🌐 Example 3: Submitting transaction to Solana");
    match submit_transaction_example(&sdk).await {
        Ok(_) => info!("✅ Transaction submission example completed"),
        Err(e) => error!("❌ Failed to submit transaction: {}", e),
    }
    
    info!("\n🎉 PolliNet SDK demonstration completed!");
    info!("💡 The SDK is now running and ready for offline transaction propagation");
    
    // Keep the SDK running
    tokio::signal::ctrl_c().await?;
    info!("👋 Shutting down PolliNet SDK...");
    
    Ok(())
}

/// Example: Create and relay a transaction
async fn create_and_relay_transaction(sdk: &PolliNetSDK) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Create and sign transaction
    info!("   Creating transaction from Alice to Bob for 100 SOL...");
    let compressed_tx = sdk.create_transaction(
        "Alice123456789012345678901234567890123456789012345678901234567890",
        "Bob123456789012345678901234567890123456789012345678901234567890",
        100_000_000_000, // 100 SOL in lamports
    ).await?;
    
    info!("   Transaction created and compressed ({} bytes)", compressed_tx.len());
    
    // 2. Fragment transaction for BLE transmission
    info!("   Fragmenting transaction for BLE transmission...");
    let fragments = sdk.fragment_transaction(&compressed_tx);
    info!("   Transaction fragmented into {} pieces", fragments.len());
    
    // 3. Relay transaction fragments over BLE mesh
    info!("   Relaying transaction fragments over BLE mesh...");
    sdk.relay_transaction(fragments).await?;
    info!("   Transaction fragments relayed successfully");
    
    Ok(())
}

/// Example: Submit transaction to Solana
async fn submit_transaction_example(sdk: &PolliNetSDK) -> Result<(), Box<dyn std::error::Error>> {
    // Create a mock transaction for demonstration
    let mock_transaction = b"mock_solana_transaction_data";
    
    // Submit to Solana (this would happen when device regains internet)
    info!("   Submitting transaction to Solana RPC...");
    let signature = sdk.submit_transaction_to_solana(mock_transaction).await?;
    info!("   Transaction submitted successfully with signature: {}", signature);
    
    // Broadcast confirmation back through BLE mesh
    info!("   Broadcasting confirmation through BLE mesh...");
    sdk.broadcast_confirmation(&signature).await?;
    info!("   Confirmation broadcasted successfully");
    
    Ok(())
}

/// Example: Demonstrate the complete pollination flow
async fn demonstrate_pollination_flow(sdk: &PolliNetSDK) -> Result<(), Box<dyn std::error::Error>> {
    info!("\n🌸 Demonstrating complete pollination flow...");
    
    // 1. Create transaction (pollen grain)
    info!("   1. 🌱 Creating transaction (pollen grain)...");
    let compressed_tx = sdk.create_transaction(
        "Farmer123456789012345678901234567890123456789012345678901234567890",
        "Market123456789012345678901234567890123456789012345678901234567890",
        50_000_000_000, // 50 SOL
    ).await?;
    
    // 2. Fragment and relay (pollen dispersal)
    info!("   2. 🌬️  Fragmenting and relaying (pollen dispersal)...");
    let fragments = sdk.fragment_transaction(&compressed_tx);
    sdk.relay_transaction(fragments).await?;
    
    // 3. Wait for submission (pollination)
    info!("   3. 🐝 Waiting for submission (pollination)...");
    info!("      Transaction is now propagating through the BLE mesh network");
    info!("      Any device with internet can submit it to Solana");
    
    // 4. Confirmation (fruit formation)
    info!("   4. 🍎 Confirmation will be broadcasted back (fruit formation)");
    info!("      Origin device will receive updated nonce for next transaction");
    
    Ok(())
}
