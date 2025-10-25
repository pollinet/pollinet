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

use pollinet::PolliNetSDK;
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
    // STEP 1: Initialize PolliNet SDK with RPC and start BLE scanning
    // ================================================================
    info!("\n📡 STEP 1: Starting BLE scanning for PolliNet devices...");
    
    let rpc_url = "https://solana-devnet.g.alchemy.com/v2/XuGpQPCCl-F1SSI-NYtsr0mSxQ8P8ts6";
    let sdk = PolliNetSDK::new_with_rpc(rpc_url).await?;
    info!("✅ PolliNet SDK initialized with RPC: {}", rpc_url);
    
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
    
    let mut connection_attempts = 0;
    let max_attempts = 20; // Increased for better discovery
    
    loop {
        connection_attempts += 1;
        info!("\n🔍 Scan attempt #{}/{}", connection_attempts, max_attempts);
        
        // Discover peers
        match sdk.discover_ble_peers().await {
            Ok(peers) => {
                if !peers.is_empty() {
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
                                
                                // Exit after successful connection and data reception
                                return Ok(());
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
        
        if connection_attempts >= max_attempts {
            warn!("⏰ Maximum connection attempts reached, continuing to listen...");
            // Continue listening indefinitely instead of exiting
        }
        
        // Wait before next attempt
        sleep(Duration::from_secs(3)).await;
    }
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
            // STEP 3: Submit compressed transaction using PolliNet SDK
            // ================================================================
            info!("\n🌐 STEP 3: Submitting transaction to blockchain using PolliNet SDK...");
            
            // Use PolliNet SDK to submit the compressed transaction
            match sdk.submit_offline_transaction(&transaction_data, true).await {
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
    _fragment_buffer: &FragmentBuffer,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    info!("⏳ Waiting for transaction data from: {}", peer_id);
    
    // Wait for real transaction data from BLE peer
    let mut attempts = 0;
    let max_attempts = 30; // 30 seconds timeout
    
    while attempts < max_attempts {
        // Check for complete transactions using SDK method
        let complete_transactions = sdk.get_complete_transactions().await;
        
        if !complete_transactions.is_empty() {
            // Get the first complete transaction
            let tx_id = &complete_transactions[0];
            info!("✅ Found complete transaction: {}", tx_id);
            
            // Get fragments for this transaction
            if let Some(fragments) = sdk.get_fragments_for_transaction(tx_id).await {
                info!("📦 Reassembling {} fragments using SDK method...", fragments.len());
                
                // Use SDK method to reassemble fragments
                match sdk.reassemble_fragments(&fragments) {
                    Ok(reassembled_data) => {
                        info!("✅ Transaction reassembled successfully: {} bytes", reassembled_data.len());
                        
                        // Clear fragments after successful reassembly
                        sdk.clear_fragments(tx_id).await;
                        
                        return Ok(reassembled_data);
                    }
                    Err(e) => {
                        error!("❌ Failed to reassemble fragments: {}", e);
                        // Clear invalid fragments
                        sdk.clear_fragments(tx_id).await;
                    }
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

