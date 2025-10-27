//! Example: Offline Transaction Sender
//!
//! This example demonstrates the complete offline transaction sending workflow:
//!
//! 1. Check if offline bundle file exists, if not prepare and save bundle to JSON
//! 2. Fetch unused bundle from file to create compressed presigned transaction offline
//! 3. Advertise, connect and send compressed presigned transaction as fragments over GATT session
//!
//! Run with: cargo run --example offline_transaction_sender

use bs58;
use pollinet::PolliNetSDK;
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use tracing::{info, warn, error};
use tokio::time::{sleep, Duration};

const BUNDLE_FILE: &str = "./offline_bundle.json";
const RECIPIENT_ADDRESS: &str = "RtsKQm3gAGL1Tayhs7ojWE9qytWqVh4G7eJTaNJs7vX"; // System Program

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("🚀 === PolliNet Offline Transaction Sender ===");
    info!("This example demonstrates offline transaction creation and BLE transmission");

    // Load sender keypair
    let sender_private_key = "5zRwe731N375MpGuQvQoUjSMUpoXNLqsGWE9J8SoqHKfivhUpNxwt3o9Gdu6jjCby4dJRCGBA6HdBzrhvLVhUaqu";
    let private_key_bytes = bs58::decode(sender_private_key)
        .into_vec()
        .map_err(|e| format!("Failed to decode private key: {}", e))?;
    let sender_keypair = Keypair::try_from(&private_key_bytes[..])
        .map_err(|e| format!("Failed to create keypair from private key: {}", e))?;

    info!("✅ Sender loaded: {}", sender_keypair.pubkey());

    // ================================================================
    // STEP 1: Check if offline bundle file exists, if not prepare it
    // ================================================================
    info!("\n📦 STEP 1: Checking offline bundle availability...");
    
    // Use SDK method to prepare or load bundle
    let rpc_url = "https://api.devnet.solana.com";
    let sdk = PolliNetSDK::new_with_rpc(rpc_url).await?;
    
    info!("🌐 Connecting to Solana RPC: {}", rpc_url);
    
    // Prepare bundle (will load existing or create new one)
    let mut bundle = sdk.prepare_offline_bundle(2, &sender_keypair, Some(BUNDLE_FILE)).await?;
    
    info!("✅ Bundle ready with {} nonce accounts", bundle.available_nonces());

    // ================================================================
    // STEP 2: Fetch unused bundle and create compressed presigned transaction
    // ================================================================
    info!("\n🔧 STEP 2: Creating compressed presigned transaction...");
    
    // Get next available nonce using SDK method
    let (index, nonce_info) = bundle.get_next_available_nonce()
        .ok_or("No available nonces in bundle")?;
    
    info!("📋 Using nonce account: {}", nonce_info.nonce_account);
    info!("🔑 Blockhash: {}", nonce_info.blockhash);
    
    // Create offline transaction using SDK method
    let amount = LAMPORTS_PER_SOL / 1000; // 0.001 SOL
    let compressed_data = sdk.create_offline_transaction(
        &sender_keypair,
        RECIPIENT_ADDRESS,
        amount,
        &sender_keypair, // Sender is nonce authority
        &nonce_info,
    ).map_err(|e| format!("Failed to create offline transaction: {}", e))?;
    
    info!("✅ Transaction created and compressed using SDK method");
    info!("   Amount: {} lamports", amount);
    info!("   Recipient: {}", RECIPIENT_ADDRESS);
    info!("📦 Compressed size: {} bytes", compressed_data.len());
    
    // ================================================================
    // STEP 3: Reset BLE state and start fresh
    // ================================================================
    info!("\n🔄 STEP 3: Resetting BLE state...");
    
    // Reset any previous BLE connections and state
    sdk.reset_ble().await?;
    info!("✅ BLE state reset - cleared all previous connections");
    
    // Start BLE advertising
    sdk.start_ble_networking().await?;
    info!("📢 BLE advertising started fresh");
    
    // // Start text message listener
    // sdk.start_text_listener().await?;
    // info!("🎧 Text message listener started");
    
    // ================================================================
    // STEP 4: Wait for receiver to connect, then send transaction
    // ================================================================
    info!("\n🔄 STEP 4: Waiting for receiver to connect...");
    info!("   Sender is advertising and scanning for receiver");
    info!("   Receiver will connect to sender, and sender will detect the connection");
    info!("   Once connected, transaction will be sent as fragments");
    
    // Make sure we're also scanning to discover the receiver when it connects
    sdk.start_ble_scanning().await?;
    info!("🔍 Scanning started to detect receiver");
    
    // Wait for a connection to be established (receiver will connect to us)
    let mut wait_attempts = 0;
    let max_wait_attempts = 60; // Wait up to 60 seconds
    let mut peer_connected = false;
    let mut connected_peer_id = String::new();
    
    while wait_attempts < max_wait_attempts && !peer_connected {
        wait_attempts += 1;
        
        // Check if any peer has connected
        let connected_count = sdk.get_connected_clients_count().await;
        
        if wait_attempts % 5 == 0 {
            info!("⏳ Still waiting for receiver connection... ({}s, {} connected)", wait_attempts, connected_count);
        }
        
        if connected_count > 0 {
            info!("✅ Receiver has connected!");
            peer_connected = true;
            
            // Try to discover the connected peer to get its ID
            match sdk.discover_ble_peers().await {
                Ok(peers) => {
                    if !peers.is_empty() {
                        connected_peer_id = peers[0].device_id.clone();
                        info!("📱 Connected peer ID: {}", connected_peer_id);
                        
                        // Also connect back to the peer for bidirectional communication
                        match sdk.connect_to_ble_peer(&connected_peer_id).await {
                            Ok(_) => {
                                info!("✅ Established bidirectional connection with receiver");
                            }
                            Err(e) => {
                                warn!("⚠️  Could not establish reverse connection: {}", e);
                                info!("   Continuing with one-way connection...");
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("⚠️  Could not discover peer details: {}", e);
                    connected_peer_id = "receiver".to_string();
                }
            }
            
            break;
        }
        
        if wait_attempts % 5 == 0 {
            info!("⏳ Waiting for receiver connection... ({}s)", wait_attempts);
        }
        
        sleep(Duration::from_secs(1)).await;
    }
    
    if !peer_connected {
        warn!("⏰ No receiver connected after {} seconds", max_wait_attempts);
        warn!("   Make sure receiver is running and can discover this device");
        return Ok(());
    }
    
    // Now send the transaction fragments
    info!("\n📤 Sending transaction fragments...");
    
    // Fragment the compressed transaction using SDK method
    info!("🔧 Fragmenting compressed transaction using SDK...");
    let fragments = sdk.fragment_transaction(&compressed_data);
    info!("   Created {} fragments for transmission", fragments.len());
    
    // Send fragments using SDK relay method
    match sdk.relay_transaction(fragments).await {
        Ok(_) => {
            info!("✅ Transaction fragments sent successfully!");
            info!("   Sent {} bytes to receiver", compressed_data.len());
            
            // Mark nonce as used using SDK method
            bundle.mark_used(index)
                .map_err(|e| format!("Failed to mark nonce as used: {}", e))?;
            
            // Save updated bundle using SDK method
            bundle.save_to_file(BUNDLE_FILE)
                .map_err(|e| format!("Failed to save updated bundle: {}", e))?;
            
            info!("💾 Updated bundle saved (nonce marked as used)");
            
            // Wait a bit for transmission to complete
            sleep(Duration::from_secs(2)).await;
        }
        Err(e) => {
            error!("❌ Failed to send transaction fragments: {}", e);
            return Err(format!("Fragment transmission failed: {}", e).into());
        }
    }
    
    info!("\n🏁 Offline transaction sender completed!");
    info!("   Bundle file: {}", BUNDLE_FILE);
    info!("   Transaction sent as compressed fragments over BLE");
    
    Ok(())
}

