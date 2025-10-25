//! Example: Offline Transaction Receiver
//!
//! This example demonstrates the complete offline transaction receiving workflow:
//!
//! 1. Scan for PolliNet devices and connect
//! 2. Receive transaction fragments and reassemble them
//! 3. Decompress the transaction
//! 4. Submit the transaction to the blockchain
//!
//! Run with: cargo run --example offline_transaction_receiver

use bincode1;
use pollinet::PolliNetSDK;
use pollinet::util::lz::Lz4Compressor;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::transaction::Transaction;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error};
use tokio::time::{sleep, Duration};
use base64;

// Fragment reassembly buffer
type FragmentBuffer = Arc<RwLock<HashMap<String, Vec<u8>>>>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("📥 === PolliNet Offline Transaction Receiver ===");
    info!("This example demonstrates receiving and processing offline transactions");

    // Initialize fragment buffer for reassembly
    let fragment_buffer: FragmentBuffer = Arc::new(RwLock::new(HashMap::new()));

    // ================================================================
    // STEP 1: Initialize PolliNet SDK and start BLE scanning
    // ================================================================
    info!("\n📡 STEP 1: Starting BLE scanning for PolliNet devices...");
    
    let sdk = PolliNetSDK::new().await?;
    info!("✅ PolliNet SDK initialized");
    
    // Start BLE networking (advertising + scanning)
    sdk.start_ble_networking().await?;
    info!("📢 BLE advertising and scanning started");
    
    // Start text message listener
    sdk.start_text_listener().await?;
    info!("🎧 Text message listener started");
    
    // ================================================================
    // STEP 2: Scan for devices and establish connections
    // ================================================================
    info!("\n🔍 STEP 2: Scanning for PolliNet devices...");
    info!("   Looking for devices advertising PolliNet service...");
    
    let mut scan_count = 0;
    let max_scans = 5; // Reduced for demo
    let mut found_peers = false;
    
    loop {
        scan_count += 1;
        info!("\n🔍 Scan #{}/{}", scan_count, max_scans);
        
        // Discover peers
        match sdk.discover_ble_peers().await {
            Ok(peers) => {
                if !peers.is_empty() {
                    found_peers = true;
                    info!("📱 Found {} peer(s):", peers.len());
                    for (i, peer) in peers.iter().enumerate() {
                        info!("   {}. {} (RSSI: {})", i + 1, peer.device_id, peer.rssi);
                    }
                    
                    // Try to connect to each peer
                    for peer in &peers {
                        info!("🔗 Attempting connection to: {}", peer.device_id);
                        
                        match sdk.connect_to_ble_peer(&peer.device_id).await {
                            Ok(_) => {
                                info!("✅ Connected to peer: {}", peer.device_id);
                                
                                // Start receiving data from this peer
                                if let Err(e) = receive_transaction_fragments(&sdk, &peer.device_id, &fragment_buffer).await {
                                    error!("❌ Error receiving from {}: {}", peer.device_id, e);
                                }
                                
                                // Disconnect after receiving
                                info!("🔌 Disconnecting from: {}", peer.device_id);
                                // Note: Disconnect functionality would be implemented here
                            }
                            Err(e) => {
                                warn!("⚠️  Failed to connect to peer {}: {}", peer.device_id, e);
                            }
                        }
                    }
                } else {
                    info!("🔍 No peers found, continuing scan...");
                }
            }
            Err(e) => {
                warn!("⚠️  Peer discovery failed: {}", e);
            }
        }
        
        if scan_count >= max_scans {
            if !found_peers {
                info!("⏰ No real peers found after {} scans", max_scans);
                info!("🔄 Continuing to listen for incoming connections...");
                
                // Keep listening for incoming connections
                loop {
                    sleep(Duration::from_secs(5)).await;
                    
                    // Check for new peers
                    match sdk.discover_ble_peers().await {
                        Ok(peers) => {
                            if !peers.is_empty() {
                                info!("📱 Found new peer(s): {}", peers.len());
                                for peer in &peers {
                                    info!("🔗 Attempting connection to: {}", peer.device_id);
                                    
                                    match sdk.connect_to_ble_peer(&peer.device_id).await {
                                        Ok(_) => {
                                            info!("✅ Connected to peer: {}", peer.device_id);
                                            
                                            // Start receiving data from this peer
                                            if let Err(e) = receive_transaction_fragments(&sdk, &peer.device_id, &fragment_buffer).await {
                                                error!("❌ Error receiving from {}: {}", peer.device_id, e);
                                            }
                                            
                                            // Disconnect after receiving
                                            info!("🔌 Disconnected from: {}", peer.device_id);
                                        }
                                        Err(e) => {
                                            warn!("⚠️  Failed to connect to peer {}: {}", peer.device_id, e);
                                        }
                                    }
                                }
                                break;
                            }
                        }
                        Err(e) => {
                            warn!("⚠️  Peer discovery failed: {}", e);
                        }
                    }
                }
            }
            break;
        }
        
        // Wait before next scan
        sleep(Duration::from_secs(3)).await;
    }
    
    info!("\n🏁 Offline transaction receiver completed!");
    
    Ok(())
}

/// Receive transaction fragments from a connected peer
async fn receive_transaction_fragments(
    sdk: &PolliNetSDK,
    peer_id: &str,
    fragment_buffer: &FragmentBuffer,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("📨 Receiving transaction fragments from: {}", peer_id);
    
    // Set up a timeout for receiving data
    let receive_timeout = tokio::time::timeout(
        Duration::from_secs(30),
        wait_for_transaction_data(sdk, peer_id, fragment_buffer)
    ).await;
    
    match receive_timeout {
        Ok(Ok(transaction_data)) => {
            info!("✅ Received complete transaction data: {} bytes", transaction_data.len());
            
            // ================================================================
            // STEP 3: Decompress the transaction
            // ================================================================
            info!("\n🗜️  STEP 3: Decompressing transaction...");
            
            let compressor = Lz4Compressor::new()
                .map_err(|e| format!("Failed to create compressor: {}", e))?;
            let decompressed_data = compressor.decompress_with_size(&transaction_data)
                .map_err(|e| format!("Failed to decompress transaction: {}", e))?;
            
            info!("📦 Decompressed size: {} bytes", decompressed_data.len());
            
            // Deserialize the transaction
            let transaction: Transaction = bincode1::deserialize(&decompressed_data)
                .map_err(|e| format!("Failed to deserialize transaction: {}", e))?;
            
            info!("✅ Transaction deserialized successfully");
            info!("   Signature: {}", transaction.signatures[0]);
            info!("   Instructions: {}", transaction.message.instructions.len());
            
            // ================================================================
            // STEP 4: Submit transaction to blockchain
            // ================================================================
            info!("\n🌐 STEP 4: Submitting transaction to blockchain...");
            
            let rpc_url = "https://solana-devnet.g.alchemy.com/v2/XuGpQPCCl-F1SSI-NYtsr0mSxQ8P8ts6";
            let rpc_client = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());
            
            info!("🌐 Connecting to Solana RPC: {}", rpc_url);
            
            // Submit the transaction
            match rpc_client.send_and_confirm_transaction(&transaction) {
                Ok(signature) => {
                    info!("✅ Transaction submitted successfully!");
                    info!("   Signature: {}", signature);
                    info!("   View on Solana Explorer: https://explorer.solana.com/tx/{}?cluster=devnet", signature);
                }
                Err(e) => {
                    error!("❌ Failed to submit transaction: {}", e);
                    return Err(format!("Transaction submission failed: {}", e).into());
                }
            }
        }
        Ok(Err(e)) => {
            error!("❌ Error receiving transaction data: {}", e);
        }
        Err(_) => {
            warn!("⏰ Timeout waiting for transaction data from {}", peer_id);
        }
    }
    
    Ok(())
}

/// Wait for transaction data from a connected peer
async fn wait_for_transaction_data(
    sdk: &PolliNetSDK,
    peer_id: &str,
    fragment_buffer: &FragmentBuffer,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    info!("⏳ Waiting for transaction data from: {}", peer_id);
    
    // Wait for real transaction data from BLE peer
    let mut attempts = 0;
    let max_attempts = 30; // 30 seconds timeout
    
    while attempts < max_attempts {
        // Check if we have received any data from this peer
        {
            let buffer = fragment_buffer.read().await;
            if let Some(peer_data) = buffer.get(peer_id) {
                if !peer_data.is_empty() {
                    info!("✅ Received transaction data: {} bytes", peer_data.len());
                    return Ok(peer_data.clone());
                }
            }
        }
        
        // Also check for incoming text messages which might contain transaction data
        if let Ok(messages) = sdk.check_incoming_messages().await {
            for message in messages {
                info!("📨 Received message: {}", message);
                
                // Try to parse as base64 encoded transaction data
                if let Ok(decoded_data) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &message) {
                    info!("✅ Decoded transaction data from message: {} bytes", decoded_data.len());
                    
                    // Store in fragment buffer for this peer
                    {
                        let mut buffer = fragment_buffer.write().await;
                        let peer_buffer = buffer.entry(peer_id.to_string()).or_insert_with(Vec::new);
                        peer_buffer.extend_from_slice(&decoded_data);
                    }
                    
                    return Ok(decoded_data);
                }
            }
        }
        
        // Wait a bit before checking again
        sleep(Duration::from_millis(1000)).await;
        attempts += 1;
        
        if attempts % 5 == 0 {
            info!("⏳ Still waiting for data from {}... ({}s)", peer_id, attempts);
        }
    }
    
    Err(format!("Timeout waiting for transaction data from {}", peer_id).into())
}

