//! Wallet utility functions for examples
//!
//! Provides helper functions for creating new wallets and requesting airdrops
//! on Solana devnet for testing purposes.

use solana_client::rpc_client::RpcClient;
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

/// Create a new wallet and request an airdrop of SOL on devnet
///
/// # Arguments
/// * `rpc_client` - The RPC client connected to Solana devnet
/// * `airdrop_amount_sol` - Amount of SOL to request (default: 5.0)
///
/// # Returns
/// The newly created keypair after airdrop confirmation
pub async fn create_and_fund_wallet(
    rpc_client: &RpcClient,
    airdrop_amount_sol: f64,
) -> Result<Keypair, Box<dyn std::error::Error>> {
    info!("=== Creating New Wallet ===");

    // Generate a new keypair
    let keypair = Keypair::new();
    let pubkey = keypair.pubkey();

    info!("✅ New wallet created: {}", pubkey);
    info!("   Requesting airdrop of {} SOL...", airdrop_amount_sol);

    // Request airdrop with retry logic for rate limiting
    let airdrop_amount_lamports = (airdrop_amount_sol * LAMPORTS_PER_SOL as f64) as u64;
    let signature =
        request_airdrop_with_retry(rpc_client, &pubkey, airdrop_amount_lamports).await?;

    info!("   Airdrop transaction signature: {}", signature);
    info!("   Waiting for confirmation...");

    // Wait for confirmation with retry
    confirm_transaction_with_retry(rpc_client, &signature).await?;

    // Verify balance
    let balance = rpc_client.get_balance(&pubkey)?;
    info!("✅ Airdrop confirmed!");
    info!(
        "   Wallet balance: {} lamports ({} SOL)",
        balance,
        balance as f64 / LAMPORTS_PER_SOL as f64
    );

    if balance < airdrop_amount_lamports {
        return Err(format!(
            "Airdrop incomplete. Expected {} SOL, got {} SOL",
            airdrop_amount_sol,
            balance as f64 / LAMPORTS_PER_SOL as f64
        )
        .into());
    }

    Ok(keypair)
}

/// Request airdrop with retry logic to handle rate limiting
async fn request_airdrop_with_retry(
    rpc_client: &RpcClient,
    pubkey: &solana_sdk::pubkey::Pubkey,
    lamports: u64,
) -> Result<solana_sdk::signature::Signature, Box<dyn std::error::Error>> {
    const MAX_RETRIES: u32 = 5;
    const INITIAL_DELAY_SECS: u64 = 2;

    for attempt in 0..MAX_RETRIES {
        match rpc_client.request_airdrop(pubkey, lamports) {
            Ok(sig) => return Ok(sig),
            Err(e) => {
                let error_str = e.to_string();
                if error_str.contains("rate limit")
                    || error_str.contains("rate_limit")
                    || error_str.contains("too many")
                {
                    if attempt < MAX_RETRIES - 1 {
                        let delay_secs = INITIAL_DELAY_SECS * (attempt + 1) as u64;
                        warn!(
                            "⚠️  Airdrop rate limited (attempt {}/{}). Waiting {} seconds before retry...",
                            attempt + 1,
                            MAX_RETRIES,
                            delay_secs
                        );
                        sleep(Duration::from_secs(delay_secs)).await;
                        continue;
                    }
                }
                return Err(Box::new(e));
            }
        }
    }

    Err("Failed to request airdrop after maximum retries".into())
}

/// Confirm transaction with retry logic
async fn confirm_transaction_with_retry(
    rpc_client: &RpcClient,
    signature: &solana_sdk::signature::Signature,
) -> Result<(), Box<dyn std::error::Error>> {
    const MAX_RETRIES: u32 = 10;
    const RETRY_DELAY_SECS: u64 = 2;

    for attempt in 0..MAX_RETRIES {
        match rpc_client.confirm_transaction(signature) {
            Ok(_) => return Ok(()),
            Err(e) => {
                if attempt < MAX_RETRIES - 1 {
                    warn!(
                        "⚠️  Transaction confirmation failed (attempt {}/{}). Retrying in {} seconds...",
                        attempt + 1,
                        MAX_RETRIES,
                        RETRY_DELAY_SECS
                    );
                    sleep(Duration::from_secs(RETRY_DELAY_SECS)).await;
                    continue;
                }
                return Err(Box::new(e));
            }
        }
    }

    Err("Failed to confirm transaction after maximum retries".into())
}
