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
use bincode1;
use pollinet::PolliNetSDK;
use pollinet::transaction::{OfflineTransactionBundle, CachedNonceData};
use pollinet::util::lz::Lz4Compressor;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::system_instruction;
use solana_sdk::transaction::Transaction;
use std::fs;
use std::path::Path;
use tracing::{info, warn, error};
use tokio::time::{sleep, Duration};

const BUNDLE_FILE: &str = "./offline_bundle.json";
const RECIPIENT_ADDRESS: &str = "11111111111111111111111111111112"; // System Program

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("🚀 === PolliNet Offline Transaction Sender ===");
    info!("This example demonstrates offline transaction creation and BLE transmission");

    // Load sender keypair
    let sender_private_key = "5zRwe731N375MpGuQvQoUjSMUpoXNLqsGWE9J8SoqHKfivhUpNxwt3o9Gdu6jjCby4dJRCGBA6HdBzrhvLVhUqu";
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
    
    let bundle = if Path::new(BUNDLE_FILE).exists() {
        info!("📁 Offline bundle file found: {}", BUNDLE_FILE);
        
        // Load existing bundle
        let bundle_data = fs::read_to_string(BUNDLE_FILE)
            .map_err(|e| format!("Failed to read bundle file: {}", e))?;
        
        let bundle: OfflineTransactionBundle = serde_json::from_str(&bundle_data)
            .map_err(|e| format!("Failed to parse bundle file: {}", e))?;
        
        info!("✅ Loaded existing bundle with {} nonce accounts", bundle.nonce_caches.len());
        bundle
    } else {
        info!("📁 No offline bundle found, creating new one...");
        
        // Create new bundle online
        let rpc_url = "https://api.devnet.solana.com";
        let rpc_client = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());
        
        info!("🌐 Connecting to Solana RPC: {}", rpc_url);
        
        // Create bundle with multiple nonce accounts
        let bundle = create_offline_bundle(&rpc_client, &sender_keypair, 5).await?;
        
        // Save bundle to file
        let bundle_json = serde_json::to_string_pretty(&bundle)
            .map_err(|e| format!("Failed to serialize bundle: {}", e))?;
        
        fs::write(BUNDLE_FILE, bundle_json)
            .map_err(|e| format!("Failed to write bundle file: {}", e))?;
        
        info!("💾 Bundle saved to: {}", BUNDLE_FILE);
        bundle
    };

    // ================================================================
    // STEP 2: Fetch unused bundle and create compressed presigned transaction
    // ================================================================
    info!("\n🔧 STEP 2: Creating compressed presigned transaction...");
    
    // Get next available nonce
    let nonce_info = bundle.nonce_caches.iter()
        .find(|n| !n.used)
        .ok_or("No available nonces in bundle")?;
    
    info!("📋 Using nonce account: {}", nonce_info.nonce_account);
    info!("🔑 Blockhash: {}", nonce_info.blockhash);
    
    // Create a simple transfer transaction
    let recipient_pubkey = RECIPIENT_ADDRESS.parse()
        .map_err(|e| format!("Invalid recipient address: {}", e))?;
    
    let transfer_instruction = system_instruction::transfer(
        &sender_keypair.pubkey(),
        &recipient_pubkey,
        LAMPORTS_PER_SOL / 1000, // 0.001 SOL
    );
    
    let mut transaction = Transaction::new_with_payer(
        &[transfer_instruction],
        Some(&sender_keypair.pubkey()),
    );
    
    // Set the recent blockhash from nonce
    let recent_blockhash = nonce_info.blockhash.parse()
        .map_err(|e| format!("Invalid blockhash: {}", e))?;
    transaction.sign(&[&sender_keypair], recent_blockhash);
    
    info!("✅ Transaction created and signed");
    info!("   Amount: {} lamports", LAMPORTS_PER_SOL / 1000);
    info!("   Recipient: {}", recipient_pubkey);
    
    // Serialize the transaction
    let transaction_bytes = bincode1::serialize(&transaction)
        .map_err(|e| format!("Failed to serialize transaction: {}", e))?;
    
    info!("📦 Transaction size: {} bytes", transaction_bytes.len());
    
    // Compress the transaction using LZ4
    let compressor = Lz4Compressor::new()
        .map_err(|e| format!("Failed to create compressor: {}", e))?;
    let compressed_data = compressor.compress_with_size(&transaction_bytes)
        .map_err(|e| format!("Failed to compress transaction: {}", e))?;
    
    info!("🗜️  Compressed size: {} bytes ({}% reduction)", 
          compressed_data.len(), 
          ((transaction_bytes.len() - compressed_data.len()) as f64 / transaction_bytes.len() as f64 * 100.0) as u32);
    
    // ================================================================
    // STEP 3: Initialize PolliNet SDK and start BLE operations
    // ================================================================
    info!("\n📡 STEP 3: Starting BLE advertising and transmission...");
    
    let sdk = PolliNetSDK::new().await?;
    info!("✅ PolliNet SDK initialized");
    
    // Start BLE advertising
    sdk.start_ble_networking().await?;
    info!("📢 BLE advertising started");
    
    // Start text message listener
    sdk.start_text_listener().await?;
    info!("🎧 Text message listener started");
    
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
                            
                            // Send the compressed transaction as fragments
                            info!("📤 Sending compressed transaction as fragments...");
                            
                            match sdk.send_to_peer(&target_peer.device_id, &compressed_data).await {
                                Ok(_) => {
                                    info!("✅ Transaction fragments sent successfully!");
                                    info!("   Sent {} bytes to peer: {}", compressed_data.len(), target_peer.device_id);
                                    
                                    // Mark nonce as used and save updated bundle
                                    let mut updated_bundle = bundle.clone();
                                    if let Some(nonce) = updated_bundle.nonce_caches.iter_mut()
                                        .find(|n| n.nonce_account == nonce_info.nonce_account) {
                                        nonce.used = true;
                                    }
                                    
                                    let updated_json = serde_json::to_string_pretty(&updated_bundle)
                                        .map_err(|e| format!("Failed to serialize updated bundle: {}", e))?;
                                    
                                    fs::write(BUNDLE_FILE, updated_json)
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

/// Create an offline bundle with multiple nonce accounts
async fn create_offline_bundle(
    rpc_client: &RpcClient,
    sender_keypair: &Keypair,
    num_nonces: usize,
) -> Result<OfflineTransactionBundle, Box<dyn std::error::Error>> {
    info!("🔨 Creating offline bundle with {} nonce accounts...", num_nonces);
    
    let mut nonce_caches = Vec::new();
    
    for i in 0..num_nonces {
        info!("   Creating nonce account {}/{}...", i + 1, num_nonces);
        
        // Create nonce account keypair
        let nonce_keypair = Keypair::new();
        let nonce_account = nonce_keypair.pubkey();
        
        // Create nonce account instruction
        let create_nonce_account_ix = system_instruction::create_account(
            &sender_keypair.pubkey(),
            &nonce_account,
            rpc_client.get_minimum_balance_for_rent_exemption(80)?,
            80,
            &solana_sdk::system_program::id(),
        );
        
        let initialize_nonce_account_ix = system_instruction::authorize_nonce_account(
            &nonce_account,
            &sender_keypair.pubkey(),
            &sender_keypair.pubkey(),
        );
        
        let mut transaction = Transaction::new_with_payer(
            &[create_nonce_account_ix, initialize_nonce_account_ix],
            Some(&sender_keypair.pubkey()),
        );
        
        let recent_blockhash = rpc_client.get_latest_blockhash()?;
        transaction.sign(&[sender_keypair, &nonce_keypair], recent_blockhash);
        
        // Submit transaction
        let signature = rpc_client.send_and_confirm_transaction(&transaction)?;
        info!("   ✅ Nonce account {} created: {}", i + 1, signature);
        
        // Get current blockhash for nonce
        let current_blockhash = rpc_client.get_latest_blockhash()?;
        
        nonce_caches.push(CachedNonceData {
            nonce_account: nonce_account.to_string(),
            authority: sender_keypair.pubkey().to_string(),
            blockhash: current_blockhash.to_string(),
            lamports_per_signature: 5000, // Default fee
            cached_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            used: false,
        });
    }
    
    info!("✅ Bundle created with {} nonce accounts", nonce_caches.len());
    
    Ok(OfflineTransactionBundle {
        nonce_caches,
        max_transactions: num_nonces,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    })
}
