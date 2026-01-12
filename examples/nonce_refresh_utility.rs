//! Example: Nonce Refresh Utility
//!
//! This example demonstrates the nonce refresh workflow:
//!
//! 1. If bundle file doesn't exist: Creates a new bundle with nonce accounts
//! 2. If bundle file exists: Refreshes used nonce data and saves to JSON file
//!
//! Run with: cargo run --example nonce_refresh_utility

mod wallet_utils;
use wallet_utils::{create_and_fund_wallet, get_rpc_url};

use pollinet::PolliNetSDK;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::signer::Signer;
use std::path::Path;
use tracing::{info, warn};

const BUNDLE_FILE: &str = ".offline_bundle.json";
const DEFAULT_NONCE_COUNT: usize = 5; // Default number of nonces to create if bundle doesn't exist
const AIRDROP_AMOUNT_SOL: f64 = 2.0; // Enough for creating nonce accounts

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("🔄 === PolliNet Nonce Refresh Utility ===");
    info!("This utility refreshes used nonces or creates a new bundle if needed");

    // Get RPC URL from .env (SOLANA_URL) or use default
    // For local validator, set SOLANA_URL=http://127.0.0.1:8899 in .env
    let rpc_url = get_rpc_url();
    info!("🌐 Using RPC endpoint: {}", rpc_url);
    let rpc_client = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::confirmed());

    // Test connection
    match rpc_client.get_health() {
        Ok(_) => info!("✅ RPC connection healthy"),
        Err(e) => {
            warn!("⚠️  RPC health check failed: {}", e);
            warn!(
                "   Make sure your local validator is running on {}",
                rpc_url
            );
            warn!("   Default Solana validator port: 8899");
        }
    }

    // ================================================================
    // STEP 1: Check if bundle file exists
    // ================================================================
    info!("\n📁 STEP 1: Checking for bundle file...");

    let bundle_exists = Path::new(BUNDLE_FILE).exists();

    if !bundle_exists {
        info!("📝 Bundle file not found: {}", BUNDLE_FILE);
        info!(
            "   Creating new bundle with {} nonce accounts...",
            DEFAULT_NONCE_COUNT
        );

        // Create a new wallet for bundle creation
        info!("\n=== Creating Wallet for Bundle Creation ===");
        let sender_keypair = create_and_fund_wallet(&rpc_client, AIRDROP_AMOUNT_SOL).await?;
        info!("✅ Wallet created: {}", sender_keypair.pubkey());

        // Initialize SDK
        let sdk = PolliNetSDK::new_with_rpc(&rpc_url).await?;

        // Create new bundle using SDK's prepare_offline_bundle
        info!("\n=== Creating New Bundle ===");
        let bundle = sdk
            .prepare_offline_bundle(DEFAULT_NONCE_COUNT, &sender_keypair, Some(BUNDLE_FILE))
            .await?;

        info!("✅ Bundle created successfully!");
        info!("   Total nonces: {}", bundle.total_nonces());
        info!("   Available nonces: {}", bundle.available_nonces());

        // Explicitly save the bundle to file
        info!("\n=== Saving Bundle to File ===");
        bundle.save_to_file(BUNDLE_FILE)?;
        info!("✅ Saved bundle to: {}", BUNDLE_FILE);

        // Verify file was created
        if Path::new(BUNDLE_FILE).exists() {
            let file_size = std::fs::metadata(BUNDLE_FILE)?.len();
            info!("   File size: {} bytes", file_size);
            info!("   File verified: exists and readable");
        } else {
            warn!(
                "⚠️  Warning: Bundle file was not created at {}",
                BUNDLE_FILE
            );
        }

        info!("\n🎉 New bundle created! You can now use it for offline transactions.");
        return Ok(());
    }

    info!("✅ Found existing bundle file: {}", BUNDLE_FILE);

    // ================================================================
    // STEP 2: Load bundle and check status
    // ================================================================
    info!("\n📊 STEP 2: Loading bundle...");

    let bundle = pollinet::transaction::OfflineTransactionBundle::load_from_file(BUNDLE_FILE)?;

    info!("📊 Bundle loaded:");
    info!("   Created at: {}", bundle.created_at);
    info!("   Total nonce accounts: {}", bundle.total_nonces());
    info!("   Used nonces: {}", bundle.used_nonces());
    info!("   Unused nonces: {}", bundle.available_nonces());

    if bundle.used_nonces() == 0 {
        info!("✅ All nonces are unused - no refresh needed!");
        info!("   Bundle is ready to use as-is");
        return Ok(());
    }

    // ================================================================
    // STEP 3: Refresh used nonces
    // ================================================================
    info!(
        "\n🔄 STEP 3: Refreshing {} used nonce(s)...",
        bundle.used_nonces()
    );

    // Create a wallet (can use existing or create new for refresh)
    info!("\n=== Creating Wallet for Nonce Refresh ===");
    let sender_keypair = create_and_fund_wallet(&rpc_client, AIRDROP_AMOUNT_SOL).await?;
    info!("✅ Wallet ready: {}", sender_keypair.pubkey());

    // Initialize SDK
    let sdk = PolliNetSDK::new_with_rpc(&rpc_url).await?;

    // Use prepare_offline_bundle which automatically refreshes used nonces
    // Pass the same count to ensure we refresh existing nonces without creating new ones
    let total_nonces = bundle.total_nonces();
    info!("   Using prepare_offline_bundle to refresh used nonces...");
    info!(
        "   This will refresh {} used nonce(s) and make them available again",
        bundle.used_nonces()
    );

    let refreshed_bundle = sdk
        .prepare_offline_bundle(total_nonces, &sender_keypair, Some(BUNDLE_FILE))
        .await?;

    // Explicitly save the refreshed bundle to file
    info!("\n=== Saving Refreshed Bundle to File ===");
    refreshed_bundle.save_to_file(BUNDLE_FILE)?;
    info!("✅ Bundle refreshed and saved successfully!");

    // ================================================================
    // STEP 4: Display final statistics
    // ================================================================
    info!("\n📈 STEP 4: Final statistics:");

    info!("   📁 Bundle file: {}", BUNDLE_FILE);
    info!(
        "   📊 Total nonce accounts: {}",
        refreshed_bundle.total_nonces()
    );
    info!(
        "   ✅ Available (unused): {}",
        refreshed_bundle.available_nonces()
    );
    info!("   ❌ Used: {}", refreshed_bundle.used_nonces());
    info!("   🔄 Refreshed: {} nonce(s)", bundle.used_nonces());

    if refreshed_bundle.available_nonces() > 0 {
        info!("\n🎉 Nonce refresh completed successfully!");
        info!(
            "   {} nonce(s) are now available for offline transactions",
            refreshed_bundle.available_nonces()
        );
        info!("   Refreshed nonces can be reused - no need to create new accounts!");
    } else {
        warn!("\n⚠️  No nonces available after refresh");
        warn!("   This should not happen - check the bundle file");
    }

    Ok(())
}
