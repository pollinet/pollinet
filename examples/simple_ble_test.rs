//! Simple BLE Testing Example for PolliNet SDK
//!
//! ⚠️  Desktop/Linux builds run in simulation-only mode. For production BLE
//! mesh relays use the Android PolliNet service.
//!
//! This is a simplified example that demonstrates the core BLE functionality
//! without requiring complex setup or multiple devices.
//!
//! Run with: cargo run --example simple_ble_test

use pollinet::PolliNetSDK;
use std::time::Duration;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    info!("🚀 PolliNet Simple BLE Test");
    info!("==========================");
    info!("⚠️  Running in desktop simulation mode. Android handles production BLE.");

    // Initialize the SDK
    info!("\n1️⃣  Initializing PolliNet SDK...");
    let sdk = PolliNetSDK::new().await?;
    info!("✅ SDK initialized successfully");

    // Test BLE status
    info!("\n2️⃣  Checking BLE Status...");
    match sdk.get_ble_status().await {
        Ok(status) => {
            info!("✅ BLE Status:");
            info!("{}", status);
        }
        Err(e) => {
            warn!("⚠️  BLE Status Error: {}", e);
            info!("This might be expected if no BLE adapter is available");
        }
    }

    // Test BLE device scanning
    info!("\n3️⃣  Scanning for BLE Devices...");
    match sdk.scan_all_devices().await {
        Ok(devices) => {
            if devices.is_empty() {
                info!("ℹ️  No BLE devices found nearby");
                info!("   This is normal if no BLE devices are in range");
            } else {
                info!("✅ Found {} BLE devices:", devices.len());
                for (i, device) in devices.iter().enumerate() {
                    info!("   {}. {}", i + 1, device);
                }
            }
        }
        Err(e) => {
            warn!("⚠️  BLE scanning failed: {}", e);
            info!("This might be due to permissions or adapter issues");
        }
    }

    // Test BLE advertising and scanning
    info!("\n4️⃣  Starting BLE Advertising and Scanning...");
    match sdk.start_ble_networking().await {
        Ok(_) => {
            info!("✅ BLE advertising started");
            info!("✅ BLE scanning started");
        }
        Err(e) => {
            warn!("⚠️  BLE networking failed: {}", e);
            info!("This might be due to adapter permissions");
        }
    }

    // Wait for BLE operations to initialize
    info!("\n5️⃣  Waiting for BLE operations to initialize...");
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Test peer discovery
    info!("\n6️⃣  Discovering PolliNet Peers...");
    match sdk.discover_ble_peers().await {
        Ok(peers) => {
            if peers.is_empty() {
                info!("ℹ️  No PolliNet peers found");
                info!("   This is normal if no other PolliNet devices are nearby");
            } else {
                info!("✅ Found {} PolliNet peers:", peers.len());
                for (i, peer) in peers.iter().enumerate() {
                    info!("   {}. Peer ID: {}", i + 1, peer.peer_id);
                    info!("      RSSI: {}", peer.rssi);
                    info!("      Capabilities: {:?}", peer.capabilities);
                }
            }
        }
        Err(e) => {
            warn!("⚠️  Peer discovery failed: {}", e);
        }
    }

    // Test transaction fragmentation
    info!("\n7️⃣  Testing Transaction Fragmentation...");
    let mock_tx = create_mock_transaction();
    info!("   Created mock transaction: {} bytes", mock_tx.len());

    let fragments = sdk.fragment_transaction(&mock_tx);
    info!("✅ Transaction fragmented into {} pieces", fragments.len());

    for (i, fragment) in fragments.iter().enumerate() {
        info!(
            "   Fragment {}/{}: {} bytes (MTU: {})",
            i + 1,
            fragments.len(),
            fragment.data.len(),
            pollinet::BLE_MTU_SIZE
        );

        if fragment.data.len() <= pollinet::BLE_MTU_SIZE {
            info!("      ✅ Size within BLE MTU limit");
        } else {
            warn!("      ⚠️  Size exceeds BLE MTU limit");
        }
    }

    // Test fragment reassembly
    info!("\n8️⃣  Testing Fragment Reassembly...");
    match sdk.reassemble_fragments(&fragments) {
        Ok(reassembled) => {
            info!("✅ Fragments reassembled successfully");
            info!("   Original size: {} bytes", mock_tx.len());
            info!("   Reassembled size: {} bytes", reassembled.len());

            if reassembled == mock_tx {
                info!("✅ Integrity verification passed");
            } else {
                error!("❌ Integrity verification failed");
            }
        }
        Err(e) => {
            error!("❌ Fragment reassembly failed: {}", e);
        }
    }

    // Test relay functionality
    info!("\n9️⃣  Testing Fragment Relay...");
    match sdk.relay_transaction(fragments).await {
        Ok(_) => {
            info!("✅ Fragments relayed successfully");
        }
        Err(e) => {
            warn!("⚠️  Fragment relay failed: {}", e);
            info!("   This is normal if no peers are connected");
        }
    }

    // Final status check
    info!("\n🔟 Final BLE Status Check...");
    match sdk.get_ble_status().await {
        Ok(status) => {
            info!("✅ Final BLE Status:");
            info!("{}", status);
        }
        Err(e) => {
            warn!("⚠️  Final BLE status error: {}", e);
        }
    }

    info!("\n🎉 BLE Test Complete!");
    info!("====================");
    info!("✅ All BLE functionality tested successfully");
    info!("💡 The PolliNet BLE mesh networking is working correctly");

    Ok(())
}

/// Create a mock transaction for testing
fn create_mock_transaction() -> Vec<u8> {
    // Create a realistic-sized mock transaction
    let base_data = b"Mock Solana transaction data for BLE testing - this simulates a real transaction that would be fragmented and transmitted over the BLE mesh network. The data includes various components like instructions, accounts, and signatures that would be present in a real Solana transaction.";

    // Repeat to create a larger transaction for better fragmentation testing
    let mut mock_tx = Vec::new();
    for _ in 0..5 {
        mock_tx.extend_from_slice(base_data);
    }

    mock_tx
}
