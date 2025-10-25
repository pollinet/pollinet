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
    // STEP 3: Initialize PolliNet SDK and start BLE operations
    // ================================================================
    info!("\n📡 STEP 3: Starting BLE advertising and transmission...");
        
    // Start BLE advertising
    sdk.start_ble_networking().await?;
    info!("📢 BLE advertising started");
    
    // // Start text message listener
    // sdk.start_text_listener().await?;
    // info!("🎧 Text message listener started");
    
    // ================================================================
    // STEP 4: Wait for connections and send transaction fragments
    // ================================================================
    info!("\n🔄 STEP 4: Waiting for BLE connections to send transaction...");
    info!("   Looking for PolliNet devices to connect to...");
    info!("   Transaction will be sent as fragments over GATT session");
    
    let mut connection_attempts = 0;
    let max_attempts = 10;
    
    loop {
        connection_attempts += 1;
        
        // Discover peers
        match sdk.discover_ble_peers().await {
            Ok(peers) => {
                if !peers.is_empty() {
                    info!("🔍 Found {} peer(s):", peers.len());
                    for (i, peer) in peers.iter().enumerate() {
                        info!("   {}. {} (RSSI: {})", i + 1, peer.device_id, peer.rssi);
                    }
                    
                    // Try to connect to the first peer
                    let target_peer = &peers[0];
                    info!("🔗 Attempting connection to: {}", target_peer.device_id);
                    
                    match sdk.connect_to_ble_peer(&target_peer.device_id).await {
                        Ok(_) => {
                            info!("✅ Connected to peer: {}", target_peer.device_id);
                            
                            // Fragment the compressed transaction using SDK method
                            info!("📤 Fragmenting compressed transaction using SDK...");
                            let fragments = sdk.fragment_transaction(&compressed_data);
                            info!("   Created {} fragments for transmission", fragments.len());
                            
                            // Send fragments using SDK relay method
                            match sdk.relay_transaction(fragments).await {
                                Ok(_) => {
                                    info!("✅ Transaction fragments sent successfully!");
                                    info!("   Sent {} bytes to peer: {}", compressed_data.len(), target_peer.device_id);
                                    
                                    // Mark nonce as used using SDK method
                                    bundle.mark_used(index)
                                        .map_err(|e| format!("Failed to mark nonce as used: {}", e))?;
                                    
                                    // Save updated bundle using SDK method
                                    bundle.save_to_file(BUNDLE_FILE)
                                        .map_err(|e| format!("Failed to save updated bundle: {}", e))?;
                                    
                                    info!("💾 Updated bundle saved (nonce marked as used)");
                                    
                                    // Wait a bit then exit
                                    sleep(Duration::from_secs(2)).await;
                                    break;
                                }
                                Err(e) => {
                                    error!("❌ Failed to send transaction fragments: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            warn!("⚠️  Failed to connect to peer: {}", e);
                        }
                    }
                } else {
                    info!("🔍 No peers found, scanning... (attempt {}/{})", connection_attempts, max_attempts);
                }
            }
            Err(e) => {
                warn!("⚠️  Peer discovery failed: {}", e);
            }
        }
        
        if connection_attempts >= max_attempts {
            warn!("⏰ Maximum connection attempts reached, exiting...");
            break;
        }
        
        // Wait before next attempt
        sleep(Duration::from_secs(3)).await;
    }
    
    info!("\n🏁 Offline transaction sender completed!");
    info!("   Bundle file: {}", BUNDLE_FILE);
    info!("   Transaction sent as compressed fragments over BLE");
    
    Ok(())
}

