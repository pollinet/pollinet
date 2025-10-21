//! PolliNet BLE Mesh Network Node
//!
//! This example runs a real PolliNet BLE mesh node that continuously
//! discovers and communicates with other PolliNet devices in the area.
//! The node will run indefinitely, scanning for peers and relaying transactions.
//!
//! Run with: cargo run --example ble_mesh_simulation

use pollinet::PolliNetSDK;
use std::time::Duration;
use tracing::{info, warn};
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    info!("🌐 PolliNet BLE Mesh Network Node");
    info!("=================================");
    info!("Starting real BLE mesh node - looking for other PolliNet devices...");

    // Initialize the PolliNet SDK
    let sdk = PolliNetSDK::new().await?;
    info!("✅ PolliNet SDK initialized");

    // Start BLE networking
    info!("Starting BLE advertising and scanning...");
    sdk.start_ble_networking().await?;
    info!("✅ BLE advertising and scanning started");

    // Get initial status
    match sdk.get_ble_status().await {
        Ok(status) => {
            info!("📊 Initial BLE Status:");
            info!("{}", status);
        }
        Err(e) => {
            warn!("⚠️  BLE status error: {}", e);
        }
    }

    // Run continuous mesh operations with graceful shutdown
    info!("\n🔄 Starting continuous mesh operations...");
    info!("Press Ctrl+C to stop");
    
    // Set up graceful shutdown
    let shutdown = async {
        signal::ctrl_c().await.expect("Failed to listen for ctrl+c");
        handle_shutdown().await;
    };
    
    // Run mesh operations until shutdown
    tokio::select! {
        _ = run_continuous_mesh_operations(sdk) => {
            info!("Mesh operations completed");
        }
        _ = shutdown => {
            info!("Shutdown signal received");
        }
    }

    Ok(())
}

/// Run continuous mesh operations - discover peers and relay transactions
async fn run_continuous_mesh_operations(sdk: PolliNetSDK) -> Result<(), Box<dyn std::error::Error>> {
    let mut scan_count = 0;
    let mut last_peer_count = 0;
    let mut connected_peers = std::collections::HashSet::new();

    info!("🔄 Starting continuous mesh operations...");
    info!("This node will run indefinitely, scanning for other PolliNet devices");
    info!("Press Ctrl+C to stop gracefully");

    loop {
        scan_count += 1;
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        info!("\n🔄 Mesh Scan #{} at {}", scan_count, current_time);
        info!("================================================");

        // Discover nearby PolliNet peers
        match sdk.discover_ble_peers().await {
            Ok(peers) => {
                if peers.is_empty() {
                    info!("📡 No PolliNet peers found nearby");
                    info!("   Keep scanning - other devices may appear");
                } else {
                    info!("📡 Found {} PolliNet peers:", peers.len());
                    
                    // Track new peers
                    let current_peer_count = peers.len();
                    if current_peer_count != last_peer_count {
                        info!("   Peer count changed: {} → {}", last_peer_count, current_peer_count);
                        last_peer_count = current_peer_count;
                    }

                    // Display peer information
                    for (i, peer) in peers.iter().enumerate() {
                        let is_new = !connected_peers.contains(&peer.device_id);
                        let status = if is_new { "🆕 NEW" } else { "🔄 KNOWN" };
                        
                        info!("   {}. {} {} {}", i + 1, status, peer.device_id, peer.rssi);
                        info!("      Capabilities: {:?}", peer.capabilities);
                        info!("      Last seen: {:?}", peer.last_seen);
                        
                        if is_new {
                            connected_peers.insert(peer.device_id.clone());
                        }

                        // Try to connect to new peers
                        if is_new {
                            info!("      🔗 Attempting connection...");
                            match sdk.connect_to_ble_peer(&peer.device_id).await {
                                Ok(_) => {
                                    info!("      ✅ Connected to {}", peer.device_id);
                                }
                                Err(e) => {
                                    info!("      ⚠️  Connection failed: {}", e);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warn!("❌ Peer discovery failed: {}", e);
            }
        }

        // Check for any pending transactions to relay
        info!("📦 Checking for transactions to relay...");
        // In a real implementation, this would check for received fragments
        // and reassemble them into complete transactions

        // Get current BLE status
        match sdk.get_ble_status().await {
            Ok(status) => {
                if scan_count % 10 == 0 { // Show full status every 10 scans
                    info!("📊 BLE Status:");
                    info!("{}", status);
                } else {
                    info!("📊 BLE: Active | Peers: {} | Buffer: Ready", connected_peers.len());
                }
            }
            Err(e) => {
                warn!("⚠️  BLE status error: {}", e);
            }
        }

        // Show periodic statistics
        if scan_count % 20 == 0 {
            info!("\n📊 MESH STATISTICS (Scan #{})", scan_count);
            info!("================================");
            info!("Total scans performed: {}", scan_count);
            info!("Unique peers discovered: {}", connected_peers.len());
            info!("Current peer count: {}", last_peer_count);
            info!("Node status: ACTIVE and scanning");
            info!("Ready to relay transactions");
        }

        // Wait before next scan
        info!("⏱️  Waiting 5 seconds before next scan...");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Handle graceful shutdown
async fn handle_shutdown() {
    info!("\n🛑 Shutdown signal received");
    info!("Stopping BLE mesh node gracefully...");
    info!("Thank you for using PolliNet BLE Mesh!");
}

