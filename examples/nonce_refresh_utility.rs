//! Example: Nonce Refresh Utility
//!
//! This example demonstrates the nonce refresh workflow:
//!
//! 1. Check the nonce JSON files created
//! 2. Refresh the used nonce data and save the new nonce data to JSON file
//!
//! Run with: cargo run --example nonce_refresh_utility

use pollinet::transaction::OfflineTransactionBundle;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use std::fs;
use std::path::Path;
use tracing::{info, warn, error};

const BUNDLE_FILE: &str = "./offline_bundle.json";
const REFRESHED_BUNDLE_FILE: &str = "./refreshed_offline_bundle.json";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("🔄 === PolliNet Nonce Refresh Utility ===");
    info!("This example demonstrates refreshing nonce data from blockchain");

    // ================================================================
    // STEP 1: Check if nonce JSON files exist
    // ================================================================
    info!("\n📁 STEP 1: Checking for nonce JSON files...");
    
    if !Path::new(BUNDLE_FILE).exists() {
        error!("❌ No offline bundle file found: {}", BUNDLE_FILE);
        error!("   Please run offline_transaction_sender first to create a bundle");
        return Err("No bundle file found".into());
    }
    
    info!("✅ Found offline bundle file: {}", BUNDLE_FILE);
    
    // Load the existing bundle
    let bundle_data = fs::read_to_string(BUNDLE_FILE)
        .map_err(|e| format!("Failed to read bundle file: {}", e))?;
    
    let mut bundle: OfflineTransactionBundle = serde_json::from_str(&bundle_data)
        .map_err(|e| format!("Failed to parse bundle file: {}", e))?;
    
    info!("📊 Bundle loaded:");
    info!("   Created at: {}", bundle.created_at);
    info!("   Total nonce accounts: {}", bundle.nonce_caches.len());
    
    let used_nonces = bundle.nonce_caches.iter().filter(|n| n.used).count();
    let unused_nonces = bundle.nonce_caches.len() - used_nonces;
    
    info!("   Used nonces: {}", used_nonces);
    info!("   Unused nonces: {}", unused_nonces);
    
    if unused_nonces == 0 {
        warn!("⚠️  No unused nonces found in bundle");
        warn!("   All nonces have been used, consider creating a new bundle");
        return Ok(());
    }
    
    // ================================================================
    // STEP 2: Connect to Solana RPC and refresh nonce data
    // ================================================================
    info!("\n🌐 STEP 2: Connecting to Solana blockchain...");
    
    let rpc_url = "https://solana-devnet.g.alchemy.com/v2/XuGpQPCCl-F1SSI-NYtsr0mSxQ8P8ts6";
    let rpc_client = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());
    
    info!("🔗 Connected to Solana RPC: {}", rpc_url);
    
    // Test connection
    match rpc_client.get_health() {
        Ok(_) => info!("✅ RPC connection healthy"),
        Err(e) => {
            error!("❌ RPC connection failed: {}", e);
            return Err(format!("RPC connection failed: {}", e).into());
        }
    }
    
    // ================================================================
    // STEP 3: Refresh nonce data for unused nonces
    // ================================================================
    info!("\n🔄 STEP 3: Refreshing nonce data...");
    
    let mut refreshed_count = 0;
    let mut error_count = 0;
    
    let total_nonces = bundle.nonce_caches.len();
    for (i, nonce_info) in bundle.nonce_caches.iter_mut().enumerate() {
        if nonce_info.used {
            info!("⏭️  Skipping used nonce {}/{}: {}", 
                  i + 1, total_nonces, nonce_info.nonce_account);
            continue;
        }
        
        info!("🔄 Refreshing nonce {}/{}: {}", 
              i + 1, total_nonces, nonce_info.nonce_account);
        
        // Parse the nonce account pubkey
        let nonce_pubkey = nonce_info.nonce_account.parse()
            .map_err(|e| format!("Invalid nonce account pubkey: {}", e))?;
        
        // Get the current blockhash from blockchain
        match rpc_client.get_latest_blockhash() {
            Ok(new_blockhash) => {
                let old_blockhash = nonce_info.blockhash.clone();
                nonce_info.blockhash = new_blockhash.to_string();
                
                info!("   ✅ Blockhash refreshed: {} -> {}", old_blockhash, new_blockhash);
                refreshed_count += 1;
            }
            Err(e) => {
                error!("   ❌ Failed to refresh nonce: {}", e);
                error_count += 1;
                
                // Check if the nonce account still exists
                match rpc_client.get_account(&nonce_pubkey) {
                    Ok(account) => {
                        info!("   ℹ️  Account exists but nonce query failed");
                    }
                    Err(account_err) => {
                        warn!("   ⚠️  Nonce account not found - may have been closed: {}", account_err);
                        nonce_info.used = true; // Mark as used since it's no longer valid
                    }
                }
            }
        }
        
        // Small delay to avoid rate limiting
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    
    info!("\n📊 Refresh summary:");
    info!("   ✅ Successfully refreshed: {} nonces", refreshed_count);
    info!("   ❌ Failed to refresh: {} nonces", error_count);
    info!("   📝 Total processed: {} nonces", refreshed_count + error_count);
    
    // ================================================================
    // STEP 4: Save refreshed bundle to new JSON file
    // ================================================================
    info!("\n💾 STEP 4: Saving refreshed bundle...");
    
    // Update the creation timestamp
    bundle.created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    // Create the refreshed bundle
    let refreshed_bundle = OfflineTransactionBundle {
        nonce_caches: bundle.nonce_caches.clone(),
        max_transactions: bundle.max_transactions,
        created_at: bundle.created_at,
    };
    
    // Save to new file
    let refreshed_json = serde_json::to_string_pretty(&refreshed_bundle)
        .map_err(|e| format!("Failed to serialize refreshed bundle: {}", e))?;
    
    fs::write(REFRESHED_BUNDLE_FILE, refreshed_json)
        .map_err(|e| format!("Failed to write refreshed bundle file: {}", e))?;
    
    info!("✅ Refreshed bundle saved to: {}", REFRESHED_BUNDLE_FILE);
    
    // Also update the original file
    let updated_json = serde_json::to_string_pretty(&bundle)
        .map_err(|e| format!("Failed to serialize updated bundle: {}", e))?;
    
    fs::write(BUNDLE_FILE, updated_json)
        .map_err(|e| format!("Failed to write updated bundle file: {}", e))?;
    
    info!("✅ Original bundle updated: {}", BUNDLE_FILE);
    
    // ================================================================
    // STEP 5: Display final statistics
    // ================================================================
    info!("\n📈 STEP 5: Final statistics:");
    
    let final_used = bundle.nonce_caches.iter().filter(|n| n.used).count();
    let final_unused = bundle.nonce_caches.len() - final_used;
    
    info!("   📁 Original bundle: {}", BUNDLE_FILE);
    info!("   📁 Refreshed bundle: {}", REFRESHED_BUNDLE_FILE);
    info!("   📊 Total nonce accounts: {}", bundle.nonce_caches.len());
    info!("   ✅ Unused nonces: {}", final_unused);
    info!("   ❌ Used nonces: {}", final_used);
    info!("   🔄 Refreshed nonces: {}", refreshed_count);
    info!("   ⏰ Last refreshed: {}", bundle.created_at);
    
    if final_unused > 0 {
        info!("\n🎉 Nonce refresh completed successfully!");
        info!("   You can now use the refreshed nonces for offline transactions");
    } else {
        warn!("\n⚠️  All nonces have been used");
        warn!("   Consider creating a new bundle with fresh nonce accounts");
    }
    
    Ok(())
}

/// Display detailed information about a nonce account
async fn display_nonce_info(
    rpc_client: &RpcClient,
    nonce_account: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let nonce_pubkey = nonce_account.parse()
        .map_err(|e| format!("Invalid nonce account pubkey: {}", e))?;
    
    info!("🔍 Detailed nonce account information:");
    info!("   Account: {}", nonce_account);
    
    // Get account info
    match rpc_client.get_account(&nonce_pubkey) {
        Ok(account) => {
            info!("   Balance: {} lamports", account.lamports);
            info!("   Owner: {}", account.owner);
            info!("   Executable: {}", account.executable);
            info!("   Rent Epoch: {}", account.rent_epoch);
        }
        Err(e) => {
            error!("   ❌ Error getting account info: {}", e);
        }
    }
    
    // Get nonce value (using get_account to get nonce account data)
    match rpc_client.get_account(&nonce_pubkey) {
        Ok(account) => {
            if account.data.len() >= 80 {
                // Parse nonce account data to get nonce value
                let nonce_data = &account.data[40..72]; // Nonce value is at offset 40-72
                let nonce_value = solana_sdk::hash::Hash::new_from_array(
                    nonce_data.try_into().unwrap_or([0; 32])
                );
                info!("   Nonce value: {}", nonce_value);
            } else {
                warn!("   ⚠️  Invalid nonce account data length");
            }
        }
        Err(e) => {
            error!("   ❌ Error getting nonce value: {}", e);
        }
    }
    
    Ok(())
}

/// Validate nonce account status
async fn validate_nonce_account(
    rpc_client: &RpcClient,
    nonce_account: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let nonce_pubkey = nonce_account.parse()
        .map_err(|e| format!("Invalid nonce account pubkey: {}", e))?;
    
    // Check if account exists
    match rpc_client.get_account(&nonce_pubkey) {
        Ok(account) => {
            // Check if it's a valid nonce account
            if account.owner == solana_sdk::system_program::id() && account.data.len() == 80 {
                info!("✅ Nonce account is valid: {}", nonce_account);
                Ok(true)
            } else {
                warn!("⚠️  Account exists but is not a valid nonce account: {}", nonce_account);
                Ok(false)
            }
        }
        Err(e) => {
            warn!("⚠️  Nonce account not found: {}", nonce_account);
            Ok(false)
        }
    }
}
