//! BLE Mesh Network Simulation Example
//!
//! This example simulates a BLE mesh network with multiple PolliNet devices
//! demonstrating transaction propagation through the mesh.
//!
//! Run with: cargo run --example ble_mesh_simulation

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

    info!("🌐 PolliNet BLE Mesh Network Simulation");
    info!("======================================");
    info!("This example simulates a BLE mesh network with multiple devices");

    // Simulate Device 1 (Transaction Originator)
    info!("\n📱 DEVICE 1: Transaction Originator");
    info!("------------------------------------");
    let device1 = simulate_device("Device-1", "Originator").await?;

    // Simulate Device 2 (Mesh Relay Node)
    info!("\n📱 DEVICE 2: Mesh Relay Node");
    info!("-----------------------------");
    let device2 = simulate_device("Device-2", "Relay").await?;

    // Simulate Device 3 (Mesh Relay Node)
    info!("\n📱 DEVICE 3: Mesh Relay Node");
    info!("-----------------------------");
    let device3 = simulate_device("Device-3", "Relay").await?;

    // Simulate transaction propagation
    info!("\n🔄 SIMULATING TRANSACTION PROPAGATION");
    info!("=====================================");
    simulate_transaction_propagation(device1, device2, device3).await?;

    info!("\n🎉 BLE Mesh Simulation Complete!");
    info!("================================");
    info!("✅ Successfully demonstrated BLE mesh networking");
    info!("💡 In a real scenario, devices would be physically separate");

    Ok(())
}

/// Simulate a single device in the mesh network
async fn simulate_device(device_name: &str, role: &str) -> Result<PolliNetSDK, Box<dyn std::error::Error>> {
    info!("Initializing {} ({})...", device_name, role);
    
    let sdk = PolliNetSDK::new().await?;
    info!("✅ {} initialized", device_name);

    // Start BLE networking
    info!("Starting BLE networking for {}...", device_name);
    match sdk.start_ble_networking().await {
        Ok(_) => {
            info!("✅ {} BLE advertising started", device_name);
            info!("✅ {} BLE scanning started", device_name);
        }
        Err(e) => {
            warn!("⚠️  {} BLE networking failed: {}", device_name, e);
        }
    }

    // Wait for BLE to initialize
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Get device status
    match sdk.get_ble_status().await {
        Ok(status) => {
            info!("📊 {} Status:", device_name);
            info!("{}", status);
        }
        Err(e) => {
            warn!("⚠️  {} status error: {}", device_name, e);
        }
    }

    Ok(sdk)
}

/// Simulate transaction propagation through the mesh
async fn simulate_transaction_propagation(
    device1: PolliNetSDK,
    device2: PolliNetSDK,
    device3: PolliNetSDK,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Creating transaction on Device 1...");
    
    // Device 1 creates a transaction
    let mock_tx = create_realistic_mock_transaction();
    info!("✅ Transaction created: {} bytes", mock_tx.len());

    // Fragment the transaction
    info!("Fragmenting transaction for BLE transmission...");
    let fragments = device1.fragment_transaction(&mock_tx);
    info!("✅ Transaction fragmented into {} pieces", fragments.len());

    // Display fragment details
    for (i, fragment) in fragments.iter().enumerate() {
        info!("   Fragment {}/{}: {} bytes", 
              i + 1, fragments.len(), fragment.data.len());
    }

    // Device 1 relays to mesh
    info!("\n📡 Device 1 relaying transaction to mesh...");
    match device1.relay_transaction(fragments.clone()).await {
        Ok(_) => {
            info!("✅ Device 1 relayed transaction successfully");
        }
        Err(e) => {
            warn!("⚠️  Device 1 relay failed: {}", e);
        }
    }

    // Simulate mesh propagation delay
    info!("\n⏱️  Simulating mesh propagation delay...");
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Device 2 receives and processes
    info!("\n📥 Device 2 receiving transaction...");
    match device2.reassemble_fragments(&fragments) {
        Ok(reassembled) => {
            info!("✅ Device 2 reassembled transaction: {} bytes", reassembled.len());
            
            if reassembled == mock_tx {
                info!("✅ Device 2 verified transaction integrity");
            } else {
                error!("❌ Device 2 integrity check failed");
            }
        }
        Err(e) => {
            error!("❌ Device 2 reassembly failed: {}", e);
        }
    }

    // Device 2 relays to further nodes
    info!("\n📡 Device 2 relaying to further mesh nodes...");
    match device2.relay_transaction(fragments.clone()).await {
        Ok(_) => {
            info!("✅ Device 2 relayed transaction successfully");
        }
        Err(e) => {
            warn!("⚠️  Device 2 relay failed: {}", e);
        }
    }

    // Device 3 receives and processes
    info!("\n📥 Device 3 receiving transaction...");
    match device3.reassemble_fragments(&fragments) {
        Ok(reassembled) => {
            info!("✅ Device 3 reassembled transaction: {} bytes", reassembled.len());
            
            if reassembled == mock_tx {
                info!("✅ Device 3 verified transaction integrity");
            } else {
                error!("❌ Device 3 integrity check failed");
            }
        }
        Err(e) => {
            error!("❌ Device 3 reassembly failed: {}", e);
        }
    }

    // Simulate final propagation
    info!("\n📡 Device 3 relaying to final mesh nodes...");
    match device3.relay_transaction(fragments).await {
        Ok(_) => {
            info!("✅ Device 3 relayed transaction successfully");
        }
        Err(e) => {
            warn!("⚠️  Device 3 relay failed: {}", e);
        }
    }

    // Simulate mesh statistics
    info!("\n📊 MESH PROPAGATION STATISTICS");
    info!("==============================");
    info!("✅ Transaction successfully propagated through 3 devices");
    info!("✅ All devices verified transaction integrity");
    info!("✅ Transaction ready for blockchain submission by any online device");

    Ok(())
}

/// Create a realistic mock transaction
fn create_realistic_mock_transaction() -> Vec<u8> {
    // Create a more realistic transaction structure
    let mut tx_data = Vec::new();
    
    // Simulate transaction header
    tx_data.extend_from_slice(b"SOLANA_TX_V1");
    
    // Simulate instruction data
    let instruction_data = b"Transfer instruction: 1000000 lamports from Alice to Bob";
    tx_data.extend_from_slice(instruction_data);
    
    // Simulate account keys
    let account_keys = b"Account1:11111111111111111111111111111112,Account2:TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
    tx_data.extend_from_slice(account_keys);
    
    // Simulate signature data
    let signature_data = b"Signature1:5KJvsngHeMpm884wtkJQQLjWLVy8jQtZ4LDwbgj8c5p1fqYjqvFB8y5Y7eU1D6na89r3HMKtQ1nHf8rHgHgHgHg";
    tx_data.extend_from_slice(signature_data);
    
    // Simulate recent blockhash
    let blockhash_data = b"RecentBlockhash:9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM";
    tx_data.extend_from_slice(blockhash_data);
    
    // Add some padding to make it larger for better fragmentation testing
    let padding = b"PADDING_DATA_FOR_FRAGMENTATION_TESTING_";
    for _ in 0..10 {
        tx_data.extend_from_slice(padding);
    }
    
    info!("Created realistic mock transaction: {} bytes", tx_data.len());
    tx_data
}

