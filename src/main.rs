//! PolliNet SDK demonstration
//!
//! Shows how to use the PolliNet SDK for offline Solana transaction propagation

use pollinet::{transaction, PolliNetSDK};
use tracing::{error, info};

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

    // Example 0: Discover and connect to BLE peers
    info!("\n🔍 Example 0: Discovering BLE peers");
    match discover_ble_peers(&sdk).await {
        Ok(_) => info!("✅ BLE peer discovery completed"),
        Err(e) => error!("❌ Failed to discover BLE peers: {}", e),
    }

    // Show BLE status
    info!("\n📊 BLE Status Check:");
    match sdk.get_ble_status().await {
        Ok(status) => info!("{}", status),
        Err(e) => error!("❌ Failed to get BLE status: {}", e),
    }

    // Debug: Scan for ALL BLE devices
    info!("\n🔍 Debug: Scanning for ALL BLE devices:");
    match sdk.scan_all_devices().await {
        Ok(devices) => {
            if devices.is_empty() {
                info!("   No BLE devices found at all");
            } else {
                info!("   Found {} total BLE devices:", devices.len());
                for device in devices {
                    info!("     {}", device);
                }
            }
        }
        Err(e) => error!("   ❌ Failed to scan all devices: {}", e),
    }

    // Example 1: Create and relay a transaction
    info!("\n📝 Example 1: Creating and relaying a transaction");
    match create_and_relay_transaction(&sdk).await {
        Ok(_) => info!("✅ Transaction created and relayed successfully"),
        Err(e) => error!("❌ Failed to create and relay transaction: {}", e),
    }

    // Example 2: Cast a governance vote
    info!("\n🗳️  Example 2: Casting a governance vote");
    match sdk.cast_vote("11111111111111111111111111111112", 1).await {
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

    // Continuous BLE scanning for debugging
    info!("\n🔄 Starting continuous BLE scanning (press Ctrl+C to stop)...");
    continuous_ble_scanning(&sdk).await?;

    Ok(())
}

/// Continuous BLE scanning for debugging
async fn continuous_ble_scanning(sdk: &PolliNetSDK) -> Result<(), Box<dyn std::error::Error>> {
    let mut scan_count = 0;

    loop {
        scan_count += 1;
        info!(
            "\n🔄 BLE Scan #{} at {}",
            scan_count,
            chrono::Utc::now().format("%H:%M:%S")
        );

        // Scan for PolliNet peers
        match sdk.discover_ble_peers().await {
            Ok(peers) => {
                if peers.is_empty() {
                    info!("   No PolliNet peers found");
                } else {
                    info!("   Found {} PolliNet peers:", peers.len());
                    for peer in peers {
                        info!("     - {} (RSSI: {})", peer.device_id, peer.rssi);
                    }
                }
            }
            Err(e) => error!("   ❌ Scan failed: {}", e),
        }

        // Wait before next scan
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }

    Ok(())
}

/// Example: Discover BLE peers
async fn discover_ble_peers(sdk: &PolliNetSDK) -> Result<(), Box<dyn std::error::Error>> {
    info!("   Discovering nearby BLE peers...");

    // Discover peers
    let peers = sdk.discover_ble_peers().await?;
    info!("   Found {} potential peers", peers.len());

    // Try to connect to the first peer if available
    if let Some(first_peer) = peers.first() {
        info!("   Attempting to connect to peer: {}", first_peer.device_id);
        match sdk.connect_to_ble_peer(&first_peer.device_id).await {
            Ok(_) => info!(
                "   ✅ Successfully connected to peer: {}",
                first_peer.device_id
            ),
            Err(e) => info!(
                "   ⚠️  Could not connect to peer: {} (Error: {})",
                first_peer.device_id, e
            ),
        }
    }

    Ok(())
}

/// Example: Create and relay a transaction
async fn create_and_relay_transaction(sdk: &PolliNetSDK) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Create and sign transaction
    info!("   Creating transaction from Alice to Bob for 100 SOL...");

    // Use proper Solana public keys (these are example keys - in production you'd use real ones)
    let alice_pubkey = "11111111111111111111111111111112"; // System Program ID as example
    let bob_pubkey = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"; // SPL Token Program ID as example

    let compressed_tx = sdk
        .create_transaction(
            alice_pubkey,
            bob_pubkey,
            100_000_000_000, // 100 SOL in lamports
        )
        .await?;

    info!(
        "   Transaction created and compressed ({} bytes)",
        compressed_tx.len()
    );

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
    let mock_transaction = transaction::MockTransaction {
        sender: "11111111111111111111111111111112".to_string(),
        recipient: "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".to_string(),
        amount: 50_000_000_000, // 50 SOL
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    // Serialize the mock transaction
    let serialized = serde_json::to_vec(&mock_transaction)?;

    // Submit to Solana (this would happen when device regains internet)
    info!("   Submitting transaction to Solana RPC...");
    let signature = sdk.submit_transaction_to_solana(&serialized).await?;
    info!(
        "   Transaction submitted successfully with signature: {}",
        signature
    );

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
    let compressed_tx = sdk
        .create_transaction(
            "11111111111111111111111111111112", // System Program ID as example
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", // SPL Token Program ID as example
            50_000_000_000,                     // 50 SOL
        )
        .await?;

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
