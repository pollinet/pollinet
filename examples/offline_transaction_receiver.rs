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
use tracing::{info, warn, error, debug};
use tokio::time::{sleep, Duration};

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
    let max_scans = 20;
    
    loop {
        scan_count += 1;
        info!("\n🔍 Scan #{}/{}", scan_count, max_scans);
        
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
            info!("⏰ Maximum scans reached, exiting...");
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
            
            let rpc_url = "https://api.devnet.solana.com";
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
    
    let mut received_bytes = 0;
    let mut timeout_count = 0;
    let max_timeouts = 10;
    
    loop {
        // Check for incoming messages
        match sdk.check_incoming_messages().await {
            Ok(messages) => {
                for message in messages {
                    debug!("📨 Received message: {} bytes", message.len());
                    
                    // Add to fragment buffer
                    {
                        let mut buffer = fragment_buffer.write().await;
                        let peer_buffer = buffer.entry(peer_id.to_string()).or_insert_with(Vec::new);
                        peer_buffer.extend_from_slice(message.as_bytes());
                        received_bytes += message.len();
                    }
                    
                    info!("📦 Received {} bytes from {} (total: {} bytes)", 
                          message.len(), peer_id, received_bytes);
                }
            }
            Err(e) => {
                debug!("⚠️  Error checking messages: {}", e);
            }
        }
        
        // Check if we have enough data (simulate fragment completion)
        // In a real implementation, this would check for complete fragments
        if received_bytes > 0 {
            // Simulate receiving complete transaction after some data
            if received_bytes >= 100 { // Arbitrary threshold for demo
                info!("✅ Complete transaction data received: {} bytes", received_bytes);
                
                // Extract the complete data
                let mut buffer = fragment_buffer.write().await;
                if let Some(peer_data) = buffer.remove(peer_id) {
                    return Ok(peer_data);
                }
            }
        }
        
        // Check for timeout
        timeout_count += 1;
        if timeout_count >= max_timeouts {
            return Err("Timeout waiting for complete transaction data".into());
        }
        
        // Wait before next check
        sleep(Duration::from_millis(500)).await;
    }
}

/// Simulate receiving a complete transaction (for testing)
async fn simulate_transaction_reception(
    sdk: &PolliNetSDK,
    peer_id: &str,
    fragment_buffer: &FragmentBuffer,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    info!("🧪 Simulating transaction reception from: {}", peer_id);
    
    // Create a mock transaction for testing
    let mock_transaction_data = create_mock_transaction_data().await?;
    
    // Simulate receiving it in chunks
    let chunk_size = 100;
    let mut offset = 0;
    
    while offset < mock_transaction_data.len() {
        let end = std::cmp::min(offset + chunk_size, mock_transaction_data.len());
        let chunk = &mock_transaction_data[offset..end];
        
        // Add to fragment buffer
        {
            let mut buffer = fragment_buffer.write().await;
            let peer_buffer = buffer.entry(peer_id.to_string()).or_insert_with(Vec::new);
            peer_buffer.extend_from_slice(chunk);
        }
        
        info!("📦 Simulated chunk: {} bytes (offset: {})", chunk.len(), offset);
        offset = end;
        
        // Small delay to simulate real reception
        sleep(Duration::from_millis(100)).await;
    }
    
    // Return the complete data
    let mut buffer = fragment_buffer.write().await;
    if let Some(peer_data) = buffer.remove(peer_id) {
        info!("✅ Complete simulated transaction: {} bytes", peer_data.len());
        return Ok(peer_data);
    }
    
    Err("Failed to simulate transaction reception".into())
}

/// Create mock transaction data for testing
async fn create_mock_transaction_data() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use solana_sdk::system_instruction;
    use solana_sdk::signature::Keypair;
    use solana_sdk::signer::Signer;
    
    // Create a mock transaction
    let sender_keypair = Keypair::new();
    let recipient_keypair = Keypair::new();
    
    let transfer_instruction = system_instruction::transfer(
        &sender_keypair.pubkey(),
        &recipient_keypair.pubkey(),
        1000, // 1000 lamports
    );
    
    let mut transaction = Transaction::new_with_payer(
        &[transfer_instruction],
        Some(&sender_keypair.pubkey()),
    );
    
    // Use a mock recent blockhash
    let recent_blockhash = solana_sdk::hash::Hash::new_unique();
    transaction.sign(&[&sender_keypair], recent_blockhash);
    
    // Serialize and compress
    let transaction_bytes = bincode1::serialize(&transaction)?;
    let compressor = Lz4Compressor::new()?;
    let compressed_data = compressor.compress_with_size(&transaction_bytes)?;
    
    info!("🧪 Created mock transaction: {} bytes -> {} bytes compressed", 
          transaction_bytes.len(), compressed_data.len());
    
    Ok(compressed_data)
}
